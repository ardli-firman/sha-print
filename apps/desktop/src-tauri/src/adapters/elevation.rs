//! The elevated setup helper.
//!
//! ADR 0001: the desktop app runs as the logged-in user and a small helper with a narrow operation
//! surface handles the configuration that needs administrator rights. The helper is this same
//! executable started again with [`SETUP_FLAG`]; the app requests it through a UAC prompt and waits
//! for the result (ADR 0005).
//!
//! The helper reports a classified failure through its exit code ([`SetupFailureKind::exit_code`])
//! and keeps Windows' own text on standard error. Only the classification crosses the process
//! boundary, so the app can always say what the user should do next.

use crate::application::ElevationBroker;
use crate::domain::{ClientQueueRequest, PrinterName, SetupAction, SetupFailure, SetupFailureKind};

/// Command line flag that turns the executable into the elevated helper.
pub const SETUP_FLAG: &str = "--setup";

/// Command line option carrying the trusted server an install request targets.
pub const SERVER_OPTION: &str = "--server";

/// Command line option carrying the shared printer an install request targets.
pub const PRINTER_OPTION: &str = "--printer";

/// What the helper was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperRequest {
    /// A configuration action that needs no further input.
    Action(SetupAction),
    /// Install (or repair) the native Windows queue for one shared printer.
    InstallClientQueue(ClientQueueRequest),
}

/// What a command line means for the executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandLine {
    /// Start the desktop app.
    Run,
    /// Perform one configuration action with administrator rights, then exit.
    Setup(HelperRequest),
    /// A malformed helper request; the app reports it instead of starting normally.
    Invalid(String),
}

/// Reads the helper request out of a command line.
pub fn parse_command_line<S: AsRef<str>>(arguments: &[S]) -> CommandLine {
    let mut arguments = arguments.iter().skip(1).map(AsRef::as_ref);
    loop {
        match arguments.next() {
            None => return CommandLine::Run,
            Some(SETUP_FLAG) => return parse_setup(&mut arguments),
            // Ignore anything the platform passes that is not ours.
            Some(_) => {}
        }
    }
}

/// Reads the action and any data that follows [`SETUP_FLAG`].
fn parse_setup(arguments: &mut dyn Iterator<Item = &str>) -> CommandLine {
    let Some(action) = arguments.next() else {
        return CommandLine::Invalid(SETUP_FLAG.to_owned());
    };
    match SetupAction::parse(action) {
        // Installing a queue is the one action that carries data, so it is parsed separately.
        Some(SetupAction::InstallPrinter) => parse_install(arguments),
        Some(action) => CommandLine::Setup(HelperRequest::Action(action)),
        None => CommandLine::Invalid(action.to_owned()),
    }
}

/// Reads `--server` and `--printer`, both required exactly once.
///
/// An unrecognised option is an error rather than something to ignore: a typo must not silently
/// install a queue for the wrong server or printer.
fn parse_install(arguments: &mut dyn Iterator<Item = &str>) -> CommandLine {
    let mut server: Option<&str> = None;
    let mut printer: Option<&str> = None;
    while let Some(option) = arguments.next() {
        let target = match option {
            SERVER_OPTION => &mut server,
            PRINTER_OPTION => &mut printer,
            other => return CommandLine::Invalid(other.to_owned()),
        };
        let Some(value) = arguments.next() else {
            return CommandLine::Invalid(option.to_owned());
        };
        if target.is_some() {
            return CommandLine::Invalid(option.to_owned());
        }
        *target = Some(value);
    }

    let (Some(server), Some(printer)) = (server, printer) else {
        return CommandLine::Invalid(SetupAction::InstallPrinter.as_str().to_owned());
    };
    // The helper re-validates everything it was handed: it must not install a queue for an address
    // or printer the app would have refused.
    match PrinterName::parse(printer).and_then(|printer| ClientQueueRequest::new(server, printer)) {
        Ok(request) => CommandLine::Setup(HelperRequest::InstallClientQueue(request)),
        Err(error) => CommandLine::Invalid(error.message().to_owned()),
    }
}

/// The parameters the app passes when it starts the helper for `request`.
pub fn helper_parameters(request: &HelperRequest) -> String {
    match request {
        HelperRequest::Action(action) => format!("{SETUP_FLAG} {}", action.as_str()),
        HelperRequest::InstallClientQueue(request) => format!(
            "{SETUP_FLAG} {} {SERVER_OPTION} {} {PRINTER_OPTION} {}",
            SetupAction::InstallPrinter.as_str(),
            quote_argument(request.server_address()),
            quote_argument(request.printer().as_str())
        ),
    }
}

/// Quotes one argument the way the Windows command line parser expects.
///
/// A printer queue name may contain spaces and quotes, and `lpParameters` is a single string, so
/// quoting is what keeps the helper's options apart.
fn quote_argument(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    let mut backslashes = 0usize;
    for character in value.chars() {
        match character {
            '\\' => {
                backslashes += 1;
                quoted.push('\\');
            }
            '"' => {
                // Backslashes before a quote must be doubled so they do not escape it.
                for _ in 0..backslashes {
                    quoted.push('\\');
                }
                backslashes = 0;
                quoted.push('\\');
                quoted.push('"');
            }
            other => {
                backslashes = 0;
                quoted.push(other);
            }
        }
    }
    // Trailing backslashes would escape the closing quote, so they are doubled too.
    for _ in 0..backslashes {
        quoted.push('\\');
    }
    quoted.push('"');
    quoted
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
    fn inbound_sharing_allowed(&self) -> Result<bool, SetupFailure> {
        #[cfg(windows)]
        {
            windows::inbound_sharing_allowed()
        }
        #[cfg(not(windows))]
        {
            Err(SetupFailure::new(
                SetupFailureKind::Unsupported,
                "inbound firewall access is only supported on Windows",
            ))
        }
    }

    fn elevate(&self, action: SetupAction) -> Result<(), SetupFailure> {
        if !action.requires_elevation() {
            // Defensive: the use case filters these out, and running a routine action here would
            // prompt the user for nothing.
            return Err(SetupFailure::new(
                SetupFailureKind::InvalidRequest,
                format!("{} does not need administrator permission", action.as_str()),
            ));
        }
        if action == SetupAction::InstallPrinter {
            // Queue installation needs the printer it targets; it has its own operation.
            return Err(SetupFailure::new(
                SetupFailureKind::InvalidRequest,
                format!(
                    "{} needs the shared printer it targets; use the queue installation operation",
                    action.as_str()
                ),
            ));
        }
        start_helper(&HelperRequest::Action(action))
    }

    fn install_queue(&self, request: &ClientQueueRequest) -> Result<(), SetupFailure> {
        start_helper(&HelperRequest::InstallClientQueue(request.clone()))
    }
}

/// Performs one request from inside an elevated process of this same executable.
pub fn run_elevated(request: HelperRequest) -> Result<(), SetupFailure> {
    match request {
        HelperRequest::Action(action) => run_action(action),
        // Non-Windows builds report `unsupported` from their own adapter.
        HelperRequest::InstallClientQueue(request) => {
            crate::adapters::queue_installation::platform_installer().install(&request)
        }
    }
}

/// Performs one parameterless action from inside the elevated helper.
fn run_action(action: SetupAction) -> Result<(), SetupFailure> {
    if !action.requires_elevation() {
        return Err(SetupFailure::new(
            SetupFailureKind::InvalidRequest,
            format!("{} does not need administrator permission", action.as_str()),
        ));
    }

    #[cfg(windows)]
    {
        windows::perform(action)
    }
    #[cfg(not(windows))]
    {
        Err(SetupFailure::new(
            SetupFailureKind::Unsupported,
            format!(
                "{} needs administrator permission, which is only supported on Windows",
                action.as_str()
            ),
        ))
    }
}

#[cfg(not(windows))]
fn start_helper(request: &HelperRequest) -> Result<(), SetupFailure> {
    Err(SetupFailure::new(
        SetupFailureKind::Unsupported,
        format!(
            "{} needs administrator permission, which is only supported on Windows",
            helper_parameters(request)
        ),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) struct InboundRule {
    pub name: String,
    pub protocol: &'static str,
    pub port: u16,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn inbound_rules() -> Vec<InboundRule> {
    let mut rules = vec![InboundRule {
        name: format!("ShaPrint ({})", crate::adapters::ipps::DEFAULT_PORT),
        protocol: "TCP",
        port: crate::adapters::ipps::DEFAULT_PORT,
    }];
    rules.extend(
        crate::adapters::discovery::DISCOVERY_PORTS
            .iter()
            .map(|port| InboundRule {
                name: format!("ShaPrint discovery ({port})"),
                protocol: "UDP",
                port: *port,
            }),
    );
    rules
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(super) fn netsh_add_rule_args(rule: &InboundRule) -> Vec<String> {
    vec![
        "advfirewall".to_owned(),
        "firewall".to_owned(),
        "add".to_owned(),
        "rule".to_owned(),
        format!("name={}", rule.name),
        "dir=in".to_owned(),
        "action=allow".to_owned(),
        format!("protocol={}", rule.protocol),
        format!("localport={}", rule.port),
        "profile=any".to_owned(),
        "remoteip=any".to_owned(),
    ]
}

#[cfg(windows)]
fn start_helper(request: &HelperRequest) -> Result<(), SetupFailure> {
    windows::start_helper(request)
}

#[cfg(windows)]
mod windows {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
        SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::{helper_parameters, inbound_rules, netsh_add_rule_args, HelperRequest};
    use crate::domain::{SetupAction, SetupFailure, SetupFailureKind};

    /// Windows error raised when the user declines the UAC prompt.
    const ERROR_CANCELLED: u32 = 1223;

    /// How long the app waits for the helper before it gives up and terminates it. Installing a
    /// queue talks to the spooler, which can be slow, but it must never hang the app forever.
    const HELPER_TIMEOUT_MILLIS: u32 = 120_000;

    /// How the helper process ended.
    enum HelperWait {
        Exited(u32),
        /// The helper overstayed its deadline; `terminated` says whether Windows ended it.
        TimedOut {
            terminated: bool,
        },
        Unreported,
    }

    /// Returns whether the current process is running with elevated (administrator) privileges.
    pub(super) fn is_elevated() -> bool {
        use windows_sys::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION {
                TokenIsElevated: 0,
            };
            let mut size = size_of::<TOKEN_ELEVATION>() as u32;
            let success = GetTokenInformation(
                token,
                TokenElevation,
                &mut elevation as *mut _ as *mut _,
                size,
                &mut size,
            );
            CloseHandle(token);
            success != 0 && elevation.TokenIsElevated != 0
        }
    }

    /// Re-runs this executable elevated and waits for the helper to finish.
    pub(super) fn start_helper(request: &HelperRequest) -> Result<(), SetupFailure> {
        if is_elevated() {
            log::info!("current process is already elevated; performing action directly");
            return super::run_elevated(request.clone());
        }

        let executable = std::env::current_exe().map_err(|error| {
            SetupFailure::new(
                SetupFailureKind::Other,
                format!("cannot locate this application: {error}"),
            )
        })?;
        let verb = wide("runas");
        let file = wide_path(&executable);
        let parameters = wide(&helper_parameters(request));

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
                return Err(SetupFailure::new(
                    SetupFailureKind::PermissionDenied,
                    "the UAC prompt was dismissed",
                ));
            }
            return Err(SetupFailure::new(
                SetupFailureKind::Other,
                format!("cannot start the setup helper (Windows error {code})"),
            ));
        }

        let waited = wait_for(info.hProcess);
        // Safety: the shell handed us the process handle and we are done with it.
        if !info.hProcess.is_null() {
            unsafe { CloseHandle(info.hProcess) };
        }

        match waited {
            HelperWait::Exited(0) => {
                log::info!("setup helper finished action={}", verb_summary(request));
                Ok(())
            }
            HelperWait::Exited(code) => Err(SetupFailure::new(
                super::classify_exit_code(code as i32),
                format!("the setup helper exited with code {code}"),
            )),
            HelperWait::TimedOut { terminated } => Err(super::timed_out_failure(terminated)),
            HelperWait::Unreported => Err(SetupFailure::new(
                SetupFailureKind::Other,
                "the setup helper did not report a result",
            )),
        }
    }

    /// What the helper was asked to do, for the log line only.
    fn verb_summary(request: &HelperRequest) -> String {
        match request {
            HelperRequest::Action(action) => action.as_str().to_owned(),
            HelperRequest::InstallClientQueue(request) => {
                format!("install-printer queue={}", request.queue_name())
            }
        }
    }

    /// Waits for the helper, terminating it if it overstays its deadline.
    fn wait_for(process: HANDLE) -> HelperWait {
        if process.is_null() {
            return HelperWait::Unreported;
        }
        // Safety: `process` is a live process handle owned by the caller.
        let waited = unsafe { WaitForSingleObject(process, HELPER_TIMEOUT_MILLIS) };
        if waited == WAIT_TIMEOUT {
            // Safety: `process` is a live handle to the helper we started.
            //
            // The app runs unelevated while the helper runs elevated, and Windows integrity control
            // refuses write access to a higher-integrity process, so ending it can legitimately
            // fail. Report what actually happened rather than claiming the helper was stopped.
            let terminated = unsafe { TerminateProcess(process, 1) } != 0;
            return HelperWait::TimedOut { terminated };
        }
        if waited != WAIT_OBJECT_0 {
            return HelperWait::Unreported;
        }
        let mut exit_code = u32::MAX;
        // Safety: `process` is signalled and `exit_code` is writable.
        if unsafe { GetExitCodeProcess(process, &mut exit_code) } == 0 {
            return HelperWait::Unreported;
        }
        HelperWait::Exited(exit_code)
    }

    /// Carries out the action inside the elevated process.
    pub(super) fn perform(action: SetupAction) -> Result<(), SetupFailure> {
        match action {
            SetupAction::AllowInboundSharing => allow_inbound_sharing(),
            // The caller routes these two to their own operations.
            SetupAction::InstallPrinter => Err(SetupFailure::new(
                SetupFailureKind::InvalidRequest,
                "install-printer needs the shared printer it targets",
            )),
            _ => Err(SetupFailure::new(
                SetupFailureKind::InvalidRequest,
                format!("{} does not need administrator permission", action.as_str()),
            )),
        }
    }

    /// Reads effective firewall policy, including rules installed by an administrator under a
    /// different name. A restricted rule is not proof that LAN Clients can reach ShaPrint.
    pub(super) fn inbound_sharing_allowed() -> Result<bool, SetupFailure> {
        let checks = inbound_rules()
            .iter()
            .map(|rule| {
                format!(
                    "if (-not (AccessForPort '{protocol}' '{port}')) {{ exit 2 }};",
                    protocol = rule.protocol,
                    port = rule.port
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        // Inspect the active policy rather than netsh's localized text. A blocking rule takes
        // precedence over any allowance; requiring unrestricted address/program/interface scope
        // prevents an unrelated app's or loopback-only rule from passing as Client access.
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
function PortMatches($entry, $protocol, $port) {{
  foreach ($filter in $entry.Ports) {{
    if (($filter.Protocol.ToString() -eq $protocol -or $filter.Protocol.ToString() -eq 'Any') -and
        (@($filter.LocalPort) -contains $port -or @($filter.LocalPort) -contains 'Any') -and
        (@($filter.RemotePort) -contains 'Any')) {{ return $true }}
  }}
  return $false
}}
function RuleProfiles($rule) {{
  return @($rule.Profile.ToString().Split(',') | ForEach-Object {{ $_.Trim() }})
}}
function OverlapsActiveProfile($rule) {{
  $profiles = @(RuleProfiles $rule)
  if ($profiles -contains 'Any') {{ return $true }}
  foreach ($profile in $profiles) {{ if ($needed.ContainsKey($profile) -and $needed[$profile]) {{ return $true }} }}
  return $false
}}
function Unrestricted($rule) {{
  $address = Get-NetFirewallAddressFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop
  $application = Get-NetFirewallApplicationFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop
  $service = Get-NetFirewallServiceFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop
  $interface = Get-NetFirewallInterfaceFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop
  $type = Get-NetFirewallInterfaceTypeFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop
  $security = Get-NetFirewallSecurityFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop
  return ((@($address.LocalAddress) -contains 'Any') -and (@($address.RemoteAddress) -contains 'Any') -and
          $application.Program -eq 'Any' -and (-not $application.Package -or $application.Package -eq 'Any') -and
          $service.Service -eq 'Any' -and (@($interface.InterfaceAlias) -contains 'Any') -and
          $type.InterfaceType -eq 'Any' -and $security.Authentication -eq 'NotRequired')
}}
function BlocksClients($rule) {{
  if (-not (OverlapsActiveProfile $rule)) {{ return $false }}
  $application = Get-NetFirewallApplicationFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop
  if ($application.Program -ne 'Any' -and
      [Environment]::ExpandEnvironmentVariables($application.Program) -ine $env:SHAPRINT_FIREWALL_EXE) {{ return $false }}
  if ($application.Package -and $application.Package -ne 'Any') {{ return $false }}
  $service = Get-NetFirewallServiceFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop
  if ($service.Service -ne 'Any') {{ return $false }}
  $address = Get-NetFirewallAddressFilter -AssociatedNetFirewallRule $rule -ErrorAction Stop
  if (@($address.RemoteAddress) -contains '127.0.0.1' -or @($address.RemoteAddress) -contains '::1') {{
    if (@($address.RemoteAddress) -notcontains 'Any' -and @($address.RemoteAddress).Count -eq 1) {{ return $false }}
  }}
  return $true
}}
function AccessForPort($protocol, $port) {{
  if (-not ($needed.Domain -or $needed.Private -or $needed.Public)) {{ return $true }}
  $covered = @{{ Domain = $defaults.Domain; Private = $defaults.Private; Public = $defaults.Public }}
  foreach ($entry in $rules) {{
    $rule = $entry.Rule
    if ($rule.Action -eq 'Block' -and (PortMatches $entry $protocol $port) -and (BlocksClients $rule)) {{ return $false }}
  }}
  foreach ($entry in $rules) {{
    $rule = $entry.Rule
    if ($rule.Action -ne 'Allow' -or -not (PortMatches $entry $protocol $port)) {{ continue }}
    if (-not (Unrestricted $rule)) {{ continue }}
    $profiles = @(RuleProfiles $rule)
    if ($profiles -contains 'Any') {{ return $true }}
    foreach ($profile in $profiles) {{ if ($covered.ContainsKey($profile)) {{ $covered[$profile] = $true }} }}
  }}
  return ((-not $needed.Domain -or $covered.Domain) -and
          (-not $needed.Private -or $covered.Private) -and
          (-not $needed.Public -or $covered.Public))
}}
try {{
  $needed = @{{ Domain = $false; Private = $false; Public = $false }}
  $defaults = @{{ Domain = $false; Private = $false; Public = $false }}
  foreach ($profile in @(Get-NetFirewallProfile -PolicyStore ActiveStore -ErrorAction Stop)) {{
    if ($profile.Enabled -eq 'True') {{
      if ($profile.AllowInboundRules -eq 'False') {{ exit 2 }}
      $needed[$profile.Name] = $true
      $defaults[$profile.Name] = ($profile.DefaultInboundAction -eq 'Allow')
    }}
  }}
  $rules = @(Get-NetFirewallRule -PolicyStore ActiveStore -ErrorAction Stop |
    Where-Object {{ $_.Enabled -eq 'True' -and $_.Direction -eq 'Inbound' -and
                    ($_.Action -eq 'Allow' -or $_.Action -eq 'Block') }} |
    ForEach-Object {{ [pscustomobject]@{{ Rule = $_; Ports = @(Get-NetFirewallPortFilter -AssociatedNetFirewallRule $_ -ErrorAction Stop) }} }})
  {checks}
  exit 0
}} catch {{ exit 3 }}
"#
        );
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let executable = std::env::current_exe().map_err(|error| {
            SetupFailure::new(
                SetupFailureKind::Other,
                format!("cannot locate ShaPrint: {error}"),
            )
        })?;
        let mut child = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .env("SHAPRINT_FIREWALL_EXE", executable)
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| {
                SetupFailure::new(
                    SetupFailureKind::Other,
                    format!("cannot check Windows firewall: {error}"),
                )
            })?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return match status.code() {
                        Some(0) => Ok(true),
                        Some(2) => Ok(false),
                        _ => Err(SetupFailure::new(
                            SetupFailureKind::Other,
                            "cannot read Windows firewall rules",
                        )),
                    }
                }
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SetupFailure::new(
                        SetupFailureKind::TimedOut,
                        "Windows firewall access check timed out",
                    ));
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(SetupFailure::new(
                        SetupFailureKind::Other,
                        format!("cannot wait for Windows firewall check: {error}"),
                    ));
                }
            }
        }
    }

    /// Lets clients reach this server through the Windows firewall.
    ///
    /// Two things have to get in: the IPPS requests a client sends to the endpoint's port, and the
    /// discovery queries a client sends to the ports a responder may have taken (ADR 0004).
    ///
    /// Every rule is removed first: re-running setup must repair a rule that changed, and `netsh`
    /// refuses to create a duplicate name.
    fn allow_inbound_sharing() -> Result<(), SetupFailure> {
        let rules = inbound_rules();
        remove_rule("ShaPrint (8631)");
        remove_rule("ShaPrint discovery (5353)");
        remove_rule("ShaPrint discovery (5354)");
        for rule in &rules {
            remove_rule(&rule.name);
        }

        for rule in &rules {
            let args = netsh_add_rule_args(rule);
            let arg_slices: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            run("netsh", &arg_slices)?;
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

    fn run(program: &str, arguments: &[&str]) -> Result<(), SetupFailure> {
        // The parent is a GUI process with no console: without this flag Windows would flash a
        // console window for every configuration command.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let output = std::process::Command::new(program)
            .args(arguments)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| {
                SetupFailure::new(
                    SetupFailureKind::Unsupported,
                    format!("cannot run {program}: {error}"),
                )
            })?;
        if output.status.success() {
            return Ok(());
        }
        Err(SetupFailure::new(
            SetupFailureKind::Other,
            format!(
                "{program} exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ))
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

/// Turns the helper's exit code back into the reason the user is told about.
#[cfg(any(windows, test))]
fn classify_exit_code(code: i32) -> SetupFailureKind {
    SetupFailureKind::from_exit_code(code).unwrap_or(SetupFailureKind::Other)
}

/// The failure to report when the helper overstayed its deadline.
///
/// Ending an elevated process from an unelevated one is not guaranteed, so the message says which
/// of the two happened: a helper that could not be stopped may still finish the install it was
/// asked for, and the user should not be told otherwise.
#[cfg(any(windows, test))]
fn timed_out_failure(terminated: bool) -> SetupFailure {
    if terminated {
        SetupFailure::new(
            SetupFailureKind::TimedOut,
            "the setup helper was still running after its deadline and was terminated",
        )
    } else {
        SetupFailure::new(
            SetupFailureKind::TimedOut,
            "the setup helper was still running after its deadline and could not be terminated; it may still be running",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;

    fn queue_request(printer: &str) -> ClientQueueRequest {
        ClientQueueRequest::new(
            "10.0.0.5:8631",
            PrinterName::parse(printer).expect("valid printer name"),
        )
        .expect("valid request")
    }

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

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
            CommandLine::Setup(HelperRequest::Action(SetupAction::AllowInboundSharing))
        );
    }

    #[test]
    fn an_unusable_helper_request_is_reported() {
        assert_eq!(
            parse_command_line(&["shaprint.exe", SETUP_FLAG]),
            CommandLine::Invalid(SETUP_FLAG.to_owned())
        );
        assert_eq!(
            parse_command_line(&["shaprint.exe", SETUP_FLAG, "install-scanner"]),
            CommandLine::Invalid("install-scanner".to_owned())
        );
    }

    #[test]
    fn an_install_request_needs_both_the_server_and_the_printer() {
        assert_eq!(
            parse_command_line(&["shaprint.exe", SETUP_FLAG, "install-printer"]),
            CommandLine::Invalid("install-printer".to_owned())
        );
        assert_eq!(
            parse_command_line(&arguments(&[
                "shaprint.exe",
                SETUP_FLAG,
                "install-printer",
                SERVER_OPTION,
                "10.0.0.5:8631",
            ])),
            CommandLine::Invalid("install-printer".to_owned())
        );
    }

    #[test]
    fn the_helper_installs_only_what_it_can_validate() {
        assert_eq!(
            parse_command_line(&arguments(&[
                "shaprint.exe",
                SETUP_FLAG,
                "install-printer",
                SERVER_OPTION,
                "http://10.0.0.5",
                PRINTER_OPTION,
                "Office Printer",
            ])),
            CommandLine::Invalid(
                "the server address is not a host or host:port; review the server connection again"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_helper_request_round_trips_through_its_parameters() {
        let request = queue_request("Office Printer");
        let encoded = helper_parameters(&HelperRequest::InstallClientQueue(request.clone()));
        let decoded = parse_command_line(&arguments(&[
            "shaprint.exe",
            SETUP_FLAG,
            "install-printer",
            SERVER_OPTION,
            request.server_address(),
            PRINTER_OPTION,
            request.printer().as_str(),
        ]));
        assert_eq!(
            decoded,
            CommandLine::Setup(HelperRequest::InstallClientQueue(request))
        );
        assert!(encoded.contains(SERVER_OPTION));
        assert!(encoded.contains(PRINTER_OPTION));
    }

    #[test]
    fn a_duplicate_or_unknown_option_is_refused() {
        for extra in [
            vec![
                "shaprint.exe",
                SETUP_FLAG,
                "install-printer",
                SERVER_OPTION,
                "10.0.0.5:8631",
                SERVER_OPTION,
                "10.0.0.6:8631",
                PRINTER_OPTION,
                "Office Printer",
            ],
            vec![
                "shaprint.exe",
                SETUP_FLAG,
                "install-printer",
                "--serer",
                "10.0.0.5:8631",
                PRINTER_OPTION,
                "Office Printer",
            ],
            vec![
                "shaprint.exe",
                SETUP_FLAG,
                "install-printer",
                SERVER_OPTION,
                "10.0.0.5:8631",
                PRINTER_OPTION,
                "Office Printer",
                "junk",
            ],
            vec![
                "shaprint.exe",
                SETUP_FLAG,
                "install-printer",
                SERVER_OPTION,
                "10.0.0.5:8631",
                PRINTER_OPTION,
            ],
        ] {
            assert!(
                matches!(
                    parse_command_line(&arguments(&extra)),
                    CommandLine::Invalid(_)
                ),
                "accepted {extra:?}"
            );
        }
    }

    #[test]
    fn arguments_are_quoted_the_way_the_windows_parser_expects() {
        assert_eq!(quote_argument("Office Printer"), "\"Office Printer\"");
        assert_eq!(quote_argument("plain"), "\"plain\"");
        assert_eq!(quote_argument("say \"hi\""), "\"say \\\"hi\\\"\"");
        // Trailing backslashes are doubled so they cannot escape the closing quote.
        assert_eq!(quote_argument("back\\slash\\"), "\"back\\slash\\\\\"");
        assert_eq!(quote_argument("a\\\"b"), "\"a\\\\\\\"b\"");
    }

    #[test]
    fn the_helper_reports_every_classified_exit_code_and_defaults_the_rest() {
        for kind in SetupFailureKind::ALL {
            assert_eq!(classify_exit_code(kind.exit_code()), kind);
        }
        // A crash or an unclassified code must still produce advice.
        assert_eq!(classify_exit_code(2), SetupFailureKind::Other);
        assert_eq!(classify_exit_code(-1073741819), SetupFailureKind::Other);
    }

    #[test]
    fn a_helper_that_could_not_be_stopped_says_so() {
        // Ending an elevated process from an unelevated one can fail; the message must not claim a
        // termination that did not happen.
        let terminated = timed_out_failure(true);
        assert_eq!(terminated.kind(), SetupFailureKind::TimedOut);
        assert!(terminated.detail().contains("and was terminated"));
        assert!(!terminated.detail().contains("could not be terminated"));

        let survived = timed_out_failure(false);
        assert_eq!(survived.kind(), SetupFailureKind::TimedOut);
        assert!(survived.detail().contains("could not be terminated"));
        assert!(survived.detail().contains("may still be running"));
        assert_ne!(terminated.detail(), survived.detail());
    }

    #[test]
    fn the_helper_refuses_routine_actions() {
        let failure =
            run_elevated(HelperRequest::Action(SetupAction::StartSharing)).expect_err("refused");
        assert_eq!(failure.kind(), SetupFailureKind::InvalidRequest);

        let failure = SystemElevation::new()
            .elevate(SetupAction::StopSharing)
            .expect_err("refused");
        assert_eq!(failure.kind(), SetupFailureKind::InvalidRequest);
    }

    #[test]
    fn the_helper_refuses_a_queue_install_without_its_printer() {
        let failure = SystemElevation::new()
            .elevate(SetupAction::InstallPrinter)
            .expect_err("refused");
        assert_eq!(failure.kind(), SetupFailureKind::InvalidRequest);
        assert!(failure.detail().contains("queue installation"));
    }

    #[cfg(not(windows))]
    #[test]
    fn a_platform_that_cannot_elevate_reports_unsupported() {
        let failure = SystemElevation::new()
            .elevate(SetupAction::AllowInboundSharing)
            .expect_err("unsupported");
        assert_eq!(failure.kind(), SetupFailureKind::Unsupported);

        let failure = SystemElevation::new()
            .install_queue(&queue_request("Office Printer"))
            .expect_err("unsupported");
        assert_eq!(failure.kind(), SetupFailureKind::Unsupported);

        let failure = run_elevated(HelperRequest::InstallClientQueue(queue_request("Office")))
            .expect_err("unsupported");
        assert_eq!(failure.kind(), SetupFailureKind::Unsupported);
    }

    #[test]
    fn a_rejected_request_reports_the_stable_error_code() {
        let failure = SetupFailure::new(SetupFailureKind::InvalidRequest, "refused");
        assert_eq!(failure.kind().error_code(), ErrorCode::InvalidInput);
    }

    #[test]
    fn inbound_firewall_rules_cover_dedicated_ports_and_unrestricted_remote_address() {
        let rules = inbound_rules();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].protocol, "TCP");
        assert_eq!(rules[0].port, 48631);
        assert_eq!(rules[0].name, "ShaPrint (48631)");

        assert_eq!(rules[1].protocol, "UDP");
        assert_eq!(rules[1].port, 48633);
        assert_eq!(rules[1].name, "ShaPrint discovery (48633)");

        for rule in &rules {
            let args = netsh_add_rule_args(rule);
            assert!(args.contains(&"remoteip=any".to_owned()));
            assert!(args.contains(&"profile=any".to_owned()));
            assert!(args.contains(&"dir=in".to_owned()));
            assert!(args.contains(&"action=allow".to_owned()));
        }
    }

    #[test]
    #[cfg(windows)]
    fn is_elevated_runs_without_panicking() {
        let _ = windows::is_elevated();
    }
}

