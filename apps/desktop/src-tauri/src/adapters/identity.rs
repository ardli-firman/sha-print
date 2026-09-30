//! The server's TLS identity: a self-signed certificate, its private key, and the fingerprint
//! clients approve (ADR 0001).
//!
//! The identity is created once and kept in the app data directory. Clients remember the
//! fingerprint they approved, so regenerating the identity would look like a changed server.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::domain::{AppError, CertificateFingerprint};

/// Certificate file name inside the identity directory.
const CERTIFICATE_FILE: &str = "server-identity.cert.der";
/// Private key file name inside the identity directory.
const KEY_FILE: &str = "server-identity.key.der";
/// Subject shown when a user inspects the certificate.
const COMMON_NAME: &str = "ShaPrint server";
/// A protected DACL granting full access only to the file owner (the creating user).
#[cfg(windows)]
const PRIVATE_FILE_DACL: &str = "D:P(A;;FA;;;OW)";

/// A certificate, its private key, and the fingerprint they produce.
pub struct ServerIdentity {
    certificate: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
    fingerprint: CertificateFingerprint,
}

impl ServerIdentity {
    /// Wraps certificate and key bytes that were stored earlier.
    pub fn from_der(certificate: Vec<u8>, key: Vec<u8>) -> Result<Self, AppError> {
        if certificate.is_empty() || key.is_empty() {
            return Err(AppError::internal(
                "the stored server certificate or key is empty",
            ));
        }
        let fingerprint = CertificateFingerprint::of_der(&certificate);
        Ok(Self {
            certificate: CertificateDer::from(certificate),
            key: PrivatePkcs8KeyDer::from(key),
            fingerprint,
        })
    }

    /// Generates a self-signed certificate for this machine.
    pub fn generate() -> Result<Self, AppError> {
        let mut params = CertificateParams::new(subject_alt_names()).map_err(|error| {
            AppError::internal(format!("cannot prepare the server certificate: {error}"))
        })?;
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, COMMON_NAME);
        distinguished_name.push(DnType::OrganizationName, "ShaPrint");
        params.distinguished_name = distinguished_name;

        let key = KeyPair::generate().map_err(|error| {
            AppError::internal(format!("cannot generate the server key: {error}"))
        })?;
        let certificate = params.self_signed(&key).map_err(|error| {
            AppError::internal(format!("cannot sign the server certificate: {error}"))
        })?;

        Self::from_der(certificate.der().to_vec(), key.serialize_der())
    }

    /// The value a client user approves before the server is trusted.
    pub fn fingerprint(&self) -> CertificateFingerprint {
        self.fingerprint
    }

    pub fn certificate(&self) -> &CertificateDer<'static> {
        &self.certificate
    }

    /// Private key material for the TLS listener. Never logged and never sent over IPC.
    pub fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(self.key.clone_key())
    }

    /// The certificate bytes, for storage.
    pub fn certificate_der(&self) -> &[u8] {
        self.certificate.as_ref()
    }

    /// The key bytes, for storage.
    pub fn key_der(&self) -> &[u8] {
        self.key.secret_pkcs8_der()
    }
}

impl std::fmt::Debug for ServerIdentity {
    /// Prints the fingerprint only: the key must never reach a log line.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServerIdentity")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

/// Keeps the server identity between runs.
pub trait IdentityStore: Send + Sync + 'static {
    /// Returns the stored identity, creating one the first time.
    fn load_or_create(&self) -> Result<ServerIdentity, AppError>;
}

/// Stores the identity as two DER files in the app data directory.
pub struct FileIdentityStore {
    directory: PathBuf,
}

impl FileIdentityStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    fn certificate_path(&self) -> PathBuf {
        self.directory.join(CERTIFICATE_FILE)
    }

    fn key_path(&self) -> PathBuf {
        self.directory.join(KEY_FILE)
    }
}

impl IdentityStore for FileIdentityStore {
    fn load_or_create(&self) -> Result<ServerIdentity, AppError> {
        let certificate_path = self.certificate_path();
        let key_path = self.key_path();

        match (certificate_path.exists(), key_path.exists()) {
            (true, true) => ServerIdentity::from_der(read(&certificate_path)?, read(&key_path)?),
            (false, false) => {
                let identity = ServerIdentity::generate()?;
                fs::create_dir_all(&self.directory).map_err(|error| {
                    AppError::internal(format!(
                        "cannot create the identity directory {}: {error}",
                        self.directory.display()
                    ))
                })?;
                write_private(&key_path, identity.key_der())?;
                if let Err(error) = write_private(&certificate_path, identity.certificate_der()) {
                    let _ = fs::remove_file(&key_path);
                    return Err(error);
                }
                log::info!(
                    "created the server certificate fingerprint={}",
                    identity.fingerprint()
                );
                Ok(identity)
            }
            // Half an identity would silently change the fingerprint a client approved, so the
            // shell reports it instead of replacing either file.
            _ => Err(AppError::internal(
                "the stored server certificate and key are out of step",
            )),
        }
    }
}

fn read(path: &Path) -> Result<Vec<u8>, AppError> {
    fs::read(path)
        .map_err(|error| AppError::internal(format!("cannot read {}: {error}", path.display())))
}

/// Replaces `path` with `contents`, readable only by the current user where the platform supports
/// it.
fn write_private(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    #[cfg(windows)]
    let result = write_private_windows(path, contents);
    #[cfg(not(windows))]
    let result = write_private_non_windows(path, contents);
    result.map_err(|error| AppError::internal(format!("cannot write {}: {error}", path.display())))
}

#[cfg(not(windows))]
fn write_private_non_windows(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let write = || -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()
    };

    write()?;
    fs::rename(&temporary, path)
}

#[cfg(windows)]
fn write_private_windows(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

    for _ in 0..16 {
        let mut temporary_name = path.as_os_str().to_os_string();
        temporary_name.push(format!(
            ".{}.{}.tmp",
            std::process::id(),
            NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
        ));
        let temporary = PathBuf::from(temporary_name);
        let mut file = match create_private_file(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };

        let write = file.write_all(contents).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = write {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        return Ok(());
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "cannot reserve a private temporary identity file",
    ))
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::{LocalFree, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL};

    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let descriptor: Vec<u16> = PRIVATE_FILE_DACL.encode_utf16().chain(Some(0)).collect();
    let mut security_descriptor = std::ptr::null_mut();
    // SAFETY: both strings are NUL-terminated; the API allocates `security_descriptor`.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut security_descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }

    let security_attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security_descriptor,
        bInheritHandle: 0,
    };
    // SAFETY: pointers remain valid for the call; CREATE_NEW prevents reuse of an
    // attacker-created temporary file with a different ACL.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE,
            0,
            &security_attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    let create_error = (handle == INVALID_HANDLE_VALUE).then(std::io::Error::last_os_error);
    // SAFETY: the descriptor was allocated by the SDDL conversion API.
    unsafe {
        LocalFree(security_descriptor);
    }
    if let Some(error) = create_error {
        return Err(error);
    }
    // SAFETY: `handle` is an open file handle transferred to `File`.
    Ok(unsafe { fs::File::from_raw_handle(handle) })
}

/// Names the certificate is valid for. Clients approve the fingerprint, so these only matter to
/// tools that validate host names.
fn subject_alt_names() -> Vec<String> {
    let mut names = vec!["localhost".to_owned()];
    for variable in ["COMPUTERNAME", "HOSTNAME"] {
        let Ok(hostname) = std::env::var(variable) else {
            continue;
        };
        let hostname = hostname.trim().to_owned();
        if is_dns_name(&hostname) && !names.contains(&hostname) {
            names.push(hostname);
        }
    }
    names
}

fn is_dns_name(candidate: &str) -> bool {
    !candidate.is_empty()
        && candidate.split('.').all(|label| {
            !label.is_empty()
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory under the test temp root, unique per call.
    fn temporary_directory() -> PathBuf {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("shaprint-identity-{}-{unique}", std::process::id()))
    }

    #[test]
    fn a_generated_identity_matches_its_fingerprint() {
        let identity = ServerIdentity::generate().expect("generates");

        assert_eq!(
            identity.fingerprint(),
            CertificateFingerprint::of_der(identity.certificate_der())
        );
        assert!(!identity.key_der().is_empty());
        assert!(format!("{identity:?}").contains(&identity.fingerprint().to_string()));
        // The key must never appear in a rendered identity.
        assert!(!format!("{identity:?}").contains("private"));
    }

    #[test]
    fn the_store_keeps_the_identity_between_runs() {
        let directory = temporary_directory();
        let store = FileIdentityStore::new(&directory);

        let created = store.load_or_create().expect("creates");
        let reloaded = store.load_or_create().expect("reloads");

        assert_eq!(created.fingerprint(), reloaded.fingerprint());
        assert_eq!(created.certificate_der(), reloaded.certificate_der());
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn the_store_refuses_a_half_written_identity() {
        let directory = temporary_directory();
        fs::create_dir_all(&directory).expect("creates the directory");
        fs::write(directory.join(KEY_FILE), b"key").expect("writes the key");

        let error = FileIdentityStore::new(&directory)
            .load_or_create()
            .expect_err("reports the mismatch");

        assert_eq!(error.code(), crate::domain::ErrorCode::Internal);
        // The files are left alone: replacing them would change a fingerprint clients approved.
        assert_eq!(
            fs::read(directory.join(KEY_FILE)).expect("still there"),
            b"key"
        );
        fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_stored_identity_with_a_missing_file_is_rejected() {
        assert!(ServerIdentity::from_der(Vec::new(), b"key".to_vec()).is_err());
        assert!(ServerIdentity::from_der(b"cert".to_vec(), Vec::new()).is_err());
    }

    #[test]
    fn only_usable_host_names_become_subject_alternatives() {
        assert!(is_dns_name("SHA-PRINT-01"));
        assert!(is_dns_name("print.example.com"));
        assert!(!is_dns_name(""));
        assert!(!is_dns_name("print host"));
        assert!(!is_dns_name("print_host"));
        assert!(!is_dns_name("print..host"));
        assert!(subject_alt_names().contains(&"localhost".to_owned()));
    }
}
