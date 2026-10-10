//! Runs an external tool against a connection. The password goes through a
//! user-only temp file (`PGPASSFILE`, `--defaults-extra-file`), never argv or
//! the environment. Stderr is streamed as log lines plus per-table progress,
//! and the run can be cancelled.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use savoia_core::{AppResult, Cancel, CancelHandle, Endpoint, SslMode};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Notify, mpsc};

use crate::tools::{Flavor, FoundTool, Tool};

/// Lines of output kept to explain a failure.
const TAIL_LINES: usize = 20;
const CHANNEL_CAPACITY: usize = 256;
/// How long to wait for output pipes to close after killing a cancelled tool.
const DRAIN_AFTER_KILL: Duration = Duration::from_secs(2);

/// A tool invocation bound to a connection. Build it with [`ToolCommand::connect`],
/// add the tool's own arguments, then [`ToolCommand::spawn`] it.
#[derive(Debug)]
pub struct ToolCommand {
    tool: Tool,
    program: PathBuf,
    args: Vec<OsString>,
    env: Vec<(&'static str, OsString)>,
    stdin: Option<PathBuf>,
    stdout: Option<PathBuf>,
    secret: Option<SecretFile>,
}

impl ToolCommand {
    /// Runs `found` with the connection options for `endpoint`.
    ///
    /// Postgres tools get the database from `endpoint`. MySQL's `mysqldump`
    /// takes it as a positional argument, so callers add it there; `mysql`
    /// gets it as `--database`.
    pub fn connect(
        found: &FoundTool,
        endpoint: &Endpoint,
        password: Option<&str>,
    ) -> io::Result<Self> {
        let mut command = Self {
            tool: found.tool,
            program: found.path.clone(),
            args: Vec::new(),
            env: Vec::new(),
            stdin: None,
            stdout: None,
            secret: None,
        };
        match found.version.flavor {
            Flavor::Postgres => command.connect_postgres(found, endpoint, password)?,
            flavor => command.connect_mysql(flavor, endpoint, password)?,
        }
        Ok(command)
    }

    fn connect_postgres(
        &mut self,
        found: &FoundTool,
        endpoint: &Endpoint,
        password: Option<&str>,
    ) -> io::Result<()> {
        // Behind a tunnel, connect to the local address but verify TLS (and
        // look up the password) against the real host name.
        let behind_tunnel =
            endpoint.host != endpoint.tls_server_name && endpoint.host.parse::<IpAddr>().is_ok();
        if behind_tunnel {
            self.arg(format!("--host={}", endpoint.tls_server_name));
            self.env.push(("PGHOSTADDR", endpoint.host.clone().into()));
        } else {
            self.arg(format!("--host={}", endpoint.host));
        }
        self.arg(format!("--port={}", endpoint.port));
        self.arg(format!("--username={}", endpoint.user));
        if let Some(database) = &endpoint.database {
            // A plain `--dbname` containing `=` would be read as a connection string.
            self.arg(format!("--dbname=dbname={}", conninfo_quote(database)));
        }
        // Never prompt: stdin isn't a terminal and the prompt would hang.
        self.arg("--no-password");

        let sslmode = match endpoint.ssl {
            SslMode::Disable => "disable",
            SslMode::Prefer => "prefer",
            SslMode::Require => "require",
            SslMode::VerifyFull => "verify-full",
        };
        self.env.push(("PGSSLMODE", sslmode.into()));
        // The app verifies against the OS trust store; libpq can since 16.
        if endpoint.ssl == SslMode::VerifyFull && found.version.version.major >= 16 {
            self.env.push(("PGSSLROOTCERT", "system".into()));
        }
        self.env.push(("PGCONNECT_TIMEOUT", "10".into()));
        self.env.push(("PGAPPNAME", "Savoia Studio".into()));
        if endpoint.read_only {
            self.env
                .push(("PGOPTIONS", "-c default_transaction_read_only=on".into()));
        }
        if let Some(password) = password {
            let secret = SecretFile::create(&pgpass_line(password))?;
            self.env.push(("PGPASSFILE", secret.path.clone().into()));
            self.secret = Some(secret);
        }
        Ok(())
    }

    fn connect_mysql(
        &mut self,
        flavor: Flavor,
        endpoint: &Endpoint,
        password: Option<&str>,
    ) -> io::Result<()> {
        if let Some(password) = password {
            let secret = SecretFile::create(&format!(
                "[client]\npassword={}\n",
                option_file_quote(password)
            ))?;
            // The tools only honour this option when it comes first.
            let mut arg = OsString::from("--defaults-extra-file=");
            arg.push(&secret.path);
            self.args.push(arg);
            self.secret = Some(secret);
        }
        self.arg(format!("--host={}", endpoint.host));
        self.arg(format!("--port={}", endpoint.port));
        self.arg(format!("--user={}", endpoint.user));
        // Otherwise "localhost" means the Unix socket, not the endpoint's port.
        self.arg("--protocol=TCP");
        for arg in mysql_ssl_args(flavor, endpoint.ssl) {
            self.arg(arg);
        }
        if self.tool == Tool::Mysql {
            if let Some(database) = &endpoint.database {
                self.arg(format!("--database={database}"));
            }
            if endpoint.read_only {
                self.arg("--init-command=SET SESSION TRANSACTION READ ONLY");
            }
        }
        Ok(())
    }

    pub fn arg(&mut self, arg: impl Into<OsString>) -> &mut Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I: IntoIterator<Item = S>, S: Into<OsString>>(&mut self, args: I) -> &mut Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Feeds `path` to the tool's stdin (e.g. `mysql < dump.sql`).
    pub fn stdin_file(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.stdin = Some(path.into());
        self
    }

    /// Writes the tool's stdout to `path` instead of the log.
    pub fn stdout_file(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.stdout = Some(path.into());
        self
    }

    /// The command as shown in the log. Holds no secrets: none are on argv.
    pub fn command_line(&self) -> String {
        let mut line = self.program.display().to_string();
        for arg in &self.args {
            line.push(' ');
            line.push_str(&arg.to_string_lossy());
        }
        if let Some(path) = &self.stdin {
            line.push_str(&format!(" < {}", path.display()));
        }
        if let Some(path) = &self.stdout {
            line.push_str(&format!(" > {}", path.display()));
        }
        line
    }

    /// Starts the tool. Needs a Tokio runtime. The password file lives until
    /// the tool exits.
    pub fn spawn(self) -> io::Result<ToolRun> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            // Inherited credentials would bypass the ones we pass.
            .env_remove("PGPASSWORD")
            .env_remove("MYSQL_PWD")
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .stdin(match &self.stdin {
                Some(path) => Stdio::from(File::open(path)?),
                None => Stdio::null(),
            })
            .stdout(match &self.stdout {
                Some(path) => Stdio::from(File::create(path)?),
                None => Stdio::piped(),
            })
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let child = command.spawn()?;

        let (tx, events) = mpsc::channel(CHANNEL_CAPACITY);
        let cancel = Arc::new(Notify::new());
        let first = RunEvent::Log(format!("$ {}", self.command_line()));
        tx.try_send(first).expect("a new channel has room");
        tokio::spawn(supervise(child, self.tool, tx, cancel.clone(), self.secret));
        Ok(ToolRun { events, cancel })
    }
}

/// What a running tool reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    /// A line of output, starting with the command line itself.
    Log(String),
    /// The tool started on this table (`schema.table` for Postgres).
    Table(String),
    /// Always the last event.
    Finished(RunEnd),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEnd {
    Succeeded,
    /// The exit status and the output lines that explain it.
    Failed(String),
    Cancelled,
}

/// The UI's end of a running tool. Dropping it cancels the run.
#[derive(Debug)]
pub struct ToolRun {
    events: mpsc::Receiver<RunEvent>,
    cancel: Arc<Notify>,
}

impl ToolRun {
    /// The next event, or `None` after [`RunEvent::Finished`].
    pub async fn next(&mut self) -> Option<RunEvent> {
        self.events.recv().await
    }

    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(KillTool(self.cancel.clone()))
    }

    /// Waits for the end, returning it with every log line.
    pub async fn wait(mut self) -> (RunEnd, Vec<String>) {
        let mut log = Vec::new();
        while let Some(event) = self.next().await {
            match event {
                RunEvent::Log(line) => log.push(line),
                RunEvent::Table(_) => {}
                RunEvent::Finished(end) => return (end, log),
            }
        }
        (
            RunEnd::Failed("the tool's runner stopped unexpectedly".into()),
            log,
        )
    }
}

impl Drop for ToolRun {
    fn drop(&mut self) {
        self.cancel.notify_one();
    }
}

struct KillTool(Arc<Notify>);

#[async_trait]
impl Cancel for KillTool {
    async fn cancel(&self) -> AppResult<()> {
        // `notify_one` stores a permit, so a cancel before the runner waits still counts.
        self.0.notify_one();
        Ok(())
    }
}

async fn supervise(
    mut child: Child,
    tool: Tool,
    tx: mpsc::Sender<RunEvent>,
    cancel: Arc<Notify>,
    secret: Option<SecretFile>,
) {
    let stderr = child
        .stderr
        .take()
        .map(|pipe| tokio::spawn(read_lines(pipe, tool, tx.clone())));
    let stdout = child
        .stdout
        .take()
        .map(|pipe| tokio::spawn(read_lines(pipe, tool, tx.clone())));
    let status = tokio::select! {
        status = child.wait() => Some(status),
        () = cancel.notified() => {
            let _ = child.kill().await;
            None
        }
    };
    // Kept until here: tools may reconnect (e.g. `pg_dump --jobs`) while running.
    drop(secret);

    let mut tail = Vec::new();
    for reader in [stderr, stdout].into_iter().flatten() {
        let abort = reader.abort_handle();
        match tokio::time::timeout(DRAIN_AFTER_KILL, reader).await {
            Ok(Ok(lines)) => tail.extend(lines),
            // A grandchild may still hold the pipe open.
            Ok(Err(_)) | Err(_) => abort.abort(),
        }
    }
    let end = match status {
        None => RunEnd::Cancelled,
        Some(Ok(status)) if status.success() => RunEnd::Succeeded,
        Some(Ok(status)) => RunEnd::Failed(failure_message(tool, status.code(), &tail)),
        Some(Err(err)) => RunEnd::Failed(format!("{tool}: {err}")),
    };
    let _ = tx.send(RunEvent::Finished(end)).await;
}

/// Streams `pipe` line by line, returning the last [`TAIL_LINES`].
async fn read_lines(
    pipe: impl AsyncRead + Unpin,
    tool: Tool,
    tx: mpsc::Sender<RunEvent>,
) -> Vec<String> {
    let mut reader = BufReader::new(pipe);
    let mut tail = VecDeque::with_capacity(TAIL_LINES);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf)
            .trim_end_matches(['\r', '\n'])
            .to_owned();
        if let Some(table) = table_progress(tool, &line)
            && tx.send(RunEvent::Table(table)).await.is_err()
        {
            break;
        }
        if tail.len() == TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line.clone());
        if tx.send(RunEvent::Log(line)).await.is_err() {
            break;
        }
    }
    tail.into()
}

/// The table a verbose progress line announces.
fn table_progress(tool: Tool, line: &str) -> Option<String> {
    let unquote = |name: &str| name.trim_matches('"').to_owned();
    match tool {
        Tool::PgDump => line
            .split_once(": dumping contents of table ")
            .map(|(_, name)| unquote(name)),
        Tool::PgRestore => line
            .split_once(": processing data for table ")
            .map(|(_, name)| unquote(name)),
        Tool::Mysqldump => line
            .strip_prefix("-- Retrieving table structure for table ")
            .map(|name| name.trim_end_matches("...").trim_matches('`').to_owned()),
        Tool::Psql | Tool::Mysql => None,
    }
}

/// "pg_dump exited with code 1: <the lines that say why>".
fn failure_message(tool: Tool, code: Option<i32>, tail: &[String]) -> String {
    let status = match code {
        Some(code) => format!("{tool} exited with code {code}"),
        None => format!("{tool} was stopped by a signal"),
    };
    let errors: Vec<&str> = tail
        .iter()
        .map(|line| line.trim())
        .filter(|line| line.to_ascii_lowercase().contains("error"))
        .collect();
    let detail: Vec<&str> = if errors.is_empty() {
        tail.iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect()
    } else {
        errors
    };
    match detail.len() {
        0 => status,
        n => format!("{status}: {}", detail[n.saturating_sub(3)..].join("\n")),
    }
}

/// One `.pgpass` line matching any host, port, database and user.
fn pgpass_line(password: &str) -> String {
    let escaped = password.replace('\\', "\\\\").replace(':', "\\:");
    format!("*:*:*:*:{escaped}\n")
}

/// A libpq connection-string value: single-quoted, `\` and `'` escaped.
fn conninfo_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// A MySQL option-file value: double-quoted, with backslash escapes.
fn option_file_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for c in value.chars() {
        match c {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// MySQL and MariaDB clients spell the TLS options differently. Recent
/// MariaDB clients verify the certificate by default, which `Prefer` and
/// `Require` must turn off to match the app's driver.
fn mysql_ssl_args(flavor: Flavor, ssl: SslMode) -> Vec<String> {
    let ca = || system_ca_bundle().map(|path| format!("--ssl-ca={}", path.display()));
    match (flavor, ssl) {
        (Flavor::Mariadb, SslMode::Disable) => vec!["--skip-ssl".into()],
        (Flavor::Mariadb, SslMode::Prefer) => vec!["--skip-ssl-verify-server-cert".into()],
        (Flavor::Mariadb, SslMode::Require) => {
            vec!["--ssl".into(), "--skip-ssl-verify-server-cert".into()]
        }
        (Flavor::Mariadb, SslMode::VerifyFull) => {
            let mut args = vec!["--ssl".into(), "--ssl-verify-server-cert".into()];
            args.extend(ca());
            args
        }
        (_, SslMode::Disable) => vec!["--ssl-mode=DISABLED".into()],
        (_, SslMode::Prefer) => vec!["--ssl-mode=PREFERRED".into()],
        (_, SslMode::Require) => vec!["--ssl-mode=REQUIRED".into()],
        (_, SslMode::VerifyFull) => {
            let mut args = vec!["--ssl-mode=VERIFY_IDENTITY".into()];
            args.extend(ca());
            args
        }
    }
}

/// The OS's PEM bundle of trusted CAs, which MySQL clients need to verify a
/// server certificate. None on Windows, where the user must configure it.
fn system_ca_bundle() -> Option<PathBuf> {
    [
        "/etc/ssl/cert.pem",
        "/etc/ssl/certs/ca-certificates.crt",
        "/etc/pki/tls/certs/ca-bundle.crt",
        "/etc/ssl/ca-bundle.pem",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

/// A temp file only the current user can read, deleted on drop.
#[derive(Debug)]
struct SecretFile {
    path: PathBuf,
}

impl SecretFile {
    fn create(contents: &str) -> io::Result<Self> {
        Self::create_in(&std::env::temp_dir(), contents)
    }

    fn create_in(dir: &Path, contents: &str) -> io::Result<Self> {
        let path = dir.join(format!("savoia-{}.cnf", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        // On Windows the temp directory is already per-user.
        let mut file = options.open(&path)?;
        let secret = Self { path };
        file.write_all(contents.as_bytes())?;
        Ok(secret)
    }
}

impl Drop for SecretFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolVersion, Version};
    use savoia_core::Engine;

    fn found(tool: Tool, flavor: Flavor, major: u32) -> FoundTool {
        FoundTool {
            tool,
            path: PathBuf::from(format!("/usr/bin/{}", tool.name())),
            version: ToolVersion {
                flavor,
                version: Version {
                    major,
                    minor: 0,
                    patch: 0,
                },
            },
        }
    }

    fn endpoint(engine: Engine) -> Endpoint {
        Endpoint {
            engine,
            host: "db.example.com".into(),
            port: engine.default_port(),
            tls_server_name: "db.example.com".into(),
            user: "app".into(),
            database: Some("shop".into()),
            ssl: SslMode::Require,
            read_only: false,
        }
    }

    fn args(command: &ToolCommand) -> Vec<String> {
        command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn env<'a>(command: &'a ToolCommand, key: &str) -> Option<&'a str> {
        command
            .env
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.to_str().unwrap())
    }

    #[test]
    fn postgres_password_goes_to_a_pgpass_file() {
        let command = ToolCommand::connect(
            &found(Tool::PgDump, Flavor::Postgres, 17),
            &endpoint(Engine::Postgres),
            Some("s3cr:et\\"),
        )
        .unwrap();
        assert_eq!(
            args(&command),
            [
                "--host=db.example.com",
                "--port=5432",
                "--username=app",
                "--dbname=dbname='shop'",
                "--no-password"
            ]
        );
        assert_eq!(env(&command, "PGSSLMODE"), Some("require"));
        let passfile = env(&command, "PGPASSFILE").unwrap();
        assert_eq!(
            std::fs::read_to_string(passfile).unwrap(),
            "*:*:*:*:s3cr\\:et\\\\\n"
        );
        assert!(!command.command_line().contains("s3cr"));

        let path = PathBuf::from(passfile);
        drop(command);
        assert!(
            !path.exists(),
            "the password file is deleted with the command"
        );
    }

    #[test]
    fn postgres_behind_a_tunnel_verifies_the_real_host() {
        let mut endpoint = endpoint(Engine::Postgres);
        endpoint.host = "127.0.0.1".into();
        endpoint.port = 61000;
        endpoint.ssl = SslMode::VerifyFull;
        endpoint.read_only = true;
        let command =
            ToolCommand::connect(&found(Tool::Psql, Flavor::Postgres, 17), &endpoint, None)
                .unwrap();
        assert!(args(&command).contains(&"--host=db.example.com".into()));
        assert!(args(&command).contains(&"--port=61000".into()));
        assert_eq!(env(&command, "PGHOSTADDR"), Some("127.0.0.1"));
        assert_eq!(env(&command, "PGSSLROOTCERT"), Some("system"));
        assert_eq!(
            env(&command, "PGOPTIONS"),
            Some("-c default_transaction_read_only=on")
        );
        assert_eq!(env(&command, "PGPASSFILE"), None);
    }

    #[test]
    fn database_names_are_quoted_for_libpq() {
        assert_eq!(conninfo_quote("it's=odd\\"), "'it\\'s=odd\\\\'");
    }

    #[test]
    fn mysql_password_goes_to_a_defaults_file_passed_first() {
        let mut endpoint = endpoint(Engine::Mysql);
        endpoint.read_only = true;
        let command = ToolCommand::connect(
            &found(Tool::Mysql, Flavor::Mysql, 8),
            &endpoint,
            Some("pa\"ss"),
        )
        .unwrap();
        let args = args(&command);
        let file = args[0]
            .strip_prefix("--defaults-extra-file=")
            .expect("first argument");
        assert_eq!(
            std::fs::read_to_string(file).unwrap(),
            "[client]\npassword=\"pa\\\"ss\"\n"
        );
        assert_eq!(
            args[1..],
            [
                "--host=db.example.com",
                "--port=3306",
                "--user=app",
                "--protocol=TCP",
                "--ssl-mode=REQUIRED",
                "--database=shop",
                "--init-command=SET SESSION TRANSACTION READ ONLY"
            ]
        );
    }

    #[test]
    fn mysqldump_leaves_the_database_to_the_caller() {
        let command = ToolCommand::connect(
            &found(Tool::Mysqldump, Flavor::Mysql, 8),
            &endpoint(Engine::Mysql),
            None,
        )
        .unwrap();
        assert!(!args(&command).iter().any(|arg| arg.contains("shop")));
        assert!(!args(&command)[0].starts_with("--defaults-extra-file"));
    }

    #[test]
    fn mariadb_clients_get_their_own_tls_options() {
        assert_eq!(
            mysql_ssl_args(Flavor::Mariadb, SslMode::Disable),
            ["--skip-ssl"]
        );
        assert_eq!(
            mysql_ssl_args(Flavor::Mariadb, SslMode::Require),
            ["--ssl", "--skip-ssl-verify-server-cert"]
        );
        assert_eq!(
            mysql_ssl_args(Flavor::Mysql, SslMode::Prefer),
            ["--ssl-mode=PREFERRED"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn secret_files_are_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let secret = SecretFile::create("x").unwrap();
        let mode = std::fs::metadata(&secret.path)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn parses_table_progress() {
        assert_eq!(
            table_progress(
                Tool::PgDump,
                "pg_dump: dumping contents of table \"public.widgets\""
            ),
            Some("public.widgets".into())
        );
        assert_eq!(
            table_progress(
                Tool::PgRestore,
                "pg_restore: processing data for table \"it.parts\""
            ),
            Some("it.parts".into())
        );
        assert_eq!(
            table_progress(
                Tool::Mysqldump,
                "-- Retrieving table structure for table it_parts..."
            ),
            Some("it_parts".into())
        );
        assert_eq!(
            table_progress(Tool::PgDump, "pg_dump: reading schemas"),
            None
        );
        assert_eq!(
            table_progress(Tool::Psql, "dumping contents of table x"),
            None
        );
    }

    #[test]
    fn failures_quote_the_error_lines() {
        let tail = [
            "pg_dump: reading schemas".to_owned(),
            "pg_dump: error: connection to server failed".to_owned(),
            "".to_owned(),
        ];
        assert_eq!(
            failure_message(Tool::PgDump, Some(1), &tail),
            "pg_dump exited with code 1: pg_dump: error: connection to server failed"
        );
        assert_eq!(
            failure_message(Tool::Mysql, Some(2), &["bye".to_owned()]),
            "mysql exited with code 2: bye"
        );
        assert_eq!(
            failure_message(Tool::Psql, None, &[]),
            "psql was stopped by a signal"
        );
    }

    #[cfg(unix)]
    mod running {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        /// A fake `pg_dump` running `script`, with no connection options.
        fn fake(script: &str) -> ToolCommand {
            let dir = std::env::temp_dir().join(format!("savoia-runner-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("pg_dump");
            std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            ToolCommand {
                tool: Tool::PgDump,
                program: path,
                args: Vec::new(),
                env: Vec::new(),
                stdin: None,
                stdout: None,
                secret: None,
            }
        }

        #[tokio::test]
        async fn streams_log_and_progress_then_succeeds() {
            let mut run = fake(
                "echo 'pg_dump: reading schemas' >&2\n\
                 echo 'pg_dump: dumping contents of table \"public.a\"' >&2\n\
                 echo done",
            )
            .spawn()
            .unwrap();
            let mut events = Vec::new();
            while let Some(event) = run.next().await {
                events.push(event);
            }
            assert!(matches!(&events[0], RunEvent::Log(line) if line.starts_with("$ ")));
            assert!(events.contains(&RunEvent::Table("public.a".into())));
            assert!(events.contains(&RunEvent::Log("done".into())));
            assert_eq!(events.last(), Some(&RunEvent::Finished(RunEnd::Succeeded)));
        }

        #[tokio::test]
        async fn reports_failures_with_the_error_line() {
            let (end, _) = fake("echo 'pg_dump: error: no such database' >&2; exit 1")
                .spawn()
                .unwrap()
                .wait()
                .await;
            assert_eq!(
                end,
                RunEnd::Failed(
                    "pg_dump exited with code 1: pg_dump: error: no such database".into()
                )
            );
        }

        #[tokio::test]
        async fn writes_stdout_to_a_file_and_reads_stdin_from_one() {
            let dir =
                std::env::temp_dir().join(format!("savoia-runner-io-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("in.sql"), "SELECT 1;\n").unwrap();
            let mut command = fake("cat");
            command
                .stdin_file(dir.join("in.sql"))
                .stdout_file(dir.join("out.sql"));
            let (end, _) = command.spawn().unwrap().wait().await;
            assert_eq!(end, RunEnd::Succeeded);
            assert_eq!(
                std::fs::read_to_string(dir.join("out.sql")).unwrap(),
                "SELECT 1;\n"
            );
        }

        #[tokio::test]
        async fn cancel_kills_the_tool_and_deletes_the_password_file() {
            let mut command = fake("echo started >&2; exec sleep 30");
            let secret = SecretFile::create("secret").unwrap();
            let secret_path = secret.path.clone();
            command.secret = Some(secret);
            let mut run = command.spawn().unwrap();
            // Wait until the tool is running.
            while run.next().await != Some(RunEvent::Log("started".into())) {}
            run.cancel_handle().cancel().await.unwrap();
            let started = std::time::Instant::now();
            let (end, _) = run.wait().await;
            assert_eq!(end, RunEnd::Cancelled);
            assert!(started.elapsed() < Duration::from_secs(5));
            assert!(!secret_path.exists());
        }
    }
}
