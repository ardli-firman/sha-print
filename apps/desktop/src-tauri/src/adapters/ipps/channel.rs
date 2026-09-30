//! Durable Network Channel verifier used only to authorize print jobs.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tokio::sync::Mutex;

use crate::domain::AppError;

const VERIFIER_FILE: &str = "network-channel-verifier.json";

/// Holds a one-way SHA-256 verifier, never the configured channel itself.
pub struct NetworkChannel {
    path: Option<PathBuf>,
    digest: RwLock<Option<[u8; 32]>>,
    configure_lock: Mutex<()>,
}

impl NetworkChannel {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            digest: RwLock::new(None),
            configure_lock: Mutex::new(()),
        }
    }

    /// Loads the verifier from app data without ever loading or persisting plaintext credentials.
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self, AppError> {
        let path = directory.into().join(VERIFIER_FILE);
        let digest = if path.exists() {
            let bytes = fs::read(&path)
                .map_err(|_| AppError::internal("cannot read the Network Channel verifier"))?;
            let record: VerifierRecord = serde_json::from_slice(&bytes)
                .map_err(|_| AppError::internal("the Network Channel verifier is invalid"))?;
            let decoded = hex::decode(record.sha256)
                .map_err(|_| AppError::internal("the Network Channel verifier is invalid"))?;
            let digest: [u8; 32] = decoded
                .try_into()
                .map_err(|_| AppError::internal("the Network Channel verifier is invalid"))?;
            Some(digest)
        } else {
            None
        };
        Ok(Self {
            path: Some(path),
            digest: RwLock::new(digest),
            configure_lock: Mutex::new(()),
        })
    }

    /// Replaces the Network Channel with its digest. The secret is never retained or serialized.
    pub async fn configure(&self, channel: &str) -> Result<bool, AppError> {
        if channel.is_empty() {
            return Err(AppError::invalid_input("Network Channel cannot be empty"));
        }
        let _guard = self.configure_lock.lock().await;
        let digest: [u8; 32] = Sha256::digest(channel.as_bytes()).into();
        if let Some(path) = self.path.clone() {
            tokio::task::spawn_blocking(move || persist(&path, digest))
                .await
                .map_err(|_| AppError::internal("Network Channel storage worker stopped"))??;
        }
        let mut current = self
            .digest
            .write()
            .map_err(|_| AppError::internal("Network Channel state is unavailable"))?;
        *current = Some(digest);
        Ok(true)
    }

    /// Whether a verifier is configured; does not reveal any credential material.
    pub fn is_configured(&self) -> bool {
        self.digest
            .read()
            .map(|value| value.is_some())
            .unwrap_or(false)
    }

    /// Checks a supplied channel without retaining it and compares the digest without early exit.
    pub fn authorizes(&self, candidate: &str) -> bool {
        let Ok(current) = self.digest.read() else {
            return false;
        };
        let Some(expected) = current.as_ref() else {
            return false;
        };
        let actual = Sha256::digest(candidate.as_bytes());
        expected
            .iter()
            .zip(actual.iter())
            .fold(0u8, |difference, (left, right)| difference | (left ^ right))
            == 0
    }
}

#[derive(Serialize, Deserialize)]
struct VerifierRecord {
    sha256: String,
}

fn persist(path: &Path, digest: [u8; 32]) -> Result<(), AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::internal("cannot locate Network Channel storage"))?;
    fs::create_dir_all(parent)
        .map_err(|_| AppError::internal("cannot create Network Channel storage"))?;
    let temporary = path.with_extension("json.tmp");
    let record = VerifierRecord {
        sha256: hex::encode(digest),
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
