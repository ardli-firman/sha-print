//! Durable Network Channel verifier used only to authorize print jobs.

use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tokio::sync::Mutex;

use crate::domain::AppError;

const VERIFIER_FILE: &str = "network-channel-verifier.json";

/// Holds a salted one-way SHA-256 verifier, never the configured channel itself.
pub struct NetworkChannel {
    path: Option<PathBuf>,
    verifier: RwLock<Option<ChannelVerifier>>,
    configure_lock: Mutex<()>,
}

impl NetworkChannel {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            verifier: RwLock::new(None),
            configure_lock: Mutex::new(()),
        }
    }

    /// Loads the verifier from app data. Legacy unsalted records are not trusted.
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, AppError> {
        let path = directory.into().join(VERIFIER_FILE);
        let verifier = if path.exists() {
            let bytes = fs::read(&path)
                .map_err(|_| AppError::internal("cannot read the Network Channel verifier"))?;
            let record: VerifierRecord = serde_json::from_slice(&bytes)
                .map_err(|_| AppError::internal("the Network Channel verifier is invalid"))?;
            match record.salt {
                Some(salt) => Some(ChannelVerifier {
                    salt: decode_hex(salt)?,
                    digest: decode_hex(record.sha256)?,
                }),
                None => None,
            }
        } else {
            None
        };
        Ok(Self {
            path: Some(path),
            verifier: RwLock::new(verifier),
            configure_lock: Mutex::new(()),
        })
    }

    /// Replaces the Network Channel with its digest. The secret is never retained or serialized.
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
        if let Some(path) = self.path.clone() {
            tokio::task::spawn_blocking(move || persist(&path, verifier))
                .await
                .map_err(|_| AppError::internal("Network Channel storage worker stopped"))??;
        }
        let mut current = self
            .verifier
            .write()
            .map_err(|_| AppError::internal("Network Channel state is unavailable"))?;
        *current = Some(verifier);
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
}

#[derive(Serialize, Deserialize)]
struct VerifierRecord {
    #[serde(default)]
    salt: Option<String>,
    sha256: String,
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

fn persist(path: &Path, verifier: ChannelVerifier) -> Result<(), AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal("cannot locate Network Channel storage"))?;
    fs::create_dir_all(parent)
        .map_err(|_| AppError::internal("cannot create Network Channel storage"))?;
    let temporary = path.with_extension("json.tmp");
    let record = VerifierRecord {
        salt: Some(hex::encode(verifier.salt)),
        sha256: hex::encode(verifier.digest),
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

impl std::fmt::Debug for NetworkChannel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NetworkChannel")
            .field("configured", &self.is_configured())
            .finish()
    }
}
