//! The elevated setup helper.
//!
//! ADR 0001: the desktop app runs as the logged-in user and a small helper with a narrow operation
//! surface handles the configuration that needs administrator rights. The helper is this same
//! executable started again with [`SETUP_FLAG`]; the app requests it through a UAC prompt and waits
//! for the result.

use crate::application::ElevationBroker;
use crate::domain::{AppError, SetupAction};

/// Command line flag that turns the executable into the elevated helper.
pub const SETUP_FLAG: &str = "--setup";

/// What a command line means for the executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandLine {
    /// Start the desktop app.
    Run,
    /// Perform one configuration action with administrator rights, then exit.
    Setup(SetupAction),
    /// A malformed helper request; the app reports it instead of starting normally.
    Invalid(String),
}

/// Reads the helper request out of a command line.
pub fn parse_command_line<S: AsRef<str>>(arguments: &[S]) -> CommandLine {
    let mut arguments = arguments.iter().skip(1).map(AsRef::as_ref);
    loop {
        match arguments.next() {
            None => return CommandLine::Run,
            Some(SETUP_FLAG) => {
                let Some(value) = arguments.next() else {
                    return CommandLine::Invalid(SETUP_FLAG.to_owned());
                };
                return match SetupAction::parse(value) {
                    Some(action) => CommandLine::Setup(action),
                    None => CommandLine::Invalid(value.to_owned()),
                };
            }
            // Ignore anything the platform passes that is not ours.
            Some(_) => {}
        }
    }
}

/// Requests administrator permission and runs the action elevated.
#[derive(Debug, Default)]
pub struct SystemElevation;

impl SystemElevation {
    pub fn new() -> Self {
        Self
    }
}

impl ElevationBroker for SystemElevation {
    fn elevate(&self, action: SetupAction) -> Result<(), AppError> {
        if !action.requires_elevation() {
            // Defensive: the use case filters these out, and running a routine action here would
            // prompt the user for nothing.
            return Err(AppError::invalid_input(format!(
                "{} does not need administrator permission",
                action.as_str()
            )));
        }
        elevate(action)
    }
}

/// Performs one configuration action from an elevated process of this same executable.
pub fn run_elevated(action: SetupAction) -> Result<(), AppError> {
    if !action.requires_elevation() {
        return Err(AppError::invalid_input(format!(
            "{} does not need administrator permission",
            action.as_str()
        )));
    }

    #[cfg(windows)]
    {
        windows::perform(action)
    }
    #[cfg(not(windows))]
    {
        Err(AppError::unsupported(format!(
            "{} needs administrator permission, which is only supported on Windows",
            action.as_str()
        )))
    }
}

#[cfg(not(windows))]
fn elevate(action: SetupAction) -> Result<(), AppError> {
    Err(AppError::unsupported(format!(
        "{} needs administrator permission, which is only supported on Windows",
        action.as_str()
    )))
}

#[cfg(windows)]
fn elevate(action: SetupAction) -> Result<(), AppError> {
    windows::elevate(action)
}

#[cfg(windows)]
mod windows {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
        SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::SETUP_FLAG;
    use crate::adapters::discovery::DISCOVERY_PORTS;
    use crate::adapters::ipps::DEFAULT_PORT;
    use crate::domain::{AppError, SetupAction};

    /// Windows error raised when the user declines the UAC prompt.
    const ERROR_CANCELLED: u32 = 1223;

    /// Re-runs this executable elevated and waits for the helper to finish.
    pub(super) fn elevate(action: SetupAction) -> Result<(), AppError> {
        let executable = std::env::current_exe().map_err(|error| {
            AppError::internal(format!("cannot locate this application: {error}"))
        })?;
        let verb = wide("runas");
        let file = wide_path(&executable);
        let parameters = wide(&format!("{SETUP_FLAG} {}", action.as_str()));

        // Safety: the strings stay alive for the duration of the call, and `info` is a plain
        // output structure the shell fills in.
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: parameters.as_ptr(),
            nShow: SW_SHOWNORMAL,
            ..Default::default()
        };
        // Safety: `info` is fully initialised and outlives the call.
        let started = unsafe { ShellExecuteExW(&mut info) };
        if started == 0 {
            // Safety: no pointers are involved.
            let code = unsafe { GetLastError() };
            if code == ERROR_CANCELLED {
                return Err(AppError::unsupported(
                    "administrator permission was not granted",
                ));
            }
            return Err(AppError::internal(format!(
                "cannot start the setup helper (Windows error {code})"
            )));
        }

        let exit_code = wait_for(info.hProcess);
        // Safety: the shell handed us the process handle and we are done with it.
        if !info.hProcess.is_null() {
            unsafe { CloseHandle(info.hProcess) };
        }

        match exit_code {
            Some(0) => {
                log::info!("setup helper finished action={}", action.as_str());
                Ok(())
            }
            Some(code) => Err(AppError::internal(format!(
                "the setup helper failed with exit code {code}"
            ))),
            None => Err(AppError::internal(
                "the setup helper did not report a result",
            )),
        }
    }

    /// Waits for the helper and returns its exit code.
    fn wait_for(process: HANDLE) -> Option<u32> {
        if process.is_null() {
            return None;
        }
        // Safety: `process` is a live process handle owned by the caller.
        if unsafe { WaitForSingleObject(process, INFINITE) } != WAIT_OBJECT_0 {
            return None;
        }
        let mut exit_code = u32::MAX;
        // Safety: `process` is signalled and `exit_code` is writable.
        if unsafe { GetExitCodeProcess(process, &mut exit_code) } == 0 {
            return None;
        }
        Some(exit_code)
    }

    /// Carries out the action inside the elevated process.
    pub(super) fn perform(action: SetupAction) -> Result<(), AppError> {
        match action {
            SetupAction::AllowInboundSharing => allow_inbound_sharing(),
            // The caller filters these out before the process is started.
            _ => Err(AppError::invalid_input(format!(
                "{} does not need administrator permission",
                action.as_str()
            ))),
        }
    }

    /// Lets clients reach this server through the Windows firewall.
    ///
    /// Two things have to get in: the IPPS requests a client sends to the endpoint's port, and the
    /// discovery queries a client sends to the ports a responder may have taken (ADR 0004).
    ///
    /// Every rule is removed first: re-running setup must repair a rule that changed, and `netsh`
    /// refuses to create a duplicate name.
    fn allow_inbound_sharing() -> Result<(), AppError> {
        let endpoint_rule = format!("ShaPrint ({DEFAULT_PORT})");
        let discovery_rules: Vec<(String, String)> = DISCOVERY_PORTS
            .iter()
            .map(|port| (format!("ShaPrint discovery ({port})"), port.to_string()))
            .collect();

        remove_rule(&endpoint_rule);
        for (name, _) in &discovery_rules {
            remove_rule(name);
        }

        run(
            "netsh",
            &[
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={endpoint_rule}"),
                "dir=in",
                "action=allow",
                "protocol=TCP",
                &format!("localport={DEFAULT_PORT}"),
                "profile=any",
            ],
        )?;

        // A client never has to be allowed in: it asks from its own port and only hears the answer,
        // which Windows lets back in as the reply to a request this machine started.
        for (name, port) in &discovery_rules {
            run(
                "netsh",
                &[
                    "advfirewall",
                    "firewall",
                    "add",
                    "rule",
                    &format!("name={name}"),
                    "dir=in",
                    "action=allow",
                    "protocol=UDP",
                    &format!("localport={port}"),
                    "profile=any",
                ],
            )?;
        }
        Ok(())
    }

    /// Removes one rule if it exists; a rule that is not there yet is not a failure.
    fn remove_rule(name: &str) {
        let _ = run(
            "netsh",
            &[
                "advfirewall",
                "firewall",
                "delete",
                "rule",
                &format!("name={name}"),
            ],
        );
    }

    fn run(program: &str, arguments: &[&str]) -> Result<(), AppError> {
        // The parent is a GUI process with no console: without this flag Windows would flash a
        // console window for every configuration command.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let output = std::process::Command::new(program)
            .args(arguments)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| AppError::internal(format!("cannot run {program}: {error}")))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(AppError::internal(format!(
                "{program} exited with {}",
                output.status
            )))
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_line_without_the_flag_starts_the_app() {
        assert_eq!(parse_command_line(&["shaprint.exe"]), CommandLine::Run);
        assert_eq!(
            parse_command_line(&["shaprint.exe", "--verbose"]),
            CommandLine::Run
        );
    }

    #[test]
    fn the_setup_flag_selects_the_helper_action() {
        assert_eq!(
            parse_command_line(&["shaprint.exe", SETUP_FLAG, "allow-inbound-sharing"]),
            CommandLine::Setup(SetupAction::AllowInboundSharing)
        );
    }

    #[test]
    fn an_unusable_helper_request_is_reported() {
        assert_eq!(
            parse_command_line(&["shaprint.exe", SETUP_FLAG]),
            CommandLine::Invalid(SETUP_FLAG.to_owned())
        );
        assert_eq!(
            parse_command_line(&["shaprint.exe", SETUP_FLAG, "install-printer"]),
            CommandLine::Invalid("install-printer".to_owned())
        );
    }

    #[test]
    fn the_helper_refuses_routine_actions() {
        let error = run_elevated(SetupAction::StartSharing).expect_err("refused");
        assert_eq!(error.code(), crate::domain::ErrorCode::InvalidInput);

        let error = SystemElevation::new()
            .elevate(SetupAction::StopSharing)
            .expect_err("refused");
        assert_eq!(error.code(), crate::domain::ErrorCode::InvalidInput);
    }
}
