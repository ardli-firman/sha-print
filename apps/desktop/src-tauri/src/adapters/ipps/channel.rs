//! Durable Network Channel storage for server authorization and the local client proxy.

use async_trait::async_trait;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tokio::sync::Mutex;

use crate::application::ChannelStore;
use crate::domain::AppError;

const VERIFIER_FILE: &str = "network-channel-verifier.json";
/// Default credential file name inside the Linux machine credential directory (`credentials/network-channel`).
pub const CLIENT_CREDENTIAL_FILE: &str = "network-channel";

/// Holds a salted one-way server verifier and, on Windows, a DPAPI-protected client credential,
/// or on Linux, a protected machine credential file.
pub struct NetworkChannel {
    path: Option<PathBuf>,
    credential_file: Option<PathBuf>,
    verifier: RwLock<Option<ChannelVerifier>>,
    client_secret: RwLock<Option<String>>,
    configure_lock: Mutex<()>,
}

impl NetworkChannel {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            credential_file: None,
            verifier: RwLock::new(None),
            client_secret: RwLock::new(None),
            configure_lock: Mutex::new(()),
        }
    }

    /// Loads the verifier from app data. Legacy unsalted records are not trusted.
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, AppError> {
        Self::open_with_credential_file(directory, None)
    }

    /// Loads the verifier from `directory` and, when `credential_file` (or systemd
    /// `CREDENTIALS_DIRECTORY`) is provided, loads the Client credential from protected machine
    /// credential storage.
    pub fn open_with_credential_file(
        directory: impl Into<PathBuf>,
        credential_file: Option<PathBuf>,
    ) -> Result<Self, AppError> {
        let path = directory.into().join(VERIFIER_FILE);
        let (verifier, mut client_secret) = if path.exists() {
            let bytes = fs::read(&path)
                .map_err(|_| AppError::internal("cannot read the Network Channel verifier"))?;
            let record: VerifierRecord = serde_json::from_slice(&bytes)
                .map_err(|_| AppError::internal("the Network Channel verifier is invalid"))?;
            let verifier = match record.salt {
                Some(salt) => Some(ChannelVerifier {
                    salt: decode_hex(salt)?,
                    digest: decode_hex(record.sha256)?,
                }),
                None => None,
            };
            let secret = record
                .client_secret_protected
                .as_deref()
                .map(decode_secret)
                .transpose()?;
            (verifier, secret)
        } else {
            (None, None)
        };

        if client_secret.is_none() {
            client_secret = load_machine_credential(credential_file.as_deref())?;
        }

        Ok(Self {
            path: Some(path),
            credential_file,
            verifier: RwLock::new(verifier),
            client_secret: RwLock::new(client_secret),
            configure_lock: Mutex::new(()),
        })
    }

    /// Replaces the server verifier and persists a protected copy for client forwarding.
    pub async fn configure(&self, channel: &str) -> Result<bool, AppError> {
        if channel.is_empty() {
            return Err(AppError::invalid_input("Network Channel cannot be empty"));
        }
        let _guard = self.configure_lock.lock().await;
        let mut salt = [0u8; 16];
        SystemRandom::new()
            .fill(&mut salt)
            .map_err(|_| AppError::internal("cannot generate a Network Channel salt"))?;
        let verifier = ChannelVerifier {
            salt,
            digest: channel_digest(&salt, channel),
        };
        let client_secret_protected = protect_secret(channel)?;
        let credential_file = self.credential_file.clone();
        let credential_copy = channel.to_owned();
        if let Some(path) = self.path.clone() {
            tokio::task::spawn_blocking(move || {
                persist(&path, verifier, client_secret_protected)?;
                if let Some(cred_path) = credential_file {
                    match persist_machine_credential(&cred_path, &credential_copy) {
                        Ok(()) => {}
                        Err(_)
                            if read_credential_file(&cred_path).ok().as_deref()
                                == Some(credential_copy.as_str()) =>
                        {
                            // When privileged `shaprintctl` already wrote the matching root-owned
                            // machine credential (`0640 root:shaprint`), keep it.
                        }
                        Err(error) => return Err(error),
                    }
                }
                Ok::<(), AppError>(())
            })
            .await
            .map_err(|_| AppError::internal("Network Channel storage worker stopped"))??;
        }
        let mut current = self
            .verifier
            .write()
            .map_err(|_| AppError::internal("Network Channel state is unavailable"))?;
        *current = Some(verifier);
        let mut secret = self
            .client_secret
            .write()
            .map_err(|_| AppError::internal("Network Channel state is unavailable"))?;
        *secret = Some(channel.to_owned());
        Ok(true)
    }

    /// Synchronously configures in-memory channel verifier for test fixtures.
    pub fn configure_sync(&self, channel: &str) -> Result<bool, AppError> {
        if channel.is_empty() {
            return Err(AppError::invalid_input("Network Channel cannot be empty"));
        }
        let mut salt = [0u8; 16];
        SystemRandom::new()
            .fill(&mut salt)
            .map_err(|_| AppError::internal("cannot generate a Network Channel salt"))?;
        let verifier = ChannelVerifier {
            salt,
            digest: channel_digest(&salt, channel),
        };
        let mut current = self
            .verifier
            .write()
            .map_err(|_| AppError::internal("Network Channel state is unavailable"))?;
        *current = Some(verifier);
        let mut secret = self
            .client_secret
            .write()
            .map_err(|_| AppError::internal("Network Channel state is unavailable"))?;
        *secret = Some(channel.to_owned());
        Ok(true)
    }

    /// Whether a verifier is configured; does not reveal any credential material.
    pub fn is_configured(&self) -> bool {
        self.verifier
            .read()
            .map(|value| value.is_some())
            .unwrap_or(false)
    }

    /// Checks a supplied channel without retaining it and compares the digest without early exit.
    pub fn authorizes(&self, candidate: &str) -> bool {
        let Ok(current) = self.verifier.read() else {
            return false;
        };
        let Some(expected) = current.as_ref() else {
            return false;
        };
        let actual = channel_digest(&expected.salt, candidate);
        expected
            .digest
            .iter()
            .zip(actual.iter())
            .fold(0u8, |difference, (left, right)| difference | (left ^ right))
            == 0
    }

    /// Credential held by the local client proxy. This value is never exposed through IPC or
    /// status; durable Windows copies are protected by DPAPI for the current user.
    pub fn client_credential(&self) -> Option<String> {
        self.client_secret
            .read()
            .ok()
            .and_then(|value| value.clone())
    }
}

#[derive(Serialize, Deserialize)]
struct VerifierRecord {
    #[serde(default)]
    salt: Option<String>,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_secret_protected: Option<String>,
}

#[derive(Clone, Copy)]
struct ChannelVerifier {
    salt: [u8; 16],
    digest: [u8; 32],
}

fn channel_digest(salt: &[u8; 16], channel: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(channel.as_bytes());
    hasher.finalize().into()
}

fn decode_hex<const N: usize>(encoded: String) -> Result<[u8; N], AppError> {
    hex::decode(encoded)
        .map_err(|_| AppError::internal("the Network Channel verifier is invalid"))?
        .try_into()
        .map_err(|_| AppError::internal("the Network Channel verifier is invalid"))
}

fn persist(
    path: &Path,
    verifier: ChannelVerifier,
    client_secret_protected: Option<String>,
) -> Result<(), AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal("cannot locate Network Channel storage"))?;
    fs::create_dir_all(parent)
        .map_err(|_| AppError::internal("cannot create Network Channel storage"))?;
    let temporary = path.with_extension("json.tmp");
    let record = VerifierRecord {
        salt: Some(hex::encode(verifier.salt)),
        sha256: hex::encode(verifier.digest),
        client_secret_protected,
    };
    let bytes = serde_json::to_vec(&record)
        .map_err(|_| AppError::internal("cannot encode the Network Channel verifier"))?;
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| AppError::internal("cannot create Network Channel verifier storage"))?;
    file.write_all(&bytes)
        .map_err(|_| AppError::internal("cannot persist the Network Channel verifier"))?;
    file.sync_all()
        .map_err(|_| AppError::internal("cannot persist the Network Channel verifier"))?;
    drop(file);
    fs::rename(&temporary, path)
        .map_err(|_| AppError::internal("cannot persist the Network Channel verifier"))
}

fn load_machine_credential(credential_file: Option<&Path>) -> Result<Option<String>, AppError> {
    if let Ok(cred_dir) = std::env::var("CREDENTIALS_DIRECTORY") {
        let systemd_path = Path::new(&cred_dir).join(CLIENT_CREDENTIAL_FILE);
        if systemd_path.exists() {
            return read_credential_file(&systemd_path).map(Some);
        }
    }
    if let Some(path) = credential_file {
        if path.exists() {
            return read_credential_file(path).map(Some);
        }
    }
    Ok(None)
}

fn read_credential_file(path: &Path) -> Result<String, AppError> {
    let bytes = fs::read(path)
        .map_err(|_| AppError::internal("cannot read the protected Network Channel credential"))?;
    let raw = String::from_utf8(bytes)
        .map_err(|_| AppError::internal("the protected Network Channel credential is invalid"))?;
    let trimmed = raw.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return Err(AppError::internal(
            "the protected Network Channel credential is empty",
        ));
    }
    Ok(trimmed.to_owned())
}

/// Persists the Client Network Channel credential into root/daemon-protected machine credential
/// storage (`0700` or `0750` parent directory, `0600` or `0640` file).
pub fn persist_machine_credential(path: &Path, channel: &str) -> Result<(), AppError> {
    if channel.is_empty() {
        return Err(AppError::invalid_input("Network Channel cannot be empty"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal("cannot locate machine credential directory"))?;
    let existed = parent.exists();
    fs::create_dir_all(parent)
        .map_err(|_| AppError::internal("cannot create machine credential directory"))?;
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|_| {
            AppError::internal("cannot set permissions on machine credential directory")
        })?;
    }
    let temporary = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| AppError::internal("cannot create machine credential file"))?;
    file.write_all(channel.as_bytes())
        .map_err(|_| AppError::internal("cannot write machine credential file"))?;
    file.sync_all()
        .map_err(|_| AppError::internal("cannot sync machine credential file"))?;
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let group_applied = Command::new("chgrp")
            .args(["shaprint", &parent.to_string_lossy()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
            && Command::new("chgrp")
                .args(["shaprint", &temporary.to_string_lossy()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
        let file_mode = if group_applied {
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o750));
            0o640
        } else {
            0o600
        };
        fs::set_permissions(&temporary, fs::Permissions::from_mode(file_mode))
            .map_err(|_| AppError::internal("cannot set permissions on machine credential file"))?;
    }
    fs::rename(&temporary, path)
        .map_err(|_| AppError::internal("cannot persist machine credential file"))
}

/// Persists the Client Network Channel credential as root into `/etc/credstore/shaprint` and
/// sets group `shaprint` (`0750` directory, `0640` file) when the `shaprint` account exists so
/// the unprivileged daemon can read it across restarts while ordinary local users cannot.
pub fn persist_root_machine_credential(path: &Path, channel: &str) -> Result<(), AppError> {
    persist_machine_credential(path, channel)
}

fn protect_secret(value: &str) -> Result<Option<String>, AppError> {
    #[cfg(windows)]
    {
        protect_data(value.as_bytes()).map(|bytes| Some(hex::encode(bytes)))
    }
    #[cfg(not(windows))]
    {
        let _ = value;
        Ok(None)
    }
}

fn decode_secret(encoded: &str) -> Result<String, AppError> {
    #[cfg(windows)]
    {
        let protected = hex::decode(encoded)
            .map_err(|_| AppError::internal("the protected Network Channel is invalid"))?;
        let bytes = unprotect_data(&protected)?;
        String::from_utf8(bytes)
            .map_err(|_| AppError::internal("the protected Network Channel is invalid"))
    }
    #[cfg(not(windows))]
    {
        let _ = encoded;
        Err(AppError::unsupported(
            "protected Network Channel storage is available on Windows only",
        ))
    }
}

#[cfg(windows)]
fn protect_data(value: &[u8]) -> Result<Vec<u8>, AppError> {
    crate::adapters::win_crypto::protect(value, None).map_err(|_| {
        AppError::internal("Windows could not protect the Network Channel for the local proxy.")
    })
}

#[cfg(windows)]
fn unprotect_data(value: &[u8]) -> Result<Vec<u8>, AppError> {
    crate::adapters::win_crypto::unprotect(value, None).map_err(|_| {
        AppError::internal(
            "Windows could not open the protected Network Channel for the local proxy.",
        )
    })
}

impl std::fmt::Debug for NetworkChannel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NetworkChannel")
            .field("configured", &self.is_configured())
            .finish()
    }
}

/// The import from the previous ShaPrint app stores the channel exactly as the user would (#41).
#[async_trait]
impl ChannelStore for NetworkChannel {
    fn is_configured(&self) -> bool {
        NetworkChannel::is_configured(self)
    }

    async fn store(&self, channel: &str) -> Result<(), AppError> {
        self.configure(channel).await.map(|_| ())
    }
}

impl crate::application::ChannelState for NetworkChannel {
    fn is_configured(&self) -> bool {
        NetworkChannel::is_configured(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::temporary_directory;

    #[cfg(unix)]
    #[tokio::test]
    async fn linux_machine_channel_persists_only_salted_server_verifier_and_stores_client_credential_in_protected_credential_store(
    ) {
        use std::os::unix::fs::PermissionsExt;

        let root = temporary_directory("linux-network-channel-persistence");
        let state_dir = root.join("var/lib/shaprint");
        let cred_dir = root.join("etc/credstore/shaprint");
        let cred_file = cred_dir.join("network-channel");
        let generated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("unix epoch")
            .as_nanos()
            .to_string();
        let secret = generated.as_str();

        let channel =
            NetworkChannel::open_with_credential_file(&state_dir, Some(cred_file.clone()))
                .expect("opens unconfigured channel");
        assert!(!channel.is_configured());
        assert_eq!(channel.client_credential(), None);

        channel
            .configure(secret)
            .await
            .expect("configures Network Channel");
        assert!(channel.is_configured());
        assert!(channel.authorizes(secret));
        assert!(!channel.authorizes("wrong-channel"));
        assert_eq!(channel.client_credential().as_deref(), Some(secret));

        // 1. Server verifier file in `/var/lib/shaprint` contains ONLY a fresh salt and SHA-256
        //    digest; it never contains plaintext or `client_secret_protected`.
        let verifier_path = state_dir.join(VERIFIER_FILE);
        let verifier_raw =
            std::fs::read_to_string(&verifier_path).expect("reads persisted verifier");
        assert!(
            !verifier_raw.contains(secret),
            "verifier file must never contain the Network Channel secret"
        );
        let parsed_verifier: serde_json::Value =
            serde_json::from_str(&verifier_raw).expect("valid verifier JSON");
        let first_salt = parsed_verifier["salt"]
            .as_str()
            .expect("has salt")
            .to_owned();
        assert_eq!(first_salt.len(), 32);
        assert!(parsed_verifier.get("client_secret_protected").is_none());
        let verifier_mode = std::fs::metadata(&verifier_path)
            .expect("verifier metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(verifier_mode, 0o600);

        // 2. Client credential is stored in the protected machine credential directory with
        //    restricted permissions (`0700` dir, `0600` file).
        assert!(cred_file.exists(), "client credential file exists");
        let cred_dir_mode = std::fs::metadata(&cred_dir)
            .expect("cred dir metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(cred_dir_mode, 0o700);
        let cred_file_mode = std::fs::metadata(&cred_file)
            .expect("cred file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(cred_file_mode, 0o600);

        // 3. Reconfiguring the same secret generates a freshly salted verifier.
        channel
            .configure(secret)
            .await
            .expect("reconfigures Network Channel");
        let second_raw =
            std::fs::read_to_string(&verifier_path).expect("reads reconfigured verifier");
        let second_verifier: serde_json::Value =
            serde_json::from_str(&second_raw).expect("valid JSON");
        let second_salt = second_verifier["salt"].as_str().expect("has salt");
        assert_ne!(
            first_salt, second_salt,
            "reconfiguring must generate a fresh random salt"
        );

        // 4. Reopening (simulating daemon restart) restores both Server verifier authorization
        //    and the Client proxy credential.
        drop(channel);
        let reopened =
            NetworkChannel::open_with_credential_file(&state_dir, Some(cred_file.clone()))
                .expect("reopens persisted Network Channel");
        assert!(reopened.is_configured());
        assert!(reopened.authorizes(secret));
        assert!(!reopened.authorizes("wrong-channel"));
        assert_eq!(reopened.client_credential().as_deref(), Some(secret));

        std::fs::remove_dir_all(&root).ok();
    }
}
