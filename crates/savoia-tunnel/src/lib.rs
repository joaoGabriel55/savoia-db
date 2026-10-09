//! SSH local port forwarding: listens on `127.0.0.1:<random>` and forwards each
//! accepted socket to `target` through a `direct-tcpip` channel. Must run
//! inside a Tokio runtime.
//!
//! Host keys are checked against known_hosts. An unknown host fails with
//! [`AppError::UnknownHostKey`] unless the caller passes
//! [`HostKeyPolicy::TrustUnknown`] (after asking the user), which also records
//! the key. A changed key always fails.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, AuthResult, Handle};
use russh::keys::{
    self, HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate, known_hosts,
};
use savoia_core::{AppError, AppResult, Secrets, SshAuth, SshConfig};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyPolicy {
    /// Fail on hosts missing from known_hosts.
    KnownOnly,
    /// Accept and record an unknown host key. Never accepts a changed key.
    TrustUnknown,
}

/// Which known_hosts file to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnownHosts {
    /// `~/.ssh/known_hosts`, shared with OpenSSH.
    User,
    File(PathBuf),
}

/// An open tunnel. Dropping it stops forwarding and closes the SSH session.
pub struct Tunnel {
    local_port: u16,
    session: Arc<Handle<HostKeyCheck>>,
    accept_task: JoinHandle<()>,
}

impl Tunnel {
    /// Connect the database driver to `127.0.0.1:local_port()`.
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    pub async fn close(self) {
        drop(
            self.session
                .disconnect(russh::Disconnect::ByApplication, "", "en")
                .await,
        );
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

pub async fn open(
    ssh: &SshConfig,
    secrets: &Secrets,
    target: (String, u16),
    policy: HostKeyPolicy,
    known_hosts: KnownHosts,
) -> AppResult<Tunnel> {
    let outcome = Arc::new(Mutex::new(None));
    let handler = HostKeyCheck {
        host: ssh.host.clone(),
        port: ssh.port,
        policy,
        known_hosts,
        outcome: outcome.clone(),
    };
    let config = Arc::new(client::Config {
        nodelay: true,
        ..Default::default()
    });

    let connect = client::connect(config, (ssh.host.as_str(), ssh.port), handler);
    let mut session = match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
        Err(_) => {
            return Err(AppError::ssh(format!(
                "timed out connecting to {}:{}",
                ssh.host, ssh.port
            )));
        }
        Ok(Ok(session)) => session,
        // A rejected host key surfaces as a generic error; report the real reason.
        Ok(Err(err)) => {
            return Err(outcome
                .lock()
                .expect("poisoned")
                .take()
                .unwrap_or_else(|| AppError::ssh(err)));
        }
    };

    authenticate(&mut session, ssh, secrets).await?;

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(AppError::ssh)?;
    let local_port = listener.local_addr().map_err(AppError::ssh)?.port();
    let session = Arc::new(session);
    let accept_task = tokio::spawn(forward(listener, session.clone(), target));
    Ok(Tunnel {
        local_port,
        session,
        accept_task,
    })
}

async fn authenticate(
    session: &mut Handle<HostKeyCheck>,
    ssh: &SshConfig,
    secrets: &Secrets,
) -> AppResult<()> {
    let user = ssh.user.clone();
    let result = match &ssh.auth {
        SshAuth::Password => {
            let password = secrets.ssh_password.clone().unwrap_or_default();
            session
                .authenticate_password(user, password)
                .await
                .map_err(AppError::ssh)?
        }
        SshAuth::PrivateKey { path } => {
            let key = keys::load_secret_key(path, secrets.ssh_key_passphrase.as_deref())
                .map_err(|e| AppError::ssh(format!("cannot load key {}: {e}", path.display())))?;
            let hash = session
                .best_supported_rsa_hash()
                .await
                .map_err(AppError::ssh)?
                .flatten();
            session
                .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await
                .map_err(AppError::ssh)?
        }
        SshAuth::Agent => agent_auth(session, &user).await?,
    };
    match result {
        AuthResult::Success => Ok(()),
        AuthResult::Failure { .. } => Err(AppError::ssh(format!(
            "authentication failed for {}@{}",
            ssh.user, ssh.host
        ))),
    }
}

#[cfg(unix)]
async fn agent_client() -> AppResult<keys::agent::client::AgentClient<tokio::net::UnixStream>> {
    keys::agent::client::AgentClient::connect_env()
        .await
        .map_err(|e| AppError::ssh(format!("SSH agent unavailable: {e}")))
}

#[cfg(windows)]
async fn agent_client()
-> AppResult<keys::agent::client::AgentClient<tokio::net::windows::named_pipe::NamedPipeClient>> {
    keys::agent::client::AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
        .await
        .map_err(|e| AppError::ssh(format!("SSH agent unavailable: {e}")))
}

/// Tries each agent identity until one is accepted.
async fn agent_auth(session: &mut Handle<HostKeyCheck>, user: &str) -> AppResult<AuthResult> {
    let mut agent = agent_client().await?;
    let identities = agent.request_identities().await.map_err(AppError::ssh)?;
    let hash = session
        .best_supported_rsa_hash()
        .await
        .map_err(AppError::ssh)?
        .flatten();
    let mut last = None;
    for identity in identities {
        let keys::agent::AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        let result = session
            .authenticate_publickey_with(user, key, hash, &mut agent)
            .await
            .map_err(AppError::ssh)?;
        if result.success() {
            return Ok(result);
        }
        last = Some(result);
    }
    last.ok_or_else(|| AppError::ssh("the SSH agent has no keys"))
}

async fn forward(listener: TcpListener, session: Arc<Handle<HostKeyCheck>>, target: (String, u16)) {
    while let Ok((mut socket, origin)) = listener.accept().await {
        let session = session.clone();
        let (host, port) = target.clone();
        tokio::spawn(async move {
            let channel = session
                .channel_open_direct_tcpip(
                    host,
                    port.into(),
                    origin.ip().to_string(),
                    origin.port().into(),
                )
                .await;
            if let Ok(channel) = channel {
                let mut stream = channel.into_stream();
                drop(tokio::io::copy_bidirectional(&mut socket, &mut stream).await);
            }
        });
    }
}

/// SHA256 fingerprint as OpenSSH prints it.
pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

struct HostKeyCheck {
    host: String,
    port: u16,
    policy: HostKeyPolicy,
    known_hosts: KnownHosts,
    /// Why the key was rejected, for the error returned by [`open`].
    outcome: Arc<Mutex<Option<AppError>>>,
}

impl HostKeyCheck {
    fn verify(&self, key: &PublicKey) -> AppResult<()> {
        let known = match &self.known_hosts {
            KnownHosts::User => known_hosts::check_known_hosts(&self.host, self.port, key),
            KnownHosts::File(path) if !path.exists() => Ok(false),
            KnownHosts::File(path) => {
                known_hosts::check_known_hosts_path(&self.host, self.port, key, path)
            }
        };
        match known {
            Ok(true) => Ok(()),
            Ok(false) if self.policy == HostKeyPolicy::TrustUnknown => {
                let learned = match &self.known_hosts {
                    KnownHosts::User => known_hosts::learn_known_hosts(&self.host, self.port, key),
                    KnownHosts::File(path) => {
                        known_hosts::learn_known_hosts_path(&self.host, self.port, key, path)
                    }
                };
                learned.map_err(|e| AppError::ssh(format!("could not record host key: {e}")))
            }
            Ok(false) => Err(AppError::UnknownHostKey {
                host: format!("{}:{}", self.host, self.port),
                fingerprint: fingerprint(key),
            }),
            Err(keys::Error::KeyChanged { line }) => Err(AppError::ssh(format!(
                "HOST KEY CHANGED for {} (known_hosts line {line}). This may be a man-in-the-middle \
                 attack; remove the old entry only if you know the key was rotated.",
                self.host
            ))),
            Err(e) => Err(AppError::ssh(format!("known_hosts: {e}"))),
        }
    }
}

impl client::Handler for HostKeyCheck {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // Certificate host keys are checked by their key, like a plain key;
        // trusting a host CA (@cert-authority) is not supported.
        let key = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => PublicKey::from(cert.public_key().clone()),
        };
        match self.verify(&key) {
            Ok(()) => Ok(true),
            Err(err) => {
                *self.outcome.lock().expect("poisoned") = Some(err);
                Ok(false)
            }
        }
    }
}
