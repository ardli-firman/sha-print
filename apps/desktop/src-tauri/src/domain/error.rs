//! Typed recoverable failures that cross the IPC boundary.
//!
//! Codes are the stable part of the contract: the frontend maps them to messages and log lines
//! carry them for diagnosis. Messages are for humans and must never contain Network Channel
//! values, credentials, or print job content.

use thiserror::Error;

/// Stable error codes the shell reports to the frontend and to logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// The caller supplied a value the shell cannot use.
    InvalidInput,
    /// The requested service is not a runtime the shell supervises.
    UnknownService,
    /// The lifecycle action is not legal for the service's current state.
    InvalidState,
    /// A lifecycle action did not finish inside its deadline.
    Timeout,
    /// Unexpected failure; specifics belong in the message, never in the code.
    Internal,
}

impl ErrorCode {
    /// Every code the shell can report, in a fixed order.
    pub const ALL: [ErrorCode; 5] = [
        ErrorCode::InvalidInput,
        ErrorCode::UnknownService,
        ErrorCode::InvalidState,
        ErrorCode::Timeout,
        ErrorCode::Internal,
    ];

    /// The wire form of the code, shared by IPC payloads and log lines.
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidInput => "invalid-input",
            ErrorCode::UnknownService => "unknown-service",
            ErrorCode::InvalidState => "invalid-state",
            ErrorCode::Timeout => "timeout",
            ErrorCode::Internal => "internal",
        }
    }
}

/// A recoverable failure tagged with a stable code.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct AppError {
    code: ErrorCode,
    message: String,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn unknown_service(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::UnknownService, message)
    }

    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidState, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Timeout, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        let codes: Vec<&str> = ErrorCode::ALL.iter().map(|code| code.as_str()).collect();
        assert_eq!(
            codes,
            vec![
                "invalid-input",
                "unknown-service",
                "invalid-state",
                "timeout",
                "internal"
            ]
        );
    }

    #[test]
    fn error_codes_are_unique() {
        let mut seen = Vec::new();
        for code in ErrorCode::ALL {
            assert!(
                !seen.contains(&code.as_str()),
                "duplicate code {}",
                code.as_str()
            );
            seen.push(code.as_str());
        }
    }

    #[test]
    fn error_exposes_code_and_message() {
        let error = AppError::timeout("service 'client-proxy' did not stop in time");
        assert_eq!(error.code(), ErrorCode::Timeout);
        assert_eq!(error.code_str(), "timeout");
        assert_eq!(
            error.to_string(),
            "service 'client-proxy' did not stop in time"
        );
    }
}
