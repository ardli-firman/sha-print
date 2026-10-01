//! The login-startup use case: whether ShaPrint starts with Windows, and changing that (#40).
//!
//! Registration is per user, so the app never needs administrator rights for it. The platform
//! adapter that writes the registration lives in `adapters::startup`; this module owns the rules:
//! what "registered" means, when the default is applied, and how the window reports it.
//!
//! The default is applied once, on a computer where the user has never made a choice. After that a
//! launch never touches the registration again, because turning login startup off has to stick.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::domain::AppError;

/// File that records the user's own choice about starting with Windows.
const PREFERENCE_FILE: &str = "login-startup.json";

/// Whether login startup is supported here, and whether it is currently on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupStatus {
    /// Whether this platform registers login startup at all.
    pub supported: bool,
    /// Whether the registration matches this installation, so a login launch runs this program.
    pub enabled: bool,
    /// The command a login launch runs; empty when the program could not be located.
    pub command: String,
}

/// The platform seam that reads and writes the login registration.
pub trait StartupRegistration: Send + Sync + 'static {
    /// Whether this platform has a login-startup mechanism.
    fn is_supported(&self) -> bool {
        true
    }

    /// The command login currently runs, or `None` when nothing is registered.
    fn registered_command(&self) -> Result<Option<String>, AppError>;

    /// Registers or unregisters the app. Registering the command that is already there changes
    /// nothing.
    fn set_enabled(&self, enabled: bool) -> Result<(), AppError>;

    /// The command registration uses for this installation.
    fn command(&self) -> String;
}

/// Reads and changes whether ShaPrint starts at Windows login.
pub struct Startup {
    registration: Arc<dyn StartupRegistration>,
    preference_path: Option<PathBuf>,
}

impl Startup {
    /// `data_dir` is where the user's recorded choice lives; `None` keeps it in memory, which is
    /// what a test without a directory needs.
    pub fn new(registration: Arc<dyn StartupRegistration>, data_dir: Option<&Path>) -> Self {
        Self {
            registration,
            preference_path: data_dir.map(|directory| directory.join(PREFERENCE_FILE)),
        }
    }

    /// The current state, without changing anything.
    ///
    /// A registration that names a different program — an older install, or one that moved — is
    /// reported as not enabled instead of as a working registration.
    pub fn status(&self) -> StartupStatus {
        let command = self.registration.command();
        if !self.registration.is_supported() {
            return StartupStatus {
                supported: false,
                enabled: false,
                command,
            };
        }
        let enabled = match self.registration.registered_command() {
            Ok(registered) => {
                registered.as_deref() == Some(command.as_str()) && !command.is_empty()
            }
            Err(error) => {
                log::warn!(
                    "cannot read the login registration code={} message={}",
                    error.code_str(),
                    error
                );
                false
            }
        };
        StartupStatus {
            supported: true,
            enabled,
            command,
        }
    }

    /// Applies the product default once: ShaPrint starts with Windows unless the user says otherwise.
    ///
    /// A computer where the user has never made a choice gets the registration, including a refresh
    /// when an older installation's command is still recorded. Once a choice exists — including
    /// "off" — a launch leaves the registration exactly as the user left it.
    pub fn apply_default(&self) -> Result<(), AppError> {
        self.require_supported()?;
        if self.preference().is_some() {
            return Ok(());
        }
        let command = self.require_command()?;
        if self.registration.registered_command()?.as_deref() != Some(command.as_str()) {
            self.registration.set_enabled(true)?;
        }
        self.remember(true)
    }

    /// Registers or unregisters ShaPrint for login and reports the resulting state.
    ///
    /// The choice is recorded, so the next launch does not undo it.
    pub fn set_enabled(&self, enabled: bool) -> Result<StartupStatus, AppError> {
        self.require_supported()?;
        if enabled {
            self.require_command()?;
        }
        self.registration.set_enabled(enabled)?;
        self.remember(enabled)?;
        Ok(self.status())
    }

    /// The user's recorded choice, when there is one.
    ///
    /// An unreadable or unrecognised record still counts as a choice, so a launch never quietly
    /// re-registers over something the user may have turned off.
    fn preference(&self) -> Option<bool> {
        let path = self.preference_path.as_ref()?;
        match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<LoginPreference>(&bytes) {
                Ok(preference) => Some(preference.enabled),
                Err(_) => {
                    log::warn!("the login startup choice could not be read; leaving it untouched");
                    Some(false)
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                log::warn!("cannot read the login startup choice message={error}");
                Some(false)
            }
        }
    }

    fn remember(&self, enabled: bool) -> Result<(), AppError> {
        let Some(path) = &self.preference_path else {
            return Ok(());
        };
        let bytes = serde_json::to_vec(&LoginPreference { enabled })
            .map_err(|_| AppError::internal("cannot record the login startup choice"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| AppError::internal("cannot record the login startup choice"))?;
        }
        std::fs::write(path, bytes)
            .map_err(|_| AppError::internal("cannot record the login startup choice"))
    }

    fn require_supported(&self) -> Result<(), AppError> {
        if self.registration.is_supported() {
            Ok(())
        } else {
            Err(AppError::unsupported(
                "Starting ShaPrint at login is available on Windows only.",
            ))
        }
    }

    fn require_command(&self) -> Result<String, AppError> {
        let command = self.registration.command();
        if command.is_empty() {
            return Err(AppError::unsupported(
                "Cannot locate the ShaPrint program to start it at login.",
            ));
        }
        Ok(command)
    }
}

/// What the user decided about starting with Windows.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct LoginPreference {
    enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;
    use crate::test_support::temporary_directory;
    use std::sync::Mutex;

    const COMMAND: &str = "\"C:\\Program Files\\ShaPrint\\shaprint-desktop.exe\" --background";

    /// A registration that keeps its state in memory and counts writes.
    struct FakeRegistration {
        supported: bool,
        command: String,
        registered: Mutex<Option<String>>,
        writes: Mutex<Vec<bool>>,
        read_error: bool,
    }

    impl FakeRegistration {
        fn new(registered: Option<&str>) -> Arc<Self> {
            Self::build(true, COMMAND, registered)
        }

        fn unsupported() -> Arc<Self> {
            Self::build(false, "", None)
        }

        fn unreadable() -> Arc<Self> {
            let built = Self::build(true, COMMAND, None);
            let registration = built.as_ref();
            Arc::new(Self {
                supported: registration.supported,
                command: registration.command.clone(),
                registered: Mutex::new(None),
                writes: Mutex::new(Vec::new()),
                read_error: true,
            })
        }

        fn build(supported: bool, command: &str, registered: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                supported,
                command: command.to_owned(),
                registered: Mutex::new(registered.map(str::to_owned)),
                writes: Mutex::new(Vec::new()),
                read_error: false,
            })
        }

        fn registered(&self) -> Option<String> {
            self.registered
                .lock()
                .map(|value| value.clone())
                .unwrap_or_default()
        }

        fn writes(&self) -> Vec<bool> {
            self.writes
                .lock()
                .map(|writes| writes.clone())
                .unwrap_or_default()
        }
    }

    impl StartupRegistration for FakeRegistration {
        fn is_supported(&self) -> bool {
            self.supported
        }

        fn registered_command(&self) -> Result<Option<String>, AppError> {
            if self.read_error {
                return Err(AppError::internal("the registry could not be read"));
            }
            Ok(self.registered())
        }

        fn set_enabled(&self, enabled: bool) -> Result<(), AppError> {
            if let Ok(mut writes) = self.writes.lock() {
                writes.push(enabled);
            }
            if let Ok(mut registered) = self.registered.lock() {
                *registered = enabled.then(|| self.command.clone());
            }
            Ok(())
        }

        fn command(&self) -> String {
            self.command.clone()
        }
    }

    fn startup(registration: Arc<FakeRegistration>, directory: &Path) -> Startup {
        Startup::new(registration, Some(directory))
    }

    #[test]
    fn nothing_is_registered_before_the_first_launch() {
        let directory = temporary_directory("first-status");
        let registration = FakeRegistration::new(None);

        let status = startup(Arc::clone(&registration), &directory).status();

        assert!(status.supported);
        assert!(!status.enabled);
        assert_eq!(status.command, COMMAND);
        assert!(registration.writes().is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_registration_that_names_this_install_is_enabled() {
        let directory = temporary_directory("names-this");
        let status = startup(FakeRegistration::new(Some(COMMAND)), &directory).status();

        assert!(status.enabled);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_registration_that_names_another_program_is_not_enabled() {
        let directory = temporary_directory("names-other");
        let status = startup(
            FakeRegistration::new(Some("\"D:\\old\\shaprint.exe\"")),
            &directory,
        )
        .status();

        assert!(status.supported);
        assert!(!status.enabled);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn unsupported_platforms_report_themselves_instead_of_failing() {
        let directory = temporary_directory("unsupported");
        let status = startup(FakeRegistration::unsupported(), &directory).status();

        assert!(!status.supported);
        assert!(!status.enabled);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_unreadable_registration_is_reported_as_not_enabled() {
        let directory = temporary_directory("unreadable");
        let status = startup(FakeRegistration::unreadable(), &directory).status();

        assert!(status.supported);
        assert!(!status.enabled);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_first_launch_registers_the_app_and_records_the_choice() {
        let directory = temporary_directory("first-launch");
        let registration = FakeRegistration::new(None);
        let startup = startup(Arc::clone(&registration), &directory);

        startup.apply_default().expect("registers");

        assert_eq!(registration.writes(), vec![true]);
        assert_eq!(registration.registered().as_deref(), Some(COMMAND));
        assert!(startup.status().enabled);
        assert!(directory.join(PREFERENCE_FILE).exists());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_registration_from_an_older_install_is_refreshed_on_the_first_launch() {
        let directory = temporary_directory("refresh");
        let registration = FakeRegistration::new(Some("\"D:\\old\\shaprint.exe\""));
        let startup = startup(Arc::clone(&registration), &directory);

        startup.apply_default().expect("refreshes");

        assert_eq!(registration.writes(), vec![true]);
        assert_eq!(registration.registered().as_deref(), Some(COMMAND));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn repeating_a_start_is_not_a_second_registration() {
        let directory = temporary_directory("repeat");
        let registration = FakeRegistration::new(Some(COMMAND));
        let startup = startup(Arc::clone(&registration), &directory);

        startup.apply_default().expect("records the choice");
        startup.apply_default().expect("stays registered");

        assert!(registration.writes().is_empty());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn turning_login_startup_off_survives_the_next_launch() {
        let directory = temporary_directory("off-sticks");
        let registration = FakeRegistration::new(None);
        let startup = startup(Arc::clone(&registration), &directory);
        startup.apply_default().expect("registers");

        assert!(!startup.set_enabled(false).expect("disables").enabled);
        assert_eq!(registration.registered(), None);

        // The next launch must not put it back: the user turned it off on purpose.
        startup.apply_default().expect("leaves the choice alone");
        assert_eq!(registration.registered(), None);
        assert_eq!(registration.writes(), vec![true, false]);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_user_can_turn_login_startup_on_again() {
        let directory = temporary_directory("on-again");
        let registration = FakeRegistration::new(Some(COMMAND));
        let startup = startup(Arc::clone(&registration), &directory);

        assert!(!startup.set_enabled(false).expect("disables").enabled);
        assert!(startup.set_enabled(true).expect("enables").enabled);

        assert_eq!(registration.registered().as_deref(), Some(COMMAND));
        assert_eq!(registration.writes(), vec![false, true]);
        startup.apply_default().expect("leaves the choice alone");
        assert_eq!(registration.writes(), vec![false, true]);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_unrecognised_choice_record_is_never_overridden() {
        let directory = temporary_directory("corrupt-choice");
        std::fs::write(directory.join(PREFERENCE_FILE), "not json")
            .expect("writes a corrupt record");
        let registration = FakeRegistration::new(None);
        let startup = startup(Arc::clone(&registration), &directory);

        startup.apply_default().expect("leaves it alone");

        assert!(registration.writes().is_empty());
        assert_eq!(registration.registered(), None);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn changing_login_startup_on_an_unsupported_platform_is_rejected() {
        let directory = temporary_directory("unsupported-change");
        let startup = startup(FakeRegistration::unsupported(), &directory);

        let error = startup.apply_default().expect_err("rejected");
        assert_eq!(error.code(), ErrorCode::Unsupported);

        let error = startup.set_enabled(true).expect_err("rejected");
        assert_eq!(error.code(), ErrorCode::Unsupported);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_program_that_cannot_be_located_is_never_registered() {
        let directory = temporary_directory("no-command");
        let registration = FakeRegistration::build(true, "", None);
        let startup = startup(Arc::clone(&registration), &directory);

        let error = startup.apply_default().expect_err("rejected");

        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert!(registration.writes().is_empty());
        assert!(!directory.join(PREFERENCE_FILE).exists());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
