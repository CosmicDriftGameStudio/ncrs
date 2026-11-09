//! The login of a connection whose host key was accepted: the password, the
//! ssh-agent, then key files, each only when the server names the method.
//! Nothing here runs before the key exchange has judged the host key.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::client::{AuthResult, Handle};
use russh::keys::{
    decode_secret_key, Error as KeyError, HashAlg, PrivateKey, PrivateKeyWithHashAlg,
};
use russh::{MethodKind, MethodSet};
use tokio::task::spawn_blocking;

use super::{HostCheck, Step};
use crate::remote::credentials::PassphraseStore;
use crate::remote::ssh_config::{expand_home, HostSettings};
use crate::remote::{Secret, ServerId, DEFAULT_PORT};

/// What a connection is made with. The password and the passphrase are secrets
/// and print as `***`.
#[derive(Debug, Clone, Default)]
pub struct Login {
    pub password: Secret,
    /// The key file of this server; without one, those of `~/.ssh/config` and
    /// the default ones are tried.
    pub key_file: Option<PathBuf>,
    /// The passphrase typed for one key file, used for exactly that file;
    /// any other encrypted key is opened only from the keychain.
    pub passphrase: Option<(PathBuf, Secret)>,
    /// Keep the passphrase in the keychain once it has opened a key that the
    /// server accepted.
    pub remember_passphrase: bool,
}

/// What the login reads from the machine, so that tests can put in their own.
#[derive(Debug, Clone, Default)]
pub struct LoginEnv {
    pub home: Option<PathBuf>,
    /// The socket of the ssh-agent.
    pub agent_socket: Option<PathBuf>,
    /// The name an address without a user logs in as, when `~/.ssh/config`
    /// names none.
    pub os_user: Option<String>,
}

impl LoginEnv {
    /// This process: its home (the real one in the sandbox), its agent socket
    /// (none in the sandbox, which does not reach it) and its user name.
    pub fn from_process() -> Self {
        let variable = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        Self {
            home: crate::sandbox::sandbox_home().or_else(|| {
                variable("HOME")
                    .or_else(|| variable("USERPROFILE"))
                    .map(PathBuf::from)
            }),
            agent_socket: if crate::sandbox_active() {
                None
            } else {
                variable("SSH_AUTH_SOCK").map(PathBuf::from)
            },
            os_user: ["USER", "LOGNAME", "USERNAME"]
                .into_iter()
                .find_map(variable)
                .and_then(|name| name.into_string().ok()),
        }
    }
}

/// The server to dial and log in to: what `~/.ssh/config` says about the alias
/// the caller gave, as OpenSSH applies it. `None` when no user name is known.
pub(in crate::remote::session) fn effective_server(
    asked: &ServerId,
    settings: &HostSettings,
    os_user: Option<&str>,
) -> Option<ServerId> {
    let host = settings.host_name.as_deref().unwrap_or(&asked.host);
    let port = if asked.port == DEFAULT_PORT {
        settings.port.unwrap_or(asked.port)
    } else {
        asked.port
    };
    let user = if asked.user.is_empty() {
        settings.user.as_deref().or(os_user)?
    } else {
        &asked.user
    };
    Some(ServerId::new(user, host, port))
}

/// Everything the steps need.
pub(in crate::remote::session) struct Attempt<'a> {
    pub user: &'a str,
    pub login: &'a Login,
    pub env: &'a LoginEnv,
    pub identity_files: &'a [PathBuf],
    pub passphrases: &'a Arc<dyn PassphraseStore>,
}

/// What the steps learned that decides the failure.
#[derive(Default)]
struct Findings {
    /// The first encrypted key that had no passphrase.
    needs_passphrase: Option<PathBuf>,
    wrong_passphrase: Option<PathBuf>,
    password_rejected: bool,
}

impl Findings {
    fn failure(self, methods: &MethodSet) -> Step {
        if let Some(key_file) = self.wrong_passphrase {
            Step::WrongPassphrase(key_file)
        } else if self.password_rejected {
            Step::AuthFailed
        } else if let Some(key_file) = self.needs_passphrase {
            Step::PassphraseNeeded(key_file)
        } else if !methods.is_empty() && !methods.contains(&MethodKind::Password) {
            Step::PasswordNotOffered
        } else {
            Step::AuthFailed
        }
    }
}

/// Tries the password, the agent and the key files, in that order, for the
/// methods the server named (a server that named none is tried with all).
pub(in crate::remote::session) async fn sign_in(
    handle: &mut Handle<HostCheck>,
    methods: &MethodSet,
    attempt: &Attempt<'_>,
) -> Result<(), Step> {
    let allowed = |kind: MethodKind| methods.is_empty() || methods.contains(&kind);
    let mut findings = Findings::default();
    // The password goes first: every refused key counts against the server's
    // limit of attempts, and a password that exists is the likelier way in.
    if !attempt.login.password.is_empty() && allowed(MethodKind::Password) {
        let reply = handle
            .authenticate_password(
                attempt.user.to_owned(),
                attempt.login.password.expose().to_owned(),
            )
            .await?;
        if reply.success() {
            return Ok(());
        }
        findings.password_rejected = true;
    }
    if allowed(MethodKind::PublicKey) {
        if with_agent(handle, attempt).await {
            return Ok(());
        }
        if with_key_files(handle, attempt, &mut findings).await? {
            return Ok(());
        }
    }
    Err(findings.failure(methods))
}

/// Every identity of the agent once. No agent, a dead one or one that declines
/// to sign is no failure: there are just no keys from it.
#[cfg(unix)]
async fn with_agent(handle: &mut Handle<HostCheck>, attempt: &Attempt<'_>) -> bool {
    use russh::keys::agent::client::AgentClient;
    use russh::keys::agent::AgentIdentity;

    let Some(socket) = attempt.env.agent_socket.as_deref() else {
        return false;
    };
    let Ok(mut agent) = AgentClient::connect_uds(socket).await else {
        return false;
    };
    let Ok(identities) = agent.request_identities().await else {
        return false;
    };
    for identity in identities {
        let AgentIdentity::PublicKey { key, .. } = identity else {
            continue;
        };
        let hash = if key.algorithm().is_rsa() {
            match handle.best_supported_rsa_hash().await {
                Ok(Some(best)) => best,
                Ok(None) => Some(HashAlg::Sha256),
                Err(_unsupported) => None,
            }
        } else {
            None
        };
        let reply = handle
            .authenticate_publickey_with(attempt.user.to_owned(), key, hash, &mut agent)
            .await;
        if matches!(reply, Ok(AuthResult::Success)) {
            return true;
        }
    }
    false
}

#[cfg(not(unix))]
async fn with_agent(_handle: &mut Handle<HostCheck>, _attempt: &Attempt<'_>) -> bool {
    false
}

/// What reading one key file came to.
enum KeyRead {
    Key(Box<PrivateKey>),
    Encrypted,
    WrongPassphrase,
    /// Missing, unreadable, not a key, or one that cannot be used here.
    Unusable,
}

/// Reads the key at `path`. Blocking: decrypting a key takes real work.
///
/// An RSA key is never used: the build has no RSA support (see the dependency
/// note in the root manifest), so russh could not sign with it, and asking for
/// its passphrase would only lead the user into a dead end.
fn read_key(path: &Path, passphrase: Option<&Secret>) -> KeyRead {
    let Ok(text) = std::fs::read_to_string(path) else {
        return KeyRead::Unusable;
    };
    let unusable_rsa = |key: &PrivateKey| key.algorithm().is_rsa();
    match decode_secret_key(&text, None) {
        Ok(key) if unusable_rsa(&key) => KeyRead::Unusable,
        Ok(key) => KeyRead::Key(Box::new(key)),
        Err(KeyError::KeyIsEncrypted) => {
            if PrivateKey::from_openssh(&text).is_ok_and(|key| unusable_rsa(&key)) {
                return KeyRead::Unusable;
            }
            let Some(secret) = passphrase else {
                return KeyRead::Encrypted;
            };
            match decode_secret_key(&text, Some(secret.expose())) {
                Ok(key) if unusable_rsa(&key) => KeyRead::Unusable,
                Ok(key) => KeyRead::Key(Box::new(key)),
                Err(_wrong) => KeyRead::WrongPassphrase,
            }
        }
        Err(_other) => KeyRead::Unusable,
    }
}

/// Where the passphrase that opened a key came from.
enum Source {
    Request,
    Keychain,
}

/// The key files in the order they are offered, each once: the one of the
/// server, else those of the config, then the default ones.
fn key_candidates(attempt: &Attempt<'_>) -> Vec<PathBuf> {
    let home = attempt.env.home.as_deref();
    let mut candidates: Vec<PathBuf> = match &attempt.login.key_file {
        Some(file) => expand_home(file, home).into_iter().collect(),
        None => attempt.identity_files.to_vec(),
    };
    if let Some(home_folder) = home {
        for name in ["id_ed25519", "id_ecdsa", "id_rsa"] {
            candidates.push(home_folder.join(".ssh").join(name));
        }
    }
    let mut seen: Vec<PathBuf> = Vec::new();
    candidates.retain(|file| {
        let new = !seen.contains(file);
        if new {
            seen.push(file.clone());
        }
        new
    });
    candidates
}

async fn read_key_off_thread(path: &Path, passphrase: Option<&Secret>) -> KeyRead {
    let (file, secret) = (path.to_path_buf(), passphrase.cloned());
    spawn_blocking(move || read_key(&file, secret.as_ref()))
        .await
        .unwrap_or(KeyRead::Unusable)
}

/// The passphrase for an encrypted key: the one of the request when it was
/// typed for this very file, else the one in the keychain.
async fn passphrase_for(
    path: &Path,
    request: &mut Option<(PathBuf, Secret)>,
    store: &Arc<dyn PassphraseStore>,
) -> Option<(Secret, Source)> {
    if request.as_ref().is_some_and(|(file, _)| file == path) {
        if let Some((_file, given)) = request.take() {
            return Some((given, Source::Request));
        }
    }
    let (file, keychain) = (path.to_path_buf(), Arc::clone(store));
    let kept = spawn_blocking(move || keychain.get(&file))
        .await
        .ok()?
        .ok()??;
    Some((kept, Source::Keychain))
}

async fn forget_passphrase(path: &Path, store: &Arc<dyn PassphraseStore>) {
    let (file, keychain) = (path.to_path_buf(), Arc::clone(store));
    // A keychain that cannot be written leaves a stale entry, which the next
    // attempt finds wrong again; the login itself does not depend on it.
    let _left = spawn_blocking(move || keychain.forget(&file)).await;
}

async fn remember_passphrase(path: &Path, passphrase: &Secret, store: &Arc<dyn PassphraseStore>) {
    let (file, secret, keychain) = (path.to_path_buf(), passphrase.clone(), Arc::clone(store));
    // Failing to remember is not failing to log in.
    let _left = spawn_blocking(move || keychain.set(&file, &secret)).await;
}

/// The key files, one after the other. `true` when the server took one.
async fn with_key_files(
    handle: &mut Handle<HostCheck>,
    attempt: &Attempt<'_>,
    findings: &mut Findings,
) -> Result<bool, Step> {
    let mut request_passphrase = attempt.login.passphrase.clone();
    for file in key_candidates(attempt) {
        let mut remember: Option<Secret> = None;
        let key = match read_key_off_thread(&file, None).await {
            KeyRead::Key(key) => key,
            KeyRead::Unusable | KeyRead::WrongPassphrase => continue,
            KeyRead::Encrypted => {
                let Some((secret, source)) =
                    passphrase_for(&file, &mut request_passphrase, attempt.passphrases).await
                else {
                    findings.needs_passphrase.get_or_insert(file);
                    continue;
                };
                match read_key_off_thread(&file, Some(&secret)).await {
                    KeyRead::Key(key) => {
                        if matches!(source, Source::Request) && attempt.login.remember_passphrase {
                            remember = Some(secret);
                        }
                        key
                    }
                    KeyRead::WrongPassphrase => {
                        match source {
                            Source::Request => {
                                findings.wrong_passphrase.get_or_insert(file);
                            }
                            Source::Keychain => {
                                forget_passphrase(&file, attempt.passphrases).await;
                                findings.needs_passphrase.get_or_insert(file);
                            }
                        }
                        continue;
                    }
                    KeyRead::Unusable | KeyRead::Encrypted => continue,
                }
            }
        };
        let reply = handle
            .authenticate_publickey(
                attempt.user.to_owned(),
                PrivateKeyWithHashAlg::new(Arc::new(*key), None),
            )
            .await?;
        if reply.success() {
            if let Some(secret) = remember {
                remember_passphrase(&file, &secret, attempt.passphrases).await;
            }
            return Ok(true);
        }
    }
    Ok(false)
}
