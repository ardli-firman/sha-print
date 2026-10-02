//! Failed print attempts, as the UI reports them.
//!
//! A record holds a stable code, the path that failed, an optional validated printer queue name,
//! and when it happened. The message and the recovery action are derived from those and stored, so
//! a record can never carry the Network Channel, credentials, or document contents: there is no
//! field for them to travel in, and the only free text it can echo is a queue name the spooler or
//! a client already had to pass validation for (#39).

use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{sanitize_text, ErrorCode, PrinterName};

/// Longest message the shell publishes for one failure.
const MESSAGE_LIMIT: usize = 200;

/// Which print path reported the failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum JobPath {
    /// The local client proxy could not get a job from an installed queue to the server.
    ClientForwarding,
    /// The server could not submit a job it accepted to the local Windows queue.
    ServerSubmission,
}

impl JobPath {
    /// Every path, in a fixed order.
    pub const ALL: [JobPath; 2] = [JobPath::ClientForwarding, JobPath::ServerSubmission];

    /// The stable id used by IPC payloads and the UI.
    pub const fn as_str(self) -> &'static str {
        match self {
            JobPath::ClientForwarding => "client-forwarding",
            JobPath::ServerSubmission => "server-submission",
        }
    }
}

/// A print attempt that failed, and what the user can do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintFailure {
    path: JobPath,
    code: ErrorCode,
    printer: Option<PrinterName>,
    message: String,
    recovery: &'static str,
    observed_at_ms: u64,
}

impl PrintFailure {
    /// A job the local client proxy could not forward to the server.
    pub fn client(code: ErrorCode, printer: Option<&PrinterName>) -> Self {
        Self::new(JobPath::ClientForwarding, code, printer)
    }

    /// A job the server accepted but could not submit to its local queue.
    pub fn server(code: ErrorCode, printer: Option<&PrinterName>) -> Self {
        Self::new(JobPath::ServerSubmission, code, printer)
    }

    fn new(path: JobPath, code: ErrorCode, printer: Option<&PrinterName>) -> Self {
        let guidance = guidance(path, code);
        let message = sanitize_text(&describe(printer, guidance.problem), MESSAGE_LIMIT);
        Self {
            path,
            code,
            printer: printer.cloned(),
            message,
            recovery: guidance.recovery,
            observed_at_ms: now_ms(),
        }
    }

    pub fn path(&self) -> JobPath {
        self.path
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// The queue the job was for, when the failing path knew it before it gave up.
    pub fn printer(&self) -> Option<&PrinterName> {
        self.printer.as_ref()
    }

    /// Human-readable summary; never contains credentials or document contents.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The one action that resolves this failure.
    pub fn recovery(&self) -> &'static str {
        self.recovery
    }

    /// When the failure was observed, as milliseconds since the Unix epoch.
    pub fn observed_at_ms(&self) -> u64 {
        self.observed_at_ms
    }
}

/// What one failed job reads like: the problem, and the single action that resolves it.
struct Guidance {
    problem: &'static str,
    recovery: &'static str,
}

/// The problem and its fix for one code on one path.
///
/// Both come from the same arm, so a condition can never describe itself without also telling the
/// user what to do about it.
fn guidance(path: JobPath, code: ErrorCode) -> Guidance {
    match path {
        JobPath::ClientForwarding => match code {
            ErrorCode::ServerUnavailable => Guidance {
                problem: "could not reach the ShaPrint server.",
                recovery: "Check that ShaPrint is running on the server, that its address is reachable from this computer, and print again.",
            },
            ErrorCode::ServerNotTrusted => Guidance {
                problem: "was not sent because this server is not approved yet.",
                recovery: "Open Nearby servers or Server connections, review the fingerprint, and approve the server before printing.",
            },
            ErrorCode::ServerIdentityChanged => Guidance {
                problem: "was blocked because the server certificate changed.",
                recovery: "Review the server's current fingerprint and reapprove it only if you trust the change.",
            },
            ErrorCode::NotAuthorized => Guidance {
                problem: "was rejected because the Network Channel is missing or incorrect.",
                recovery: "Set the Network Channel to the value this ShaPrint network uses, then print again.",
            },
            ErrorCode::PrinterNotShared => Guidance {
                problem: "was rejected because the server does not share that printer.",
                recovery: "Ask the server user to select this printer in ShaPrint and start sharing, then print again.",
            },
            ErrorCode::QueueUnavailable => Guidance {
                problem: "was rejected because the server could not use its printer queue.",
                recovery: "Ask the server user to check the printer and its queue on the server, then print again.",
            },
            ErrorCode::InvalidInput => Guidance {
                problem: "was rejected because the request the printer sent could not be used.",
                recovery: "Open the Services panel, check that the client proxy is running, and print again from the printer's own dialog.",
            },
            ErrorCode::Timeout => Guidance {
                problem: "timed out before the server finished it.",
                recovery: "Check the server and the network route to it, then print again.",
            },
            ErrorCode::Unsupported => Guidance {
                problem: "was rejected because the server does not support it.",
                recovery: "Update ShaPrint on both computers so they agree on what a print job may contain, then print again.",
            },
            _ => Guidance {
                problem: "failed because the client proxy could not complete it.",
                recovery: "Open the Services panel, check that the client proxy is running, then print again.",
            },
        },
        JobPath::ServerSubmission => match code {
            ErrorCode::QueueUnavailable => Guidance {
                problem: "could not be submitted to the Windows printer queue.",
                recovery: "Check that the printer is switched on and reachable from this computer, then print again.",
            },
            ErrorCode::PrinterNotShared => Guidance {
                problem: "was rejected because that printer is no longer shared.",
                recovery: "Select this printer in the ShaPrint sharing panel and keep sharing running.",
            },
            ErrorCode::NotAuthorized => Guidance {
                problem: "was rejected because the Network Channel does not match.",
                recovery: "Set the Network Channel to the value this ShaPrint network uses, then restart sharing.",
            },
            ErrorCode::Timeout => Guidance {
                problem: "timed out while the printer queue was accepting it.",
                recovery: "Check that the printer is switched on and reachable from this computer, then print again.",
            },
            ErrorCode::InvalidInput => Guidance {
                problem: "was rejected because its printer settings are not supported.",
                recovery: "Print again with a supported media size, colour mode, and page count.",
            },
            ErrorCode::Unsupported => Guidance {
                problem: "could not be submitted because this server cannot reach Windows printer queues.",
                recovery: "Check that the printer is switched on and reachable from this computer, then print again.",
            },
            _ => Guidance {
                problem: "could not be submitted to the Windows printer queue.",
                recovery: "Check the printer and the sharing panel on this computer, then print again.",
            },
        },
    }
}

fn describe(printer: Option<&PrinterName>, problem: &str) -> String {
    match printer {
        Some(name) => format!("The job for '{}' {problem}", name.as_str()),
        None => format!("A print job {problem}"),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn printer(name: &str) -> PrinterName {
        PrinterName::parse(name).expect("a valid queue name")
    }

    #[test]
    fn path_names_are_stable() {
        let names: Vec<&str> = JobPath::ALL.iter().map(|path| path.as_str()).collect();
        assert_eq!(names, vec!["client-forwarding", "server-submission"]);
    }

    #[test]
    fn a_failure_names_the_queue_and_the_problem() {
        let queue = printer("Office Printer");
        let failure = PrintFailure::client(ErrorCode::ServerUnavailable, Some(&queue));

        assert_eq!(failure.path(), JobPath::ClientForwarding);
        assert_eq!(failure.code(), ErrorCode::ServerUnavailable);
        assert_eq!(failure.printer(), Some(&queue));
        assert_eq!(
            failure.message(),
            "The job for 'Office Printer' could not reach the ShaPrint server."
        );
        assert!(failure.observed_at_ms() > 0);
    }

    #[test]
    fn a_failure_without_a_queue_still_reads_as_a_sentence() {
        let failure = PrintFailure::server(ErrorCode::QueueUnavailable, None);

        assert_eq!(failure.printer(), None);
        assert_eq!(
            failure.message(),
            "A print job could not be submitted to the Windows printer queue."
        );
    }

    #[test]
    fn every_code_on_every_path_has_a_problem_and_a_specific_recovery_action() {
        let queue = printer("Zebra");
        for path in JobPath::ALL {
            let mut distinct_recoveries = HashSet::new();
            for code in ErrorCode::ALL {
                let failure = match path {
                    JobPath::ClientForwarding => PrintFailure::client(code, Some(&queue)),
                    JobPath::ServerSubmission => PrintFailure::server(code, Some(&queue)),
                };
                assert!(
                    failure.message().ends_with('.'),
                    "{} on {} has no problem sentence: {}",
                    code.as_str(),
                    path.as_str(),
                    failure.message()
                );
                let recovery = failure.recovery();
                assert!(
                    recovery.len() > 20 && recovery.ends_with('.'),
                    "{} on {} has no actionable recovery: {recovery}",
                    code.as_str(),
                    path.as_str()
                );
                distinct_recoveries.insert(recovery);
            }
            // A generic fallback is allowed for unexpected conditions, but the conditions a user can
            // actually hit must not all collapse into one sentence.
            assert!(
                distinct_recoveries.len() >= 5,
                "{} reports only {} distinct recovery actions",
                path.as_str(),
                distinct_recoveries.len()
            );
        }
    }

    #[test]
    fn the_recovery_action_names_the_condition_the_user_has_to_fix() {
        let queue = printer("Zebra");
        let cases = [
            (ErrorCode::ServerUnavailable, "server"),
            (ErrorCode::ServerNotTrusted, "approve"),
            (ErrorCode::ServerIdentityChanged, "reapprove"),
            (ErrorCode::NotAuthorized, "Network Channel"),
            (ErrorCode::PrinterNotShared, "sharing"),
            (ErrorCode::QueueUnavailable, "printer"),
            (ErrorCode::Timeout, "network"),
        ];
        for (code, expected) in cases {
            let failure = PrintFailure::client(code, Some(&queue));
            assert!(
                failure.recovery().contains(expected),
                "{} recovery does not mention '{expected}': {}",
                code.as_str(),
                failure.recovery()
            );
        }
    }

    #[test]
    fn a_queue_name_is_validated_before_it_can_reach_a_record() {
        // The only free text a record can echo is a queue name, and that name already passed the
        // same validation the spooler and the IPP client apply.
        assert!(PrinterName::parse("Zebra\r\nspoof").is_err());

        let queue = printer("Zebra");
        let failure = PrintFailure::client(ErrorCode::ServerUnavailable, Some(&queue));
        assert_eq!(
            failure.message(),
            "The job for 'Zebra' could not reach the ShaPrint server."
        );
        assert!(!failure.message().contains('\n'));
    }

    #[test]
    fn the_message_is_always_derived_from_the_code_and_the_queue() {
        // There is no field an adapter could put channel material or a document into: the same code
        // and queue always produce the same sentence.
        let queue = printer("Zebra");
        let first = PrintFailure::client(ErrorCode::NotAuthorized, Some(&queue));
        let second = PrintFailure::client(ErrorCode::NotAuthorized, Some(&queue));

        assert_eq!(first.message(), second.message());
        assert_eq!(
            first.message(),
            "The job for 'Zebra' was rejected because the Network Channel is missing or incorrect."
        );
    }

    #[test]
    fn a_long_queue_name_is_truncated_rather_than_published_whole() {
        let queue = printer(&"q".repeat(220));
        let failure = PrintFailure::client(ErrorCode::ServerUnavailable, Some(&queue));

        assert_eq!(failure.message().chars().count(), MESSAGE_LIMIT + 1);
        assert!(failure.message().ends_with('…'));
    }
}
