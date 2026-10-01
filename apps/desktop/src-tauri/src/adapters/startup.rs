//! Login-startup registration for the current user (#40).
//!
//! Windows runs the command stored under the per-user `Run` key. That key needs no administrator
//! rights and never touches machine-wide state, so starting at login stays an unprivileged action
//! (ADR 0001). The registration is refreshed when the program moves, and removing it is what the
//! user sees as turning "start at login" off.

use crate::application::StartupRegistration;
use crate::domain::AppError;

/// Argument a login launch passes so the app starts without showing its main window.
pub const BACKGROUND_ARG: &str = "--background";

/// Value name the app uses under the current user's `Run` key.
#[cfg(windows)]
const RUN_VALUE: &str = "ShaPrint";

/// Registry path of the per-user login commands.
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Whether this launch came from login rather than from the user opening the app.
pub fn launches_in_background<S: AsRef<str>>(arguments: &[S]) -> bool {
    arguments
        .iter()
        .skip(1)
        .any(|argument| argument.as_ref() == BACKGROUND_ARG)
}

/// The command line a login launch runs for this installation.
///
/// Empty when the program cannot be located, which the use case reports as unsupported rather than
/// registering a command that would not start anything.
pub fn login_command() -> String {
    match std::env::current_exe() {
        Ok(executable) => format!("\"{}\" {BACKGROUND_ARG}", executable.display()),
        Err(error) => {
            log::warn!("cannot locate the ShaPrint program message={error}");
            String::new()
        }
    }
}

/// Decodes the little-endian UTF-16 bytes Windows stores a `REG_SZ` value as.
///
/// The value ends at its null terminator; a trailing byte that cannot complete a code unit is
/// ignored rather than treated as text. This is the one part of reading the registration that is
/// not a call into Windows, so it is compiled for tests on every platform and checked there.
#[cfg(any(windows, test))]
fn decode_registry_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// Reads and writes the per-user login registration.
#[cfg(windows)]
pub struct WindowsStartup {
    command: String,
}

#[cfg(windows)]
impl WindowsStartup {
    pub fn new() -> Self {
        Self {
            command: login_command(),
        }
    }
}

#[cfg(windows)]
impl Default for WindowsStartup {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(windows)]
impl StartupRegistration for WindowsStartup {
    fn registered_command(&self) -> Result<Option<String>, AppError> {
        read_run_value(RUN_VALUE)
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), AppError> {
        if enabled {
            if self.command.is_empty() {
                return Err(AppError::unsupported(
                    "Cannot locate the ShaPrint program to start it at login.",
                ));
            }
            write_run_value(RUN_VALUE, &self.command)
        } else {
            delete_run_value(RUN_VALUE)
        }
    }

    fn command(&self) -> String {
        self.command.clone()
    }
}

/// Login startup is a Windows feature; other platforms report it as unavailable.
#[cfg(not(windows))]
pub struct UnsupportedStartup;

#[cfg(not(windows))]
impl StartupRegistration for UnsupportedStartup {
    fn is_supported(&self) -> bool {
        false
    }

    fn registered_command(&self) -> Result<Option<String>, AppError> {
        Err(AppError::unsupported(
            "Starting ShaPrint at login is available on Windows only.",
        ))
    }

    fn set_enabled(&self, _enabled: bool) -> Result<(), AppError> {
        Err(AppError::unsupported(
            "Starting ShaPrint at login is available on Windows only.",
        ))
    }

    fn command(&self) -> String {
        String::new()
    }
}

#[cfg(windows)]
mod registry {
    use super::{AppError, RUN_KEY};
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    /// Reads one string value from the per-user `Run` key.
    pub(super) fn read_run_value(name: &str) -> Result<Option<String>, AppError> {
        let Some(key) = open_run_key(KEY_QUERY_VALUE, false)? else {
            return Ok(None);
        };
        let wide = to_wide(name);
        let mut kind = 0u32;
        let mut size = 0u32;
        // A first call with no buffer reports the size the value needs.
        let status = unsafe {
            RegQueryValueExW(
                key,
                wide.as_ptr(),
                std::ptr::null(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            close(key);
            return Ok(None);
        }
        if status != ERROR_SUCCESS || kind != REG_SZ || size == 0 {
            close(key);
            return Err(AppError::internal(
                "Windows could not read the ShaPrint login registration.",
            ));
        }
        let mut buffer = vec![0u8; size as usize];
        let status = unsafe {
            RegQueryValueExW(
                key,
                wide.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buffer.as_mut_ptr(),
                &mut size,
            )
        };
        close(key);
        if status != ERROR_SUCCESS {
            return Err(AppError::internal(
                "Windows could not read the ShaPrint login registration.",
            ));
        }
        Ok(Some(super::decode_registry_string(&buffer)))
    }

    /// Writes one string value into the per-user `Run` key, creating the key when needed.
    pub(super) fn write_run_value(name: &str, value: &str) -> Result<(), AppError> {
        let Some(key) = open_run_key(KEY_SET_VALUE, true)? else {
            return Err(AppError::internal(
                "Windows could not open the ShaPrint login registration for writing.",
            ));
        };
        let name = to_wide(name);
        let value = to_wide(value);
        let bytes: Vec<u8> = value.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        let status = unsafe {
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        };
        close(key);
        if status != ERROR_SUCCESS {
            return Err(AppError::internal(
                "Windows could not save the ShaPrint login registration.",
            ));
        }
        Ok(())
    }

    /// Removes the value; removing something that is not there succeeds.
    pub(super) fn delete_run_value(name: &str) -> Result<(), AppError> {
        let Some(key) = open_run_key(KEY_SET_VALUE, false)? else {
            return Ok(());
        };
        let name = to_wide(name);
        let status = unsafe { RegDeleteValueW(key, name.as_ptr()) };
        close(key);
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            Err(AppError::internal(
                "Windows could not remove the ShaPrint login registration.",
            ))
        }
    }

    /// Opens the per-user `Run` key, creating it only when the caller is about to write.
    fn open_run_key(access: u32, create: bool) -> Result<Option<HKEY>, AppError> {
        let path = to_wide(RUN_KEY);
        let mut key: HKEY = std::ptr::null_mut();
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
        if status == ERROR_SUCCESS {
            return Ok(Some(key));
        }
        if !create {
            if status == ERROR_FILE_NOT_FOUND {
                // A missing key is how Windows reports that nothing is registered.
                return Ok(None);
            }
            return Err(AppError::internal(
                "Windows could not read the ShaPrint login registration.",
            ));
        }
        let mut created: HKEY = std::ptr::null_mut();
        let mut disposition = 0u32;
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                access,
                std::ptr::null(),
                &mut created,
                &mut disposition,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(AppError::internal(
                "Windows could not open the ShaPrint login registration.",
            ));
        }
        Ok(Some(created))
    }

    fn close(key: HKEY) {
        unsafe {
            RegCloseKey(key);
        }
    }

    fn to_wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
use registry::{delete_run_value, read_run_value, write_run_value};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_launch_is_recognised() {
        assert!(launches_in_background(&[
            "shaprint-desktop.exe",
            BACKGROUND_ARG
        ]));
        assert!(launches_in_background(&[
            "shaprint-desktop.exe",
            "--verbose",
            BACKGROUND_ARG
        ]));
    }

    #[test]
    fn the_program_name_is_not_mistaken_for_the_flag() {
        assert!(!launches_in_background(&["--background-like"]));
        assert!(!launches_in_background(&["shaprint-desktop.exe"]));
        assert!(!launches_in_background::<&str>(&[]));
    }

    #[test]
    fn the_login_command_starts_the_app_in_the_background() {
        let command = login_command();

        assert!(command.ends_with(BACKGROUND_ARG), "{command}");
        assert!(command.starts_with('"'), "{command}");
    }

    /// A helper for the decoding test: `text` as Windows stores a `REG_SZ` value.
    fn registry_bytes(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    /// A command line as the registration records it, with a path that needs quoting.
    const REGISTERED: &str = "\"C:\\Program Files\\ShaPrint\\shaprint-desktop.exe\" --background";

    #[test]
    fn a_registry_string_is_decoded_as_little_endian_utf16() {
        assert_eq!(
            decode_registry_string(&registry_bytes(REGISTERED)),
            REGISTERED
        );
        assert_eq!(
            decode_registry_string(&registry_bytes("ShaPrint")),
            "ShaPrint"
        );
        assert_eq!(decode_registry_string(&registry_bytes("")), "");
    }

    #[test]
    fn a_registry_string_ends_at_its_null_terminator() {
        // Windows reports the size it wrote; anything after the terminator is not part of the value.
        let mut bytes = registry_bytes("ShaPrint");
        bytes.extend_from_slice(&u16::to_le_bytes(b'X' as u16));

        assert_eq!(decode_registry_string(&bytes), "ShaPrint");
    }

    #[test]
    fn an_incomplete_trailing_code_unit_is_ignored() {
        let mut bytes = registry_bytes("ab");
        bytes.push(0xFF);

        assert_eq!(decode_registry_string(&bytes), "ab");
        assert_eq!(decode_registry_string(&[]), "");
    }
}
