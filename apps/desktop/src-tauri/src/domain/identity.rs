//! The server's certificate identity.
//!
//! Clients approve the server by certificate fingerprint on first use (ADR 0001), so the identity
//! and its fingerprint are part of the domain: they are compared, displayed, and logged, while the
//! private key material stays in an adapter.

use std::fmt;

use sha2::{Digest, Sha256};

/// SHA-256 fingerprint of the server's TLS certificate, the value a user compares with what a
/// client shows before trusting the server.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CertificateFingerprint([u8; 32]);

impl CertificateFingerprint {
    /// Fingerprint of a DER-encoded X.509 certificate.
    pub fn of_der(certificate: &[u8]) -> Self {
        let digest = Sha256::digest(certificate);
        Self(digest.into())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hexadecimal, without separators; the form stored and compared.
    pub fn hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Display for CertificateFingerprint {
    /// Uppercase hexadecimal byte pairs separated by colons, the form certificate tools print.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = String::with_capacity(self.0.len() * 3);
        for (index, byte) in self.0.iter().enumerate() {
            if index > 0 {
                text.push(':');
            }
            text.push_str(&format!("{byte:02X}"));
        }
        formatter.write_str(&text)
    }
}

impl fmt::Debug for CertificateFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_the_sha256_of_the_certificate() {
        // SHA-256 of "shaprint".
        let fingerprint = CertificateFingerprint::of_der(b"shaprint");
        assert_eq!(
            fingerprint.hex(),
            "0fb767b01143a0e08aa85244e4a47dd4a810e6073d2acb96ec050dab47e529ee"
        );
        assert_eq!(fingerprint.as_bytes().len(), 32);
    }

    #[test]
    fn fingerprint_displays_as_uppercase_colon_separated_bytes() {
        let fingerprint = CertificateFingerprint::of_der(b"shaprint");
        let displayed = fingerprint.to_string();

        assert_eq!(displayed.split(':').count(), 32);
        assert_eq!(displayed.len(), 32 * 3 - 1);
        assert!(displayed
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == ':'));
        assert_eq!(displayed.to_uppercase(), displayed);
    }

    #[test]
    fn different_certificates_have_different_fingerprints() {
        assert_ne!(
            CertificateFingerprint::of_der(b"one"),
            CertificateFingerprint::of_der(b"two")
        );
        assert_eq!(
            CertificateFingerprint::of_der(b"one"),
            CertificateFingerprint::of_der(b"one")
        );
    }
}
