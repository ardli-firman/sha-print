//! Login-startup registration for the current installation (#40, #200).
//!
//! On Windows, ShaPrint registers a scheduled task (`ShaPrint_Startup`) under
//! `BUILTIN\Administrators` (`S-1-5-32-544`) with `HighestAvailable` privileges so the elevated
//! desktop shell starts automatically at logon without UAC prompts or Win32 Error 740. Legacy
//! per-user `Run` registry keys are automatically cleaned up and migrated to the scheduled task,
//! and outdated or relocated tasks are detected by inspecting the live task XML.

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

/// Name of the elevated task registered in Windows Task Scheduler.
#[cfg(windows)]
pub const TASK_NAME: &str = "ShaPrint_Startup";

/// Well-known Security Identifier (SID) for the local Administrators group (`BUILTIN\Administrators`).
#[cfg(any(windows, test))]
pub const ADMINISTRATORS_GROUP_SID: &str = "S-1-5-32-544";

/// Canonical name Windows may emit instead of the SID when exporting the Administrators group.
#[cfg(any(windows, test))]
const ADMINISTRATORS_GROUP_NAME: &str = r"BUILTIN\Administrators";

/// Sentinel returned when a `ShaPrint_Startup` scheduled task exists in Windows Task Scheduler
/// but its XML is outdated, disabled, or malformed (for example, an older `S-1-5-32-545` task).
///
/// Returning a non-matching sentinel instead of `None` lets `Startup::apply_default()` both
/// upgrade the task when startup is enabled AND delete the leftover task when startup is disabled.
#[cfg(windows)]
const OUTDATED_TASK_SENTINEL: &str = "<outdated-task>";

/// Generates the XML definition for a Scheduled Task running with `HighestAvailable` privileges.
///
/// Note: Under the Windows Task Scheduler XML schema (`[MS-TSCH]` `principalType`), `<GroupId>`
/// implies group activation (`TASK_LOGON_GROUP`) and MUST NOT be paired with `<LogonType>`
/// (pairing `<GroupId>` with `<LogonType>InteractiveToken</LogonType>` causes `schtasks.exe` to
/// reject the XML with `SCHED_E_UNEXPECTEDNODE` / `SCHED_E_INVALIDVALUE`).
#[cfg(any(windows, test))]
pub fn generate_task_xml(exe_path: &str, argument: &str) -> String {
    let escaped_exe = escape_xml(exe_path);
    let escaped_arg = escape_xml(argument);
    let exe_dir = exe_path
        .rsplit_once(['\\', '/'])
        .map(|(dir, _)| dir)
        .filter(|dir| !dir.is_empty());
    let working_dir_tag = match exe_dir {
        Some(dir) => format!(
            "\n      <WorkingDirectory>{}</WorkingDirectory>",
            escape_xml(dir)
        ),
        None => String::new(),
    };

    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <GroupId>{ADMINISTRATORS_GROUP_SID}</GroupId>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{escaped_exe}</Command>
      <Arguments>{escaped_arg}</Arguments>{working_dir_tag}
    </Exec>
  </Actions>
</Task>"#
    )
}

#[cfg(any(windows, test))]
fn escape_xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(any(windows, test))]
fn unescape_xml(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Decodes stdout bytes from `schtasks.exe /query /xml`, which may be UTF-16 (with BOM) or UTF-8.
#[cfg(any(windows, test))]
fn decode_schtasks_output(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let trimmed = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(trimmed).into_owned()
}

/// Extracts the inner text of the first `<tag ...>...</tag>` element inside `xml`.
#[cfg(any(windows, test))]
fn extract_xml_element<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open_prefix = format!("<{tag}");
    let close_tag = format!("</{tag}>");
    let mut search_from = 0;

    while let Some(rel_start) = xml[search_from..].find(&open_prefix) {
        let start = search_from + rel_start;
        let after_name = start + open_prefix.len();
        let next_char = xml[after_name..].chars().next()?;
        if next_char != '>' && !next_char.is_whitespace() && next_char != '/' {
            search_from = after_name;
            continue;
        }
        let open_end = xml[after_name..].find('>')? + after_name;
        if xml[start..=open_end].trim_end().ends_with("/>") {
            return Some("");
        }
        let body_start = open_end + 1;
        let body_end = xml[body_start..].find(&close_tag)? + body_start;
        return Some(&xml[body_start..body_end]);
    }
    None
}

/// Validates an exported `schtasks.exe /query /xml` document and reconstructs the registered login
/// command line (`"<exe>" <args>`).
///
/// Returns `None` if the task is disabled, lacks a `<LogonTrigger>`, uses an outdated or invalid
/// `<Principal>` (such as `S-1-5-32-545`, missing `HighestAvailable`, or `<LogonType>` paired with
/// `<GroupId>`), or has no `<Exec><Command>`.
#[cfg(any(windows, test))]
pub fn parse_registered_task_command(xml: &str) -> Option<String> {
    // Task must have a LogonTrigger that is not explicitly disabled (omitted <Enabled> defaults to true).
    let trigger = extract_xml_element(xml, "LogonTrigger")?;
    if extract_xml_element(trigger, "Enabled")
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("false"))
    {
        return None;
    }

    // Settings must not explicitly disable the task (omitted <Enabled> defaults to true).
    if let Some(settings) = extract_xml_element(xml, "Settings") {
        if extract_xml_element(settings, "Enabled")
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("false"))
        {
            return None;
        }
    }

    // Principal must target BUILTIN\Administrators with HighestAvailable and no conflicting LogonType.
    let principal = extract_xml_element(xml, "Principal")?;
    let group_id = extract_xml_element(principal, "GroupId")?.trim();
    if !group_id.eq_ignore_ascii_case(ADMINISTRATORS_GROUP_SID)
        && !group_id.eq_ignore_ascii_case(ADMINISTRATORS_GROUP_NAME)
    {
        return None;
    }
    let run_level = extract_xml_element(principal, "RunLevel")?.trim();
    if !run_level.eq_ignore_ascii_case("HighestAvailable") {
        return None;
    }
    if extract_xml_element(principal, "LogonType").is_some() {
        return None;
    }

    // Reconstruct the canonical command line from <Exec>.
    let exec = extract_xml_element(xml, "Exec")?;
    let raw_command = unescape_xml(extract_xml_element(exec, "Command")?.trim());
    let command = raw_command.trim_matches('"').trim();
    if command.is_empty() {
        return None;
    }

    let arguments = extract_xml_element(exec, "Arguments")
        .map(|arg| unescape_xml(arg.trim()))
        .unwrap_or_default();
    let arguments = arguments.trim();

    if arguments.is_empty() {
        Some(format!("\"{command}\""))
    } else {
        Some(format!("\"{command}\" {arguments}"))
    }
}

/// Reads and writes the per-user login registration using Windows Task Scheduler (HighestAvailable)
/// with backward-compatible fallback to the Run key.
#[cfg(windows)]
pub struct WindowsStartup {
    command: String,
    executable_path: String,
}

#[cfg(windows)]
impl WindowsStartup {
    pub fn new() -> Self {
        let executable_path = match std::env::current_exe() {
            Ok(exe) => exe.to_string_lossy().to_string(),
            Err(_) => String::new(),
        };
        let startup = Self {
            command: login_command(),
            executable_path,
        };
        startup.migrate_legacy_run_key();
        startup
    }

    /// Automatically migrates any legacy registry Run key to Windows Task Scheduler (HighestAvailable)
    /// or cleans up obsolete Run keys if an up-to-date Task Scheduler task is already registered.
    fn migrate_legacy_run_key(&self) {
        if let Ok(Some(xml)) = task_scheduler::query_task_xml(TASK_NAME) {
            if parse_registered_task_command(&xml).as_deref() == Some(self.command.as_str()) {
                let _ = delete_run_value(RUN_VALUE);
                return;
            }
        }

        if matches!(read_run_value(RUN_VALUE), Ok(Some(_))) {
            log::info!("migrating legacy Run key to Windows Task Scheduler ({TASK_NAME}) with HighestAvailable");
            let _ = delete_run_value(RUN_VALUE);
            if !self.executable_path.is_empty() {
                if let Err(error) =
                    task_scheduler::create_task(TASK_NAME, &self.executable_path, BACKGROUND_ARG)
                {
                    log::warn!("cannot migrate legacy Run key to Task Scheduler: {error}");
                }
            }
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
        if let Some(xml) = task_scheduler::query_task_xml(TASK_NAME)? {
            let _ = delete_run_value(RUN_VALUE);
            return match parse_registered_task_command(&xml) {
                Some(command) => Ok(Some(command)),
                None => {
                    log::info!(
                        "existing scheduled task ({TASK_NAME}) is outdated or disabled; marking for repair"
                    );
                    Ok(Some(OUTDATED_TASK_SENTINEL.to_owned()))
                }
            };
        }

        if read_run_value(RUN_VALUE)?.is_some() {
            log::info!(
                "found legacy Run key during registered_command query; migrating to Task Scheduler"
            );
            let _ = delete_run_value(RUN_VALUE);
            if !self.executable_path.is_empty() {
                task_scheduler::create_task(TASK_NAME, &self.executable_path, BACKGROUND_ARG)?;
                return Ok(Some(self.command.clone()));
            }
        }

        Ok(None)
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), AppError> {
        let _ = delete_run_value(RUN_VALUE);

        if enabled {
            if self.executable_path.is_empty() {
                return Err(AppError::unsupported(
                    "Cannot locate the ShaPrint program to start it at login.",
                ));
            }
            task_scheduler::create_task(TASK_NAME, &self.executable_path, BACKGROUND_ARG)?;
        } else {
            task_scheduler::delete_task(TASK_NAME)?;
        }
        Ok(())
    }

    fn command(&self) -> String {
        self.command.clone()
    }
}

#[cfg(windows)]
mod task_scheduler {
    use super::AppError;
    use std::os::windows::process::CommandExt;
    use std::path::Path;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub(super) fn query_task_xml(task_name: &str) -> Result<Option<String>, AppError> {
        let output = std::process::Command::new("schtasks.exe")
            .args(["/query", "/tn", task_name, "/xml"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| {
                AppError::internal(format!("cannot query Windows Task Scheduler: {error}"))
            })?;

        if !output.status.success() {
            return Ok(None);
        }

        Ok(Some(super::decode_schtasks_output(&output.stdout)))
    }

    pub(super) fn create_task(task_name: &str, exe_path: &str, arg: &str) -> Result<(), AppError> {
        let xml = super::generate_task_xml(exe_path, arg);
        let temp_dir = std::env::temp_dir();
        let temp_xml = temp_dir.join(format!("{task_name}.xml"));

        write_utf16le_file(&temp_xml, &xml).map_err(|error| {
            AppError::internal(format!("cannot write Task Scheduler XML: {error}"))
        })?;

        let output = std::process::Command::new("schtasks.exe")
            .args([
                "/create",
                "/tn",
                task_name,
                "/xml",
                &temp_xml.to_string_lossy(),
                "/f",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let _ = std::fs::remove_file(&temp_xml);

        let output = output.map_err(|error| {
            AppError::internal(format!(
                "cannot execute schtasks.exe to create task: {error}"
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::internal(format!(
                "schtasks.exe failed to create task (exit code {:?}): {}",
                output.status.code(),
                stderr.trim()
            )));
        }

        Ok(())
    }

    pub(super) fn delete_task(task_name: &str) -> Result<(), AppError> {
        let _ = std::process::Command::new("schtasks.exe")
            .args(["/delete", "/tn", task_name, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        Ok(())
    }

    fn write_utf16le_file(path: &Path, content: &str) -> std::io::Result<()> {
        let mut bytes = vec![0xFF, 0xFE]; // UTF-16 LE BOM
        for unit in content.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(path, bytes)
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
    #[allow(dead_code)]
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
use registry::{delete_run_value, read_run_value};

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

    #[test]
    fn task_xml_contains_highest_available_and_logon_trigger() {
        let xml = generate_task_xml(
            "C:\\Program Files\\ShaPrint\\shaprint-desktop.exe",
            BACKGROUND_ARG,
        );
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<LogonTrigger>"));
        assert!(xml.contains(&format!("<GroupId>{ADMINISTRATORS_GROUP_SID}</GroupId>")));
        // Pairing <LogonType> with <GroupId> is rejected by Windows Task Scheduler XML validation.
        assert!(!xml.contains("<LogonType>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains(&format!("<Arguments>{BACKGROUND_ARG}</Arguments>")));
        assert!(
            xml.contains("<Command>C:\\Program Files\\ShaPrint\\shaprint-desktop.exe</Command>")
        );
        assert!(xml.contains("<WorkingDirectory>C:\\Program Files\\ShaPrint</WorkingDirectory>"));
        assert!(!xml.contains("&quot;"));
    }

    #[test]
    fn task_xml_omits_empty_working_directory() {
        let xml = generate_task_xml("shaprint-desktop.exe", BACKGROUND_ARG);
        assert!(!xml.contains("<WorkingDirectory>"));
    }

    #[test]
    fn generated_task_xml_round_trips_through_parser() {
        let xml = generate_task_xml(
            "C:\\Program Files\\ShaPrint\\shaprint-desktop.exe",
            BACKGROUND_ARG,
        );
        assert_eq!(
            parse_registered_task_command(&xml).as_deref(),
            Some(REGISTERED)
        );
    }

    #[test]
    fn parser_accepts_exported_windows_xml_with_omitted_default_enabled_and_builtin_name() {
        let exported = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Triggers>
    <LogonTrigger />
  </Triggers>
  <Principals>
    <Principal id="Author">
      <GroupId>BUILTIN\Administrators</GroupId>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"C:\Program Files\ShaPrint\shaprint-desktop.exe"</Command>
      <Arguments>--background</Arguments>
    </Exec>
  </Actions>
</Task>"#;

        assert_eq!(
            parse_registered_task_command(exported).as_deref(),
            Some(REGISTERED)
        );
    }

    #[test]
    fn parser_rejects_legacy_users_group_sid_task_so_upgrades_repair_it() {
        let legacy_xml = generate_task_xml(
            "C:\\Program Files\\ShaPrint\\shaprint-desktop.exe",
            BACKGROUND_ARG,
        )
        .replace(ADMINISTRATORS_GROUP_SID, "S-1-5-32-545");

        assert_eq!(parse_registered_task_command(&legacy_xml), None);
    }

    #[test]
    fn parser_rejects_invalid_logon_type_paired_with_group_id() {
        let invalid_xml = generate_task_xml(
            "C:\\Program Files\\ShaPrint\\shaprint-desktop.exe",
            BACKGROUND_ARG,
        )
        .replace(
            "<RunLevel>HighestAvailable</RunLevel>",
            "<LogonType>InteractiveToken</LogonType>\n      <RunLevel>HighestAvailable</RunLevel>",
        );

        assert_eq!(parse_registered_task_command(&invalid_xml), None);
    }

    #[test]
    fn parser_rejects_disabled_task_or_trigger() {
        let disabled_settings = generate_task_xml(
            "C:\\Program Files\\ShaPrint\\shaprint-desktop.exe",
            BACKGROUND_ARG,
        )
        .replace(
            "<AllowStartOnDemand>true</AllowStartOnDemand>\n    <Enabled>true</Enabled>",
            "<AllowStartOnDemand>true</AllowStartOnDemand>\n    <Enabled>false</Enabled>",
        );
        assert_eq!(parse_registered_task_command(&disabled_settings), None);

        let disabled_trigger = generate_task_xml(
            "C:\\Program Files\\ShaPrint\\shaprint-desktop.exe",
            BACKGROUND_ARG,
        )
        .replace(
            "<LogonTrigger>\n      <Enabled>true</Enabled>",
            "<LogonTrigger>\n      <Enabled>false</Enabled>",
        );
        assert_eq!(parse_registered_task_command(&disabled_trigger), None);
    }

    #[test]
    fn schtasks_output_decoder_handles_utf16_le_and_utf8_bom() {
        let sample = "<Task />";
        let mut utf16_le = vec![0xFF, 0xFE];
        for unit in sample.encode_utf16() {
            utf16_le.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_schtasks_output(&utf16_le), sample);

        let mut utf8_bom = vec![0xEF, 0xBB, 0xBF];
        utf8_bom.extend_from_slice(sample.as_bytes());
        assert_eq!(decode_schtasks_output(&utf8_bom), sample);
        assert_eq!(decode_schtasks_output(sample.as_bytes()), sample);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "registers and removes a temporary elevated Windows Scheduled Task; needs an elevated session"]
    fn windows_schtasks_creates_and_round_trips_generated_task_xml() {
        struct TaskCleanup(String);
        impl Drop for TaskCleanup {
            fn drop(&mut self) {
                let _ = task_scheduler::delete_task(&self.0);
            }
        }

        let task_name = format!("ShaPrint_Startup_Test_{}", std::process::id());
        let _cleanup = TaskCleanup(task_name.clone());
        let exe_path = r"C:\Program Files\ShaPrint\shaprint-desktop.exe";
        let expected_command = format!("\"{exe_path}\" {BACKGROUND_ARG}");

        task_scheduler::create_task(&task_name, exe_path, BACKGROUND_ARG)
            .expect("schtasks.exe accepts the generated XML");

        let exported_xml = task_scheduler::query_task_xml(&task_name)
            .expect("queries created task")
            .expect("created task exists in Windows Task Scheduler");

        assert_eq!(
            parse_registered_task_command(&exported_xml).as_deref(),
            Some(expected_command.as_str()),
            "exported Windows Task Scheduler XML did not round-trip:\n{exported_xml}"
        );
    }
}
