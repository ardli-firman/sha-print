//! The shell's print-failure surface: the latest failed print attempt (#39).
//!
//! One shell-wide surface, like the runtime status registry: the client proxy and the sharing
//! endpoint both report here, so a user sees one place that answers "why did my print not come
//! out?". Only the latest failure is kept, and a failure carries a stable code, a derived message,
//! and the action that resolves it — never channel material or document contents.

use tokio::sync::watch;

use crate::domain::PrintFailure;

/// Holds the latest print failure and publishes every change.
///
/// One channel is both the state and the notification, so the value a reader sees and the value
/// subscribers were told about can never disagree.
#[derive(Debug)]
pub struct PrintFailures {
    latest: watch::Sender<Option<PrintFailure>>,
}

impl Default for PrintFailures {
    fn default() -> Self {
        Self::new()
    }
}

impl PrintFailures {
    /// Nothing has failed yet.
    pub fn new() -> Self {
        let (latest, _) = watch::channel(None);
        Self { latest }
    }

    /// Records a failure, replacing whatever was reported before.
    pub fn report(&self, failure: PrintFailure) {
        self.latest.send_replace(Some(failure));
    }

    /// The latest failure, or `None` once the user has dismissed it.
    pub fn latest(&self) -> Option<PrintFailure> {
        self.latest.borrow().clone()
    }

    /// Dismisses the latest failure. Reporting the same problem again brings it back.
    pub fn clear(&self) {
        self.latest.send_replace(None);
    }

    /// Follows every change; the receiver also holds the current value.
    pub fn subscribe(&self) -> watch::Receiver<Option<PrintFailure>> {
        self.latest.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ErrorCode, JobPath, PrinterName};

    fn printer(name: &str) -> PrinterName {
        PrinterName::parse(name).expect("a valid queue name")
    }

    #[test]
    fn nothing_is_reported_before_a_job_fails() {
        let failures = PrintFailures::new();

        assert_eq!(failures.latest(), None);
    }

    #[test]
    fn the_latest_failure_replaces_the_previous_one() {
        let failures = PrintFailures::new();
        let zebra = printer("Zebra");
        let office = printer("Office Printer");

        failures.report(PrintFailure::client(
            ErrorCode::ServerUnavailable,
            Some(&zebra),
        ));
        failures.report(PrintFailure::server(
            ErrorCode::QueueUnavailable,
            Some(&office),
        ));

        let failure = failures.latest().expect("a failure");
        assert_eq!(failure.path(), JobPath::ServerSubmission);
        assert_eq!(failure.code(), ErrorCode::QueueUnavailable);
        assert_eq!(
            failure.message(),
            "The job for 'Office Printer' could not be submitted to the Windows printer queue."
        );
    }

    #[test]
    fn a_dismissal_is_not_a_report() {
        let failures = PrintFailures::new();
        failures.report(PrintFailure::client(
            ErrorCode::ServerUnavailable,
            Some(&printer("Zebra")),
        ));

        failures.clear();

        assert_eq!(failures.latest(), None);
    }

    #[tokio::test]
    async fn a_subscriber_sees_every_report_and_the_dismissal() {
        let failures = PrintFailures::new();
        let mut changes = failures.subscribe();
        assert_eq!(changes.borrow_and_update().clone(), None);

        failures.report(PrintFailure::client(
            ErrorCode::ServerNotTrusted,
            Some(&printer("Zebra")),
        ));
        changes.changed().await.expect("publishes the failure");
        assert_eq!(
            changes.borrow_and_update().as_ref().map(PrintFailure::code),
            Some(ErrorCode::ServerNotTrusted)
        );

        failures.clear();
        changes.changed().await.expect("publishes the dismissal");
        assert_eq!(changes.borrow_and_update().clone(), None);
        assert_eq!(failures.latest(), None);
    }

    #[tokio::test]
    async fn reporting_the_same_problem_again_is_published_again() {
        let failures = PrintFailures::new();
        let mut changes = failures.subscribe();
        let zebra = printer("Zebra");

        for expected in 1..=2 {
            failures.report(PrintFailure::server(
                ErrorCode::QueueUnavailable,
                Some(&zebra),
            ));
            changes.changed().await.expect("publishes every attempt");
            let published = changes.borrow_and_update().clone();
            assert!(published.is_some(), "attempt {expected} was not published");
        }
    }
}
