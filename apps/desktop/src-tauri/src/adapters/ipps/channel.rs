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

/// Holds a salted one-way server verifier and, on Windows, a DPAPI-protected client credential.
pub struct NetworkChannel {
    path: Option<PathBuf>,
    verifier: RwLock<Option<ChannelVerifier>>,
    client_secret: RwLock<Option<String>>,
    configure_lock: Mutex<()>,
}

impl NetworkChannel {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            verifier: RwLock::new(None),
            client_secret: RwLock::new(None),
            configure_lock: Mutex::new(()),
        }
    }

    /// Loads the verifier from app data. Legacy unsalted records are not trusted.
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, AppError> {
        let path = directory.into().join(VERIFIER_FILE);
        let (verifier, client_secret) = if path.exists() {
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
        Ok(Self {
            path: Some(path),
            verifier: RwLock::new(verifier),
            client_secret: RwLock::new(client_secret),
            configure_lock: Mutex::new(()),
        })
    }

    /// Replaces the server verifier and persists a user-protected copy for client forwarding.
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
        if let Some(path) = self.path.clone() {
            tokio::task::spawn_blocking(move || persist(&path, verifier, client_secret_protected))
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
