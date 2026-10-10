//! Runs a SQL script one statement at a time, reading it in chunks. The
//! statement splitter from the console finds the boundaries; the last
//! statement of each chunk waits for the next one, since it may be cut off.

use std::io::Read;
use std::path::PathBuf;

use savoia_core::split::split_from;
use savoia_core::{AppError, AppResult, Connection, Engine};

use super::{
    ImportError, ImportEvent, ImportOptions, ImportSummary, Input, Tracker, abort, end, preview,
    run, start,
};

const CHUNK: usize = 1 << 20;

#[derive(Debug, Clone)]
pub struct SqlImport {
    /// A `.sql` file, optionally gzip-compressed.
    pub path: PathBuf,
    pub options: ImportOptions,
}

/// Runs the script at `request.path` on `conn`.
///
/// `Err` means the import couldn't go on at all: the file is unreadable or
/// needs `psql`. Statements the server rejects are reported in the summary.
pub async fn import_sql(
    conn: &dyn Connection,
    engine: Engine,
    request: &SqlImport,
    progress: impl FnMut(ImportEvent) + Send,
) -> AppResult<ImportSummary> {
    let mut tracker = Tracker::new(request.options, &Input::open(&request.path)?, progress);
    // Find psql-only commands before running anything: a `pg_dump --clean`
    // script would otherwise drop the tables, then fail at its first `COPY`.
    if engine == Engine::Postgres {
        let mut input = Input::open(&request.path)?;
        run_script(conn, engine, request, &mut input, &mut tracker, Pass::Check).await?;
    }
    let mut input = Input::open(&request.path)?;
    tracker.read = input.read.clone();
    start(conn, engine, request.options).await?;
    if let Err(err) = run_script(conn, engine, request, &mut input, &mut tracker, Pass::Run).await {
        abort(conn, request.options).await;
        return Err(err);
    }
    let mut summary = tracker.summary;
    end(conn, request.options, &mut summary).await?;
    Ok(summary)
}

async fn run_script(
    conn: &dyn Connection,
    engine: Engine,
    request: &SqlImport,
    input: &mut Input,
    tracker: &mut Tracker<impl FnMut(ImportEvent) + Send>,
    pass: Pass,
) -> AppResult<()> {
    let storage =
        |err: std::io::Error| AppError::storage(format!("{}: {err}", request.path.display()));
    let mut raw: Vec<u8> = Vec::new();
    let mut text = String::new();
    let mut delimiter = ";".to_owned();
    // Line number of the first line in `text`.
    let mut line = 1u64;
    let mut first = true;
    'chunks: loop {
        let mut chunk = vec![0; CHUNK];
        let n = input.reader.read(&mut chunk).map_err(storage)?;
        let eof = n == 0;
        raw.extend_from_slice(&chunk[..n]);
        // Keep a character cut in half by the chunk for the next round.
        let valid = match std::str::from_utf8(&raw) {
            Ok(_) => raw.len(),
            Err(err) if !eof && err.error_len().is_none() => err.valid_up_to(),
            Err(err) => {
                let at = line + bytecount(&raw[..err.valid_up_to()]);
                return Err(AppError::invalid(format!(
                    "line {at}: the file isn't valid UTF-8 text. If it holds raw binary data \
                     (mysqldump without --hex-blob), import it with the mysql client instead."
                )));
            }
        };
        text.push_str(std::str::from_utf8(&raw[..valid]).expect("checked above"));
        raw.drain(..valid);
        if first {
            if let Some(rest) = text.strip_prefix('\u{feff}') {
                text = rest.to_owned();
            }
            first = false;
        }

        let statements = split_from(&text, engine, &delimiter);
        let ready = if eof {
            statements.len()
        } else {
            statements.len().saturating_sub(1)
        };
        let mut counted = 0;
        for (range, _) in &statements[..ready] {
            let at = line + count_lines(&text[counted..range.start]);
            line = at;
            counted = range.start;
            let Some(sql) = strip_client_commands(engine, &text[range.clone()], at)? else {
                continue;
            };
            if pass == Pass::Check {
                continue;
            }
            match run(conn, sql.to_owned()).await {
                Ok(()) => tracker.succeeded(1),
                Err(message) if ignorable(engine, sql, &message) => tracker.succeeded(1),
                Err(message) => {
                    let error = ImportError {
                        line: at,
                        what: preview(sql, 200),
                        message,
                    };
                    if tracker.failed(error) {
                        break 'chunks;
                    }
                }
            }
        }
        if eof {
            break;
        }
        // Carry the unfinished statement, with the delimiter it started under.
        // With no statement yet, `text` is kept whole: it may end in a cut comment.
        if let Some((range, statement_delimiter)) = statements.get(ready) {
            line += count_lines(&text[counted..range.start]);
            text.drain(..range.start);
            delimiter.clone_from(statement_delimiter);
        }
    }
    Ok(())
}

/// The statement without the psql commands this importer can skip, or
/// `None` if nothing is left. pg_dump 17.6+ wraps scripts in
/// `\restrict`/`\unrestrict`, which only guard psql's own commands; any other
/// psql command means the script needs psql.
fn strip_client_commands(engine: Engine, sql: &str, line: u64) -> AppResult<Option<&str>> {
    if engine != Engine::Postgres {
        return Ok(Some(sql));
    }
    let mut rest = sql;
    while rest.starts_with('\\') {
        let (command, after) = rest.split_once('\n').unwrap_or((rest, ""));
        let name = command.split_whitespace().next().unwrap_or(command);
        if !matches!(name, "\\restrict" | "\\unrestrict") {
            return Err(needs_psql(line, name));
        }
        rest = after.trim_start();
    }
    if rest.is_empty() {
        return Ok(None);
    }
    let upper: String = rest
        .chars()
        .take(4096)
        .collect::<String>()
        .to_ascii_uppercase();
    if upper.starts_with("COPY ") && upper.contains("FROM STDIN") {
        return Err(needs_psql(line, "COPY … FROM stdin"));
    }
    Ok(Some(rest))
}

/// A failure that changes nothing: pg_dump 17+ writes
/// `SET transaction_timeout = 0`, which servers before 17 don't know, and
/// psql users routinely skip.
fn ignorable(engine: Engine, sql: &str, message: &str) -> bool {
    engine == Engine::Postgres
        && sql
            .trim()
            .eq_ignore_ascii_case("SET transaction_timeout = 0")
        && message.contains("transaction_timeout")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// Only look for statements the importer can't run.
    Check,
    Run,
}

fn needs_psql(line: u64, what: &str) -> AppError {
    AppError::invalid(format!(
        "line {line}: `{what}` is a psql feature the built-in importer can't run. \
         Install the PostgreSQL client tools (psql) to import this file."
    ))
}

fn count_lines(text: &str) -> u64 {
    bytecount(text.as_bytes())
}

fn bytecount(bytes: &[u8]) -> u64 {
    bytes.iter().filter(|&&b| b == b'\n').count() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_restrict_and_rejects_other_psql_commands() {
        assert_eq!(
            strip_client_commands(Engine::Postgres, "\\restrict abc\nSET x = 1", 1).unwrap(),
            Some("SET x = 1")
        );
        assert_eq!(
            strip_client_commands(Engine::Postgres, "\\unrestrict abc", 9).unwrap(),
            None
        );
        let err =
            strip_client_commands(Engine::Postgres, "\\connect other\nSET x = 1", 3).unwrap_err();
        assert!(err.to_string().starts_with("line 3: `\\connect`"), "{err}");
        assert!(strip_client_commands(Engine::Postgres, "COPY t (a) FROM stdin", 1).is_err());
        assert_eq!(
            strip_client_commands(Engine::Mysql, "\\restrict", 1).unwrap(),
            Some("\\restrict")
        );
    }
}
