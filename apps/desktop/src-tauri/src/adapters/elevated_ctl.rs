//! Elevated `shaprintctl` execution for desktop machine mutations through polkit (`pkexec`).
//!
//! The desktop GUI never runs as root and never talks to machine state directly. Reads go over
//! the daemon's Unix socket unprivileged; every mutation is executed by running the *same*
//! `shaprintctl` CLI the headless operator uses, wrapped in `pkexec` (ADR 0018: "mutations
//! require polkit from the desktop or `sudo` from the CLI"). The polkit `.policy` actions in
//! `packaging/linux/polkit` pin `org.freedesktop.policykit.exec.path` to `shaprintctl` and use
//! the `argv1` annotation so each command word (`start`, `stop`, `network-channel`, `trust`,
//! `queue`, `sharing`) authorizes under its own narrow action.
//! Before execution, `pkcheck` authorizes the temporary setup meta action for this exact
//! GUI process. Its implications cover those six actions for polkit's cache lifetime;
//! reads never authorize, and purge is excluded. No password or authorization token is stored.
//!
//! Contract with the caller:
//! * the Network Channel secret travels on the helper's standard input (`--stdin`), never in
//!   arguments, environment, or logs;
//! * `shaprintctl` reports failures as `code=<stable> message=<text>` on stderr, which is parsed
//!   back into the same `AppError` codes the GUI and CLI share;
//! * polkit exit `126` (authentication dialog dismissed) and `127` (not authorized / no agent)
//!   become `not-authorized` errors that name the required action and state that no change was
//!   made, so the UI can show how to retry; any other unparseable exit becomes `internal` and
//!   makes no claim about whether the mutation happened.
//!
//! Troubleshooting — a polkit *authentication agent* must be running in the desktop session for
//! the authorization dialog to appear at all. Full GNOME and KDE sessions ship one; minimal
//! window-manager sessions do not, and there `pkexec` fails fast with exit `127` (the GUI then
//! reports `not-authorized` naming the action, with no state changed). On such sessions install
//! and start an agent, e.g. `polkit-gnome-authentication-agent-1` (package `polkit-gnome`) or
//! `lxpolkit`, and retry.

use std::path::{Path, PathBuf};

/// Default absolute path of the packaged `shaprintctl` helper on Linux.
#[cfg(unix)]
pub const DEFAULT_CTL_PATH: &str = "/usr/bin/shaprintctl";

/// Environment override for the polkit helper program (tests point this at a fake).
#[cfg(unix)]
pub const PKEXEC_ENV: &str = "SHAPRINT_PKEXEC";

/// Environment override for the `shaprintctl` program (tests point this at a fake).
#[cfg(unix)]
pub const CTL_ENV: &str = "SHAPRINT_CTL";

/// The narrow polkit action a desktop mutation is authorized under.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    Start,
    Stop,
    NetworkChannel,
    Trust,
    Queue,
    Sharing,
}

#[cfg(unix)]
impl ControlAction {
    /// The polkit action id installed by `packaging/linux/polkit/org.shaprint.control.policy`.
    pub const fn action_id(self) -> &'static str {
        match self {
            Self::Start => "org.shaprint.control.start",
            Self::Stop => "org.shaprint.control.stop",
            Self::NetworkChannel => "org.shaprint.control.network-channel",
            Self::Trust => "org.shaprint.control.trust",
            Self::Queue => "org.shaprint.control.queue",
            Self::Sharing => "org.shaprint.control.sharing",
        }
    }

    /// What the user is authenticating for, in the retry message the GUI shows.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Start => "start ShaPrint print services",
            Self::Stop => "stop ShaPrint print services",
            Self::NetworkChannel => "change the Network Channel",
            Self::Trust => "change Trusted Servers",
            Self::Queue => "install or repair a Client queue",
            Self::Sharing => "change the shared printers",
        }
    }

    /// Every bounded desktop mutation action, in a stable order. Setup and purge are separate.
    pub const ALL: [ControlAction; 6] = [
        Self::Start,
        Self::Stop,
        Self::NetworkChannel,
        Self::Trust,
        Self::Queue,
        Self::Sharing,
    ];
}

/// One machine mutation the desktop requests through the elevated CLI helper.
///
/// Deliberately `Debug`-less: [`Self::SetNetworkChannel`] carries the secret, and no derived
/// implementation can accidentally print it into a log line.
#[cfg(unix)]
pub enum ElevatedMutation {
    StartService(String),
    StopService(String),
    /// Stops every supervised service and the daemon itself (`shaprintctl stop`).
    StopDaemon,
    /// Replaces the machine Network Channel; the value is written to the helper's stdin.
    SetNetworkChannel(String),
    ApproveServer {
        address: String,
        fingerprint: String,
    },
    ForgetServer(String),
    InstallClientQueue {
        address: String,
        printer: String,
    },
    RepairClientQueue {
        address: String,
        printer: String,
    },
    SetSharedPrinters(Vec<String>),
}

/// What the elevated helper printed on stdout for a successful mutation.
#[cfg(unix)]
#[derive(Debug)]
pub enum ElevatedOutcome {
    Status(crate::ipc::dto::RuntimeStatusDto),
    NetworkChannel(crate::adapters::daemon::NetworkChannelStatusDto),
    Review(crate::adapters::daemon::ServerTrustReviewDto),
    Queue(crate::ipc::dto::ClientQueueDto),
    LocalPrinters(Vec<crate::ipc::dto::LocalPrinterDto>),
    /// The mutation succeeded and its output does not need to be read back.
    Acknowledged,
}

#[cfg(unix)]
impl ElevatedMutation {
    /// The narrow polkit action this mutation authorizes under.
    pub const fn action(&self) -> ControlAction {
        match self {
            Self::StartService(_) => ControlAction::Start,
            Self::StopService(_) | Self::StopDaemon => ControlAction::Stop,
            Self::SetNetworkChannel(_) => ControlAction::NetworkChannel,
            Self::ApproveServer { .. } | Self::ForgetServer(_) => ControlAction::Trust,
            Self::InstallClientQueue { .. } | Self::RepairClientQueue { .. } => {
                ControlAction::Queue
            }
            Self::SetSharedPrinters(_) => ControlAction::Sharing,
        }
    }

    /// The Network Channel secret, sent on stdin — never as an argument.
    fn secret(&self) -> Option<&str> {
        match self {
            Self::SetNetworkChannel(channel) => Some(channel),
            _ => None,
        }
    }

    /// The `shaprintctl` arguments. The first argument is always the command word the polkit
    /// `argv1` annotation matches against.
    fn arguments(&self, socket_path: &Path) -> Vec<String> {
        let socket = socket_path.display().to_string();
        let mut args: Vec<String> = match self {
            Self::StartService(id) => vec!["start".to_owned(), id.clone()],
            Self::StopService(id) => vec!["stop".to_owned(), id.clone()],
            Self::StopDaemon => vec!["stop".to_owned()],
            Self::SetNetworkChannel(_) => vec![
                "network-channel".to_owned(),
                "set".to_owned(),
                "--stdin".to_owned(),
            ],
            Self::ApproveServer {
                address,
                fingerprint,
            } => vec![
                "trust".to_owned(),
                "approve".to_owned(),
                address.clone(),
                fingerprint.clone(),
            ],
            Self::ForgetServer(address) => {
                vec!["trust".to_owned(), "forget".to_owned(), address.clone()]
            }
            Self::InstallClientQueue { address, printer } => vec![
                "queue".to_owned(),
                "install".to_owned(),
                address.clone(),
                printer.clone(),
            ],
            Self::RepairClientQueue { address, printer } => vec![
                "queue".to_owned(),
                "repair".to_owned(),
                address.clone(),
                printer.clone(),
            ],
            Self::SetSharedPrinters(printers) => {
                if printers.is_empty() {
                    vec!["sharing".to_owned(), "clear".to_owned()]
                } else {
                    let mut args = vec!["sharing".to_owned(), "set".to_owned()];
                    args.extend(printers.iter().cloned());
                    args
                }
            }
        };
        args.push("--json".to_owned());
        args.push("--socket".to_owned());
        args.push(socket);
        args
    }

    /// The stdout payload expected for a successful run.
    const fn expected_output(&self) -> ExpectedOutput {
        match self {
            Self::StartService(_) | Self::StopService(_) | Self::StopDaemon => {
                ExpectedOutput::Status
            }
            Self::SetNetworkChannel(_) => ExpectedOutput::NetworkChannel,
            Self::ApproveServer { .. } => ExpectedOutput::Review,
            Self::ForgetServer(_) => ExpectedOutput::Acknowledged,
            Self::InstallClientQueue { .. } | Self::RepairClientQueue { .. } => {
                ExpectedOutput::Queue
            }
            Self::SetSharedPrinters(_) => ExpectedOutput::LocalPrinters,
        }
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedOutput {
    Status,
    NetworkChannel,
    Review,
    Queue,
    LocalPrinters,
    Acknowledged,
}

/// Runs machine mutations as `pkexec shaprintctl`, the desktop counterpart of `sudo shaprintctl`.
#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct ElevatedCtl {
    pkexec: PathBuf,
    pkcheck: PathBuf,
    program: PathBuf,
    socket_path: PathBuf,
    authorization: std::sync::Arc<tokio::sync::Mutex<()>>,
}

#[cfg(unix)]
pub const SETUP_ACTION: &str = "org.shaprint.control.setup";

#[cfg(unix)]
impl ElevatedCtl {
    /// Builds the runner for `socket_path`, honoring the `SHAPRINT_PKEXEC` and `SHAPRINT_CTL`
    /// environment overrides and otherwise using the packaged paths.
    ///
    /// The helper defaults to `DEFAULT_CTL_PATH` and not to "whatever sits next to this
    /// executable": only that path carries the `org.shaprint.control.*` polkit annotations, so
    /// any other path would authorize under pkexec's generic action while the GUI quotes a
    /// narrow one. Unpackaged layouts set `SHAPRINT_CTL` explicitly.
    pub fn system(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            pkexec: option_env_path(PKEXEC_ENV).unwrap_or_else(|| PathBuf::from("pkexec")),
            pkcheck: PathBuf::from("/usr/bin/pkcheck"),
            program: option_env_path(CTL_ENV).unwrap_or_else(|| PathBuf::from(DEFAULT_CTL_PATH)),
            socket_path: socket_path.into(),
            authorization: Default::default(),
        }
    }

    /// Builds the runner with explicit program paths (tests).
    pub fn new(
        pkexec: impl Into<PathBuf>,
        program: impl Into<PathBuf>,
        socket_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            pkexec: pkexec.into(),
            pkcheck: PathBuf::from("/usr/bin/pkcheck"),
            program: program.into(),
            socket_path: socket_path.into(),
            authorization: Default::default(),
        }
    }

    /// Supplies the authorization checker for isolated control-plane tests.
    pub fn with_authorization_program(mut self, program: impl Into<PathBuf>) -> Self {
        self.pkcheck = program.into();
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Runs `mutation` through the polkit-wrapped helper and returns the parsed result.
    ///
    /// Declined or failed authorization returns `not-authorized` *before* any daemon state is
    /// touched, because the helper only reaches the daemon once polkit has authorized it.
    pub async fn run(
        &self,
        mutation: ElevatedMutation,
    ) -> Result<ElevatedOutcome, crate::domain::AppError> {
        use crate::domain::AppError;

        if !self.program.is_file() {
            return Err(AppError::unsupported(format!(
                "the ShaPrint control helper '{}' is not installed; install the ShaPrint package and retry",
                self.program.display()
            )));
        }

        let pkexec = self.pkexec.clone();
        let pkcheck = self.pkcheck.clone();
        let program = self.program.clone();
        let args = mutation.arguments(&self.socket_path);
        let action = mutation.action();
        let secret = mutation.secret().map(ToOwned::to_owned);
        let expected = mutation.expected_output();

        // Clones share the gate: concurrent requests cannot stack authentication dialogs.
        // Recheck with polkit every time so expiration/revocation is never bypassed locally.
        let authorization = self.authorization.clone().lock_owned().await;

        tokio::task::spawn_blocking(move || {
            let _authorization = authorization;
            authorize_setup(&pkcheck)?;
            execute_elevated(
                &pkexec,
                &program,
                &args,
                secret.as_deref(),
                action,
                expected,
            )
        })
        .await
        .map_err(|error| {
            AppError::internal(format!("the elevated control task stopped: {error}"))
        })?
    }
}

#[cfg(unix)]
fn authorize_setup(pkcheck: &Path) -> Result<(), crate::domain::AppError> {
    use crate::domain::AppError;
    use std::process::{Command, Stdio};

    let stat = std::fs::read_to_string("/proc/self/stat")
        .map_err(|_| AppError::internal("could not identify the setup authorization process"))?;
    let start = stat
        .rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| AppError::internal("could not identify the setup authorization process"))?;
    let uid = crate::adapters::daemon::current_effective_uid()
        .ok_or_else(|| AppError::internal("could not identify the setup authorization user"))?;
    let subject = format!("{},{start},{uid}", std::process::id());
    let output = Command::new(pkcheck)
        .args([
            "--action-id",
            SETUP_ACTION,
            "--process",
            &subject,
            "--allow-user-interaction",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|_| {
            AppError::unsupported(
                "the polkit authorization checker is unavailable; install polkit and retry",
            )
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::not_authorized(
            "ShaPrint setup was not authorized. No changes were made. Authorize setup to change printing settings; opening the app and refreshing status need no administrator password.",
        ))
    }
}

#[cfg(unix)]
fn option_env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(unix)]
fn execute_elevated(
    pkexec: &Path,
    program: &Path,
    args: &[String],
    secret: Option<&str>,
    action: ControlAction,
    expected: ExpectedOutput,
) -> Result<ElevatedOutcome, crate::domain::AppError> {
    use crate::domain::AppError;
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(pkexec)
        .arg(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AppError::unsupported(format!(
                "cannot start the polkit authentication helper '{}': {error}",
                pkexec.display()
            ))
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        // The secret reaches the helper on standard input, exactly like `shaprintctl --stdin`,
        // so it never appears in process arguments, the environment, or logs. Dropping the
        // pipe always closes it, so the helper never waits for input that will not come.
        if let Some(secret) = secret {
            let _ = stdin.write_all(secret.as_bytes());
            let _ = stdin.write_all(b"\n");
        }
    }

    let output = child.wait_with_output().map_err(|error| {
        AppError::internal(format!("the elevated helper did not finish: {error}"))
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        return parse_elevated_stdout(&stdout, expected);
    }

    if let Some(error) = parse_cli_failure(&stderr) {
        return Err(error);
    }
    Err(authorization_error(action, output.status.code()))
}

/// Parses a `shaprintctl` failure line (`code=<stable> message=<text>`) back into an `AppError`
/// so GUI and CLI surface the identical stable error code and message.
#[cfg(unix)]
fn parse_cli_failure(stderr: &str) -> Option<crate::domain::AppError> {
    use crate::domain::{AppError, ErrorCode};

    for line in stderr.lines() {
        let rest = line
            .strip_prefix("shaprintctl failed ")
            .or_else(|| line.strip_prefix("shaprintctl error "));
        let Some(rest) = rest else { continue };
        let Some(code_text) = rest.strip_prefix("code=") else {
            continue;
        };
        let Some((code, message)) = code_text.split_once(" message=") else {
            continue;
        };
        let code = ErrorCode::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == code)
            .unwrap_or(ErrorCode::Internal);
        return Some(AppError::new(code, message.to_owned()));
    }
    None
}

/// Maps a polkit refusal to the stable `not-authorized` error the GUI shows with the action
/// required to retry. A dismissed or refused prompt never ran the helper, so machine state is
/// unchanged and the message can say so.
///
/// Any other unparseable exit is an anomaly rather than a refusal: it becomes an `internal`
/// error that does *not* claim the mutation was skipped, because the helper may have failed
/// after the daemon already applied it.
#[cfg(unix)]
fn authorization_error(action: ControlAction, exit_code: Option<i32>) -> crate::domain::AppError {
    use crate::domain::AppError;

    match exit_code {
        Some(code @ (126 | 127)) => {
            let outcome = if code == 126 {
                "Authentication was dismissed."
            } else {
                "Authentication was not granted."
            };
            AppError::not_authorized(format!(
                "{outcome} Administrator authentication is required to {} (polkit action {}); \
                 approve the authentication prompt and retry. No changes were made.",
                action.description(),
                action.action_id()
            ))
        }
        other => {
            let detail = match other {
                Some(code) => format!("exit status {code}"),
                None => "no exit status".to_owned(),
            };
            AppError::internal(format!(
                "the privileged helper ended without reporting a result ({detail}); \
                 check the system journal, then retry"
            ))
        }
    }
}

#[cfg(unix)]
fn parse_elevated_stdout(
    stdout: &str,
    expected: ExpectedOutput,
) -> Result<ElevatedOutcome, crate::domain::AppError> {
    use crate::domain::AppError;
    use serde::de::DeserializeOwned;

    fn parse<T: DeserializeOwned>(stdout: &str, label: &str) -> Result<T, AppError> {
        serde_json::from_str(stdout.trim()).map_err(|error| {
            AppError::internal(format!("cannot read the {label} response: {error}"))
        })
    }

    match expected {
        ExpectedOutput::Status => Ok(ElevatedOutcome::Status(parse(stdout, "status")?)),
        ExpectedOutput::NetworkChannel => Ok(ElevatedOutcome::NetworkChannel(parse(
            stdout,
            "network channel",
        )?)),
        ExpectedOutput::Review => Ok(ElevatedOutcome::Review(parse(stdout, "server review")?)),
        ExpectedOutput::Queue => Ok(ElevatedOutcome::Queue(parse(stdout, "client queue")?)),
        ExpectedOutput::LocalPrinters => Ok(ElevatedOutcome::LocalPrinters(parse(
            stdout,
            "printer list",
        )?)),
        ExpectedOutput::Acknowledged => Ok(ElevatedOutcome::Acknowledged),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::domain::ErrorCode;

    fn socket() -> PathBuf {
        PathBuf::from("/run/shaprint/shaprintd.sock")
    }

    fn arguments(mutation: &ElevatedMutation) -> Vec<String> {
        mutation.arguments(&socket())
    }

    #[test]
    fn every_mutation_authorizes_under_its_own_narrow_action() {
        assert_eq!(
            ElevatedMutation::StartService("client-proxy".into()).action(),
            ControlAction::Start
        );
        assert_eq!(
            ElevatedMutation::StopService("server-sharing".into()).action(),
            ControlAction::Stop
        );
        assert_eq!(ElevatedMutation::StopDaemon.action(), ControlAction::Stop);
        assert_eq!(
            ElevatedMutation::SetNetworkChannel("secret".into()).action(),
            ControlAction::NetworkChannel
        );
        assert_eq!(
            ElevatedMutation::ApproveServer {
                address: "a".into(),
                fingerprint: "f".into()
            }
            .action(),
            ControlAction::Trust
        );
        assert_eq!(
            ElevatedMutation::ForgetServer("a".into()).action(),
            ControlAction::Trust
        );
        assert_eq!(
            ElevatedMutation::InstallClientQueue {
                address: "a".into(),
                printer: "p".into()
            }
            .action(),
            ControlAction::Queue
        );
        assert_eq!(
            ElevatedMutation::SetSharedPrinters(vec![]).action(),
            ControlAction::Sharing
        );

        let mut ids: Vec<&str> = ControlAction::ALL.iter().map(|a| a.action_id()).collect();
        ids.dedup();
        assert_eq!(ids.len(), ControlAction::ALL.len());
        for action in ControlAction::ALL {
            assert!(action.action_id().starts_with("org.shaprint.control."));
        }
    }

    #[test]
    fn arguments_start_with_the_polkit_argv1_command_word_and_target_the_socket() {
        let cases: Vec<(ElevatedMutation, Vec<&str>)> = vec![
            (
                ElevatedMutation::StartService("client-proxy".into()),
                vec!["start", "client-proxy"],
            ),
            (
                ElevatedMutation::StopService("server-sharing".into()),
                vec!["stop", "server-sharing"],
            ),
            (ElevatedMutation::StopDaemon, vec!["stop"]),
            (
                ElevatedMutation::SetNetworkChannel("secret".into()),
                vec!["network-channel", "set", "--stdin"],
            ),
            (
                ElevatedMutation::ApproveServer {
                    address: "10.0.0.5:48631".into(),
                    fingerprint: "AA:BB".into(),
                },
                vec!["trust", "approve", "10.0.0.5:48631", "AA:BB"],
            ),
            (
                ElevatedMutation::ForgetServer("10.0.0.5:48631".into()),
                vec!["trust", "forget", "10.0.0.5:48631"],
            ),
            (
                ElevatedMutation::InstallClientQueue {
                    address: "10.0.0.5:48631".into(),
                    printer: "Office Printer".into(),
                },
                vec!["queue", "install", "10.0.0.5:48631", "Office Printer"],
            ),
            (
                ElevatedMutation::RepairClientQueue {
                    address: "10.0.0.5:48631".into(),
                    printer: "Office Printer".into(),
                },
                vec!["queue", "repair", "10.0.0.5:48631", "Office Printer"],
            ),
            (
                ElevatedMutation::SetSharedPrinters(vec!["Laser".into()]),
                vec!["sharing", "set", "Laser"],
            ),
            (
                ElevatedMutation::SetSharedPrinters(vec![]),
                vec!["sharing", "clear"],
            ),
        ];

        for (mutation, prefix) in cases {
            let args = arguments(&mutation);
            let prefix: Vec<String> = prefix.into_iter().map(ToOwned::to_owned).collect();
            assert_eq!(
                &args[..prefix.len()],
                prefix.as_slice(),
                "argv1 must be the polkit command word for the action"
            );
            assert_eq!(
                &args[args.len() - 3..],
                &["--json", "--socket", "/run/shaprint/shaprintd.sock"]
            );
        }
    }

    #[test]
    fn the_network_channel_secret_never_appears_in_arguments() {
        let mutation = ElevatedMutation::SetNetworkChannel("super-secret-value".to_owned());
        let args = arguments(&mutation);
        assert!(
            !args.iter().any(|arg| arg.contains("super-secret-value")),
            "secret leaked into arguments: {args:?}"
        );
        assert_eq!(mutation.secret(), Some("super-secret-value"));
        assert!(arguments(&ElevatedMutation::StopDaemon)
            .iter()
            .all(|arg| !arg.contains("super-secret-value")));
    }

    #[test]
    fn cli_failure_lines_round_trip_into_stable_errors() {
        let error = parse_cli_failure(
            "pkexec: Warning: something\nshaprintctl failed code=not-authorized message=admin changes require root privileges (run with sudo)\n",
        )
        .expect("parses the failure line");
        assert_eq!(error.code(), ErrorCode::NotAuthorized);
        assert!(error.message().contains("run with sudo"));

        let parse_error =
            parse_cli_failure("shaprintctl error code=invalid-input message=missing command\n")
                .expect("parses the parse error line");
        assert_eq!(parse_error.code(), ErrorCode::InvalidInput);

        assert!(parse_cli_failure("pkexec: No authentication agent is available.").is_none());
        assert!(parse_cli_failure("shaprintctl failed code=wat message=unknown").is_some());
    }

    #[test]
    fn declined_authentication_names_the_action_and_states_nothing_changed() {
        let dismissed = authorization_error(ControlAction::Trust, Some(126));
        assert_eq!(dismissed.code(), ErrorCode::NotAuthorized);
        assert!(dismissed.message().contains("org.shaprint.control.trust"));
        assert!(dismissed.message().contains("change Trusted Servers"));
        assert!(dismissed.message().contains("retry"));
        assert!(dismissed.message().contains("No changes were made"));

        let denied = authorization_error(ControlAction::Sharing, Some(127));
        assert_eq!(denied.code(), ErrorCode::NotAuthorized);
        assert!(denied.message().contains("org.shaprint.control.sharing"));
    }

    /// An exit that is neither a polkit refusal nor a `shaprintctl` failure line says nothing
    /// about the outcome, so it must not claim the machine is unchanged.
    #[test]
    fn an_unparseable_helper_failure_does_not_pretend_nothing_changed() {
        let crashed = authorization_error(ControlAction::Stop, Some(3));
        assert_eq!(crashed.code(), ErrorCode::Internal);
        assert!(!crashed.message().contains("No changes were made"));
        assert!(crashed.message().contains("exit status 3"));
        assert!(crashed.message().contains("journal"));

        let signaled = authorization_error(ControlAction::Stop, None);
        assert_eq!(signaled.code(), ErrorCode::Internal);
        assert!(!signaled.message().contains("No changes were made"));
    }

    /// Writes an executable shell script for the fake helper programs.
    fn script(directory: &Path, name: &str, body: &str) -> PathBuf {
        std::fs::create_dir_all(directory).expect("creates script dir");
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("writes script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("makes script executable");
        }
        path
    }

    fn workdir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("shaprint-elevated-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creates workdir");
        dir
    }

    #[tokio::test]
    async fn setup_authorization_is_reused_across_actions_and_denial_never_executes() {
        let dir = workdir("setup-session");
        let checker = script(
            &dir,
            "pkcheck",
            &format!(
                r#"test "$1" = --action-id && test "$2" = org.shaprint.control.setup || exit 2
test "$3" = --process && test "$5" = --allow-user-interaction || exit 2
test -f '{0}/denied' && exit 1
if [ ! -f '{0}/cached' ]; then
    echo prompt >> '{0}/prompts'
    touch '{0}/cached'
fi
exit 0"#,
                dir.display()
            ),
        );
        let pkexec = script(&dir, "pkexec", r#"exec "$@""#);
        let ctl = script(
            &dir,
            "shaprintctl",
            &format!(
                "echo applied >> '{}/applied'; if [ \"$1\" = network-channel ]; then echo '{{\"configured\":true}}'; else echo '[]'; fi",
                dir.display()
            ),
        );
        let runner = ElevatedCtl::new(pkexec, ctl, dir.join("unused.sock"))
            .with_authorization_program(checker);
        // Sharing and Network Channel require different narrow actions, but both are
        // checked against the same temporary setup authorization before execution.
        runner
            .run(ElevatedMutation::SetSharedPrinters(vec![]))
            .await
            .unwrap();
        runner
            .run(ElevatedMutation::SetNetworkChannel(String::new()))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("prompts"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("applied"))
                .unwrap()
                .lines()
                .count(),
            2
        );

        std::fs::remove_file(dir.join("cached")).unwrap();
        runner
            .run(ElevatedMutation::SetSharedPrinters(vec![]))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("prompts"))
                .unwrap()
                .lines()
                .count(),
            2
        );

        std::fs::write(dir.join("denied"), "").unwrap();
        let error = runner
            .run(ElevatedMutation::SetSharedPrinters(vec![]))
            .await
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::NotAuthorized);
        assert_eq!(
            std::fs::read_to_string(dir.join("applied"))
                .unwrap()
                .lines()
                .count(),
            3
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn successful_mutation_parses_the_cli_json_output() {
        let dir = workdir("success");
        // A fake `pkexec` that runs the program directly, like the real one does after polkit
        // authorizes it.
        let pkexec = script(&dir, "fake-pkexec", r#"exec "$@""#);
        let status = serde_json::json!({
            "services": [{"id": "client-proxy", "state": "running", "detail": "running"}],
            "network_channel_configured": null
        });
        let ctl = script(&dir, "shaprintctl", &format!("cat <<'EOF'\n{status}\nEOF"));
        let runner = ElevatedCtl::new(&pkexec, &ctl, dir.join("unused.sock"))
            .with_authorization_program(script(&dir, "pkcheck", "exit 0"));

        let outcome = runner
            .run(ElevatedMutation::StartService("client-proxy".into()))
            .await
            .expect("elevated start succeeds");
        match outcome {
            ElevatedOutcome::Status(status) => {
                assert_eq!(status.services.len(), 1);
                assert_eq!(status.services[0].id, "client-proxy");
                assert_eq!(status.services[0].state, "running");
            }
            other => panic!("expected status outcome, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn dismissed_authorization_fails_before_the_daemon_is_contacted() {
        let dir = workdir("declined");
        let pkexec = script(&dir, "fake-pkexec", "exit 126");
        // The program would report "touched" if it ever ran; it must not run.
        let ctl = script(&dir, "shaprintctl", "echo touched; exit 0");
        let runner = ElevatedCtl::new(&pkexec, &ctl, dir.join("unused.sock"))
            .with_authorization_program(script(&dir, "pkcheck", "exit 0"));

        let error = runner
            .run(ElevatedMutation::SetSharedPrinters(vec!["Laser".into()]))
            .await
            .expect_err("declined authentication must fail");
        assert_eq!(error.code(), ErrorCode::NotAuthorized);
        assert!(error.message().contains("org.shaprint.control.sharing"));
        assert!(error.message().contains("No changes were made"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn secret_travels_on_stdin_and_cli_errors_keep_their_stable_code() {
        let dir = workdir("stdin");
        let pkexec = script(&dir, "fake-pkexec", r#"exec "$@""#);
        // Echoes stdin length so the test proves the secret arrived on stdin, then behaves like
        // `shaprintctl` failing with a stable error.
        let ctl = script(
            &dir,
            "shaprintctl",
            "read line; echo \"shaprintctl failed code=not-authorized message=declined for ${#line} bytes\" >&2; exit 1",
        );
        let runner = ElevatedCtl::new(&pkexec, &ctl, dir.join("unused.sock"))
            .with_authorization_program(script(&dir, "pkcheck", "exit 0"));

        let error = runner
            .run(ElevatedMutation::SetNetworkChannel(
                "super-secret-value".to_owned(),
            ))
            .await
            .expect_err("helper fails");
        assert_eq!(error.code(), ErrorCode::NotAuthorized);
        // "super-secret-value" is 18 bytes: the value crossed on stdin, not in the arguments.
        assert!(error.message().contains("18 bytes"), "{}", error.message());
        assert!(!error.message().contains("super-secret-value"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_missing_control_helper_is_an_actionable_unsupported_error() {
        let dir = workdir("missing");
        let pkexec = script(&dir, "fake-pkexec", "exit 0");
        let runner = ElevatedCtl::new(&pkexec, dir.join("no-such-shaprintctl"), dir.join("s.sock"));

        let error = runner
            .run(ElevatedMutation::StopDaemon)
            .await
            .expect_err("missing helper must fail");
        assert_eq!(error.code(), ErrorCode::Unsupported);
        assert!(error.message().contains("not installed"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn temporary_setup_policy_grants_only_bounded_printing_actions() {
        let policy =
            include_str!("../../../../../packaging/linux/polkit/org.shaprint.control.policy");
        let setup = policy
            .split(&format!("<action id=\"{SETUP_ACTION}\">"))
            .nth(1)
            .unwrap()
            .split("</action>")
            .next()
            .unwrap();
        let implied = setup
            .split("<annotate key=\"org.freedesktop.policykit.imply\">")
            .nth(1)
            .unwrap()
            .split("</annotate>")
            .next()
            .unwrap();
        let mut actual: Vec<_> = implied.split_whitespace().collect();
        let mut expected: Vec<_> = ControlAction::ALL
            .iter()
            .map(|action| action.action_id())
            .collect();
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(
            actual, expected,
            "setup must not grant purge or arbitrary root execution"
        );
        assert!(setup.contains("<allow_active>auth_admin_keep</allow_active>"));
        assert!(setup.contains("<allow_inactive>no</allow_inactive>"));
        assert!(setup.contains("<allow_any>no</allow_any>"));
        assert!(!setup.contains("org.freedesktop.policykit.exec.path"));
    }
}
