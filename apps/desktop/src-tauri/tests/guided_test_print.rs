//! Exercises session orchestration at the agreed application boundary.
use async_trait::async_trait;
use shaprint_desktop::application::*;
use shaprint_desktop::domain::{
    AppError, ClientQueueName, PrinterName, RecognisedClientQueue, ServiceId,
};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};
struct Spooler {
    pending: AtomicU32,
    sends: AtomicU32,
    media: AtomicU32,
    media_error: Mutex<Option<String>>,
    server: AtomicU32,
    rejected: AtomicBool,
    outcome: Mutex<SubmissionOutcome>,
    delayed: AtomicBool,
    release: tokio::sync::Notify,
}
#[async_trait]
impl LocalPrinterCatalog for Spooler {
    async fn local_printers(&self) -> Result<Vec<PrinterName>, AppError> {
        unreachable!()
    }
    async fn inspect_client_queue(
        &self,
        _: &RecognisedClientQueue,
    ) -> Result<LocalQueueInspection, AppError> {
        Ok(match self.pending.load(Ordering::SeqCst) {
            u32::MAX => LocalQueueInspection::Matching { pending_jobs: None },
            100 => LocalQueueInspection::Missing,
            101 => LocalQueueInspection::Mismatched,
            n => LocalQueueInspection::Matching {
                pending_jobs: Some(n),
            },
        })
    }
}
#[async_trait]
impl NativeTestPrint for Spooler {
    async fn default_media(
        &self,
        _: &RecognisedClientQueue,
    ) -> Result<Option<TestMedia>, AppError> {
        if let Some(error) = self.media_error.lock().unwrap().as_ref() {
            return Err(AppError::internal(error));
        }
        Ok(match self.media.load(Ordering::SeqCst) {
            0 => TestMedia::from_keyword("iso_a4_210x297mm"),
            1 => None,
            _ => TestMedia::from_keyword("na_letter_8.5x11in"),
        })
    }
    async fn submit(&self, target: &RecognisedClientQueue, media: TestMedia) -> SubmissionOutcome {
        assert_eq!(
            target.queue_name().as_str(),
            "Office (ShaPrint 192.0.2.10-48631)"
        );
        assert_eq!(media.keyword(), "iso_a4_210x297mm");
        self.sends.fetch_add(1, Ordering::SeqCst);
        if self.delayed.load(Ordering::SeqCst) {
            self.release.notified().await;
        }
        *self.outcome.lock().unwrap()
    }
}
#[async_trait]
impl ServerInspector for Spooler {
    async fn inspect_server(&self, _: &str, _: &str) -> Result<ServerInspection, AppError> {
        Ok(match self.server.load(Ordering::SeqCst) {
            1 => ServerInspection::Unreachable,
            2 => ServerInspection::Timeout,
            state => ServerInspection::Reachable {
                identity: match state {
                    3 => ServerIdentityInspection::Changed,
                    5 => ServerIdentityInspection::NotTrusted,
                    _ => ServerIdentityInspection::Matching,
                },
                printer_shared: Some(state != 4),
            },
        })
    }
}
struct Channel;
impl ChannelState for Channel {
    fn is_configured(&self) -> bool {
        true
    }
}
impl ObservedAuthorization for Spooler {
    fn is_rejected(&self, _: &str) -> bool {
        self.rejected.load(Ordering::SeqCst)
    }
}
struct Proxy;
#[async_trait]
impl RuntimeService for Proxy {
    fn id(&self) -> ServiceId {
        ServiceId::ClientProxy
    }
    fn autostart(&self) -> bool {
        false
    }
    async fn run(&self, context: ServiceContext) -> Result<(), AppError> {
        context.reporter().ready()?;
        context.cancelled().await;
        Ok(())
    }
}
fn target() -> RecognisedClientQueue {
    RecognisedClientQueue::new(
        ClientQueueName::parse("Office (ShaPrint 192.0.2.10-48631)").unwrap(),
        "192.0.2.10:48631".into(),
        PrinterName::parse("Office").unwrap(),
    )
}
async fn fixture() -> (Arc<GuidedTestPrint>, Arc<Spooler>) {
    let spooler = Arc::new(Spooler {
        pending: AtomicU32::new(0),
        sends: AtomicU32::new(0),
        media: AtomicU32::new(0),
        media_error: Mutex::new(None),
        server: AtomicU32::new(0),
        rejected: AtomicBool::new(false),
        outcome: Mutex::new(SubmissionOutcome::Accepted),
        delayed: AtomicBool::new(false),
        release: tokio::sync::Notify::new(),
    });
    let runtime = Arc::new(RuntimeCoordinator::new(vec![Arc::new(Proxy)]));
    runtime.start(ServiceId::ClientProxy).await.unwrap();
    let checks = LocalTroubleshooting::new(spooler.clone(), runtime).with_server_inspection(
        spooler.clone(),
        Arc::new(Channel),
        spooler.clone(),
    );
    (
        Arc::new(GuidedTestPrint::new(checks, Some(spooler.clone()))),
        spooler,
    )
}
#[tokio::test]
async fn explicit_send_rechecks_queue_and_never_claims_physical_output() {
    let (flow, spooler) = fixture().await;
    let ready = flow.open(target()).await.unwrap();
    assert!(ready.can_send());
    assert_eq!(spooler.sends.load(Ordering::SeqCst), 0);
    spooler.pending.store(1, Ordering::SeqCst);
    let blocked = flow.send(ready.session_id).await.unwrap();
    assert_eq!(blocked.submission, SubmissionOutcome::Blocked);
    assert_eq!(spooler.sends.load(Ordering::SeqCst), 0);
    spooler.pending.store(0, Ordering::SeqCst);
    flow.refresh(ready.session_id).await.unwrap();
    let accepted = flow.send(ready.session_id).await.unwrap();
    assert_eq!(accepted.submission, SubmissionOutcome::Accepted);
    assert_eq!(accepted.physical_result, PhysicalResult::Unconfirmed);
    assert!(!accepted.can_send());
    assert_eq!(spooler.sends.load(Ordering::SeqCst), 1);
    // The blocked attempt submitted no page, so the accepted page is the first send.
    let public = shaprint_desktop::ipc::dto::LocalTroubleshootingDto::from(accepted);
    assert_eq!(
        public
            .attempted_actions
            .iter()
            .map(|action| (action.action.as_str(), action.outcome.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("send-test-print", "blocked"),
            ("send-test-print", "accepted"),
        ]
    );
    assert!(flow.send(ready.session_id).await.is_err());
    let confirmed = flow
        .confirm(ready.session_id, PhysicalResult::Correct, None)
        .unwrap();
    assert_eq!(confirmed.physical_result, PhysicalResult::Correct);
}

#[tokio::test]
async fn public_snapshot_reports_requested_media_and_user_confirmation_separately() {
    use shaprint_desktop::ipc::dto::LocalTroubleshootingDto;
    let (flow, _) = fixture().await;
    let ready = flow.open(target()).await.unwrap();
    let dto = LocalTroubleshootingDto::from(ready.clone());
    assert_eq!(dto.session_id, Some(ready.session_id));
    assert_eq!(dto.requested_media.as_deref(), Some("iso_a4_210x297mm"));
    assert!(dto.test_print_enabled);
    let sent = LocalTroubleshootingDto::from(flow.send(ready.session_id).await.unwrap());
    assert_eq!(sent.submission.as_deref(), Some("accepted"));
    assert_eq!(sent.physical_result.as_deref(), Some("unconfirmed"));
    assert_eq!(sent.attempted_actions[0].outcome, "accepted");
}

#[tokio::test]
async fn reopening_discards_physical_confirmation_and_rejects_old_callbacks() {
    let (flow, spooler) = fixture().await;
    let first = flow.open(target()).await.unwrap();
    flow.send(first.session_id).await.unwrap();
    flow.confirm(first.session_id, PhysicalResult::NoOutput, None)
        .unwrap();
    flow.close(first.session_id);
    let second = flow.open(target()).await.unwrap();
    assert_ne!(first.session_id, second.session_id);
    assert_eq!(second.physical_result, PhysicalResult::Unconfirmed);
    assert_eq!(second.submission, SubmissionOutcome::NotSent);
    assert!(second.attempts.is_empty());
    assert!(flow
        .confirm(first.session_id, PhysicalResult::Correct, None)
        .is_err());
    assert!(flow.send(first.session_id).await.is_err());
    assert_eq!(spooler.sends.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn missing_media_and_changed_prerequisites_block_without_native_submission() {
    for pending in [1, u32::MAX, 100, 101] {
        let (flow, native) = fixture().await;
        let ready = flow.open(target()).await.unwrap();
        native.pending.store(pending, Ordering::SeqCst);
        assert_eq!(
            flow.send(ready.session_id).await.unwrap().submission,
            SubmissionOutcome::Blocked
        );
        assert_eq!(native.sends.load(Ordering::SeqCst), 0);
    }
    for server in 1..=5 {
        let (flow, native) = fixture().await;
        let ready = flow.open(target()).await.unwrap();
        native.server.store(server, Ordering::SeqCst);
        assert_eq!(
            flow.send(ready.session_id).await.unwrap().submission,
            SubmissionOutcome::Blocked
        );
        assert_eq!(native.sends.load(Ordering::SeqCst), 0);
    }
    let (flow, native) = fixture().await;
    let ready = flow.open(target()).await.unwrap();
    native.rejected.store(true, Ordering::SeqCst);
    assert_eq!(
        flow.send(ready.session_id).await.unwrap().submission,
        SubmissionOutcome::Blocked
    );
    assert_eq!(native.sends.load(Ordering::SeqCst), 0);
    for media in [1, 2] {
        let (flow, native) = fixture().await;
        let ready = flow.open(target()).await.unwrap();
        native.media.store(media, Ordering::SeqCst);
        let blocked = flow.send(ready.session_id).await.unwrap();
        assert_eq!(blocked.submission, SubmissionOutcome::Blocked);
        assert_eq!(native.sends.load(Ordering::SeqCst), 0);
    }
    let (flow, native) = fixture().await;
    native.media.store(1, Ordering::SeqCst);
    let snapshot = flow.open(target()).await.unwrap();
    assert!(!snapshot.can_send());
    let public = shaprint_desktop::ipc::dto::LocalTroubleshootingDto::from(snapshot);
    assert!(public
        .checks
        .iter()
        .any(|check| check.check == "requested-media" && check.status == "inconclusive"));
}

#[tokio::test]
async fn rejection_uncertainty_and_all_physical_results_remain_distinct() {
    for outcome in [
        SubmissionOutcome::Rejected,
        SubmissionOutcome::Uncertain,
        SubmissionOutcome::Accepted,
    ] {
        let (flow, native) = fixture().await;
        *native.outcome.lock().unwrap() = outcome;
        let ready = flow.open(target()).await.unwrap();
        let sent = flow.send(ready.session_id).await.unwrap();
        assert_eq!(sent.submission, outcome);
        assert_eq!(sent.physical_result, PhysicalResult::Unconfirmed);
        assert!(!sent.can_send());
        for physical in [
            PhysicalResult::Correct,
            PhysicalResult::NoOutput,
            PhysicalResult::Incorrect,
        ] {
            let result = flow.confirm(ready.session_id, physical, None);
            if outcome == SubmissionOutcome::Rejected {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap().physical_result, physical);
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn timeout_and_closed_sessions_never_retry_or_consume_late_submission_results() {
    let (flow, native) = fixture().await;
    native.delayed.store(true, Ordering::SeqCst);
    let ready = flow.open(target()).await.unwrap();
    let send = tokio::spawn({
        let flow = flow.clone();
        async move { flow.send(ready.session_id).await }
    });
    while native.sends.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    assert!(flow.send(ready.session_id).await.is_err());
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    let timed_out = send.await.unwrap().unwrap();
    assert_eq!(timed_out.submission, SubmissionOutcome::Uncertain);
    assert!(!flow.refresh(ready.session_id).await.unwrap().can_send());
    flow.close(ready.session_id);
    let fresh = flow.open(target()).await.unwrap();
    assert!(!fresh.can_send());
    native.release.notify_one();
    tokio::task::yield_now().await;
    let fresh = flow.refresh(fresh.session_id).await.unwrap();
    assert_eq!(fresh.submission, SubmissionOutcome::NotSent);
    assert_eq!(fresh.physical_result, PhysicalResult::Unconfirmed);
    assert!(fresh.attempts.is_empty());
    assert!(fresh.can_send());
    assert_eq!(native.sends.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn closing_during_submission_does_not_confirm_or_cancel_work() {
    let (flow, native) = fixture().await;
    native.delayed.store(true, Ordering::SeqCst);
    let ready = flow.open(target()).await.unwrap();
    let send = tokio::spawn({
        let flow = flow.clone();
        async move { flow.send(ready.session_id).await }
    });
    while native.sends.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    flow.close(ready.session_id);
    let fresh = flow.open(target()).await.unwrap();
    assert!(!fresh.can_send());
    native.release.notify_one();
    assert!(send.await.unwrap().is_err());
    let fresh = flow.refresh(fresh.session_id).await.unwrap();
    assert_eq!(fresh.submission, SubmissionOutcome::NotSent);
    assert!(fresh.attempts.is_empty());
    assert_eq!(native.sends.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn protected_driver_data_cannot_enter_the_public_session_snapshot() {
    use ring::rand::{SecureRandom, SystemRandom};
    let (flow, native) = fixture().await;
    let mut bytes = [0u8; 32];
    SystemRandom::new().fill(&mut bytes).unwrap();
    let generated = hex::encode(bytes);
    *native.media_error.lock().unwrap() = Some(generated.clone());
    let snapshot = flow.open(target()).await.unwrap();
    assert!(!snapshot.can_send());
    let dto = shaprint_desktop::ipc::dto::LocalTroubleshootingDto::from(snapshot);
    let public = serde_json::to_string(&dto).unwrap();
    assert!(!public.contains(&generated));
    assert!(!public.contains("document"));
    assert!(dto.requested_media.is_none());
    assert_eq!(native.sends.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn classified_symptoms_are_recorded_and_closed_to_free_form_values() {
    use shaprint_desktop::ipc::dto::LocalTroubleshootingDto;
    let (flow, native) = fixture().await;
    let ready = flow.open(target()).await.unwrap();
    flow.send(ready.session_id).await.unwrap();
    for (symptom, wire, alternate) in [
        (UserSymptom::Clipping, "clipping", "clipping"),
        (UserSymptom::Shifted, "shifted", "shifted-position"),
        (UserSymptom::IncorrectSize, "incorrect-size", "size"),
        (UserSymptom::Blank, "blank", "blank-output"),
    ] {
        assert_eq!(UserSymptom::parse(wire).unwrap(), symptom);
        assert_eq!(UserSymptom::parse(alternate).unwrap(), symptom);
        let reported = flow
            .confirm(ready.session_id, PhysicalResult::Incorrect, Some(symptom))
            .unwrap();
        assert_eq!(reported.physical_result, PhysicalResult::Incorrect);
        assert_eq!(reported.user_symptom, Some(symptom));
        let public = LocalTroubleshootingDto::from(reported);
        assert_eq!(public.user_symptom.as_deref(), Some(wire));
        // A later correct result replaces the incorrect report and clears the symptom.
        let corrected = flow
            .confirm(ready.session_id, PhysicalResult::Correct, None)
            .unwrap();
        assert_eq!(corrected.user_symptom, None);
        // Symptoms never apply to non-incorrect results.
        assert!(flow
            .confirm(ready.session_id, PhysicalResult::Correct, Some(symptom))
            .is_err());
        assert!(flow
            .confirm(ready.session_id, PhysicalResult::Incorrect, Some(symptom))
            .is_ok());
    }
    // Free-form symptom values are rejected without reaching the snapshot.
    let generated = {
        use ring::rand::{SecureRandom, SystemRandom};
        let mut bytes = [0u8; 16];
        SystemRandom::new().fill(&mut bytes).unwrap();
        hex::encode(bytes)
    };
    assert!(UserSymptom::parse(&generated).is_err());
    let reported = flow
        .confirm(ready.session_id, PhysicalResult::Incorrect, None)
        .unwrap();
    assert_eq!(reported.user_symptom, None);
    assert_eq!(native.sends.load(Ordering::SeqCst), 1);
    // A stale session cannot classify symptoms into a newer session.
    flow.close(ready.session_id);
    let fresh = flow.open(target()).await.unwrap();
    assert_eq!(fresh.user_symptom, None);
    assert!(flow
        .confirm(
            ready.session_id,
            PhysicalResult::Incorrect,
            Some(UserSymptom::Blank)
        )
        .is_err());
    let refreshed = flow.refresh(fresh.session_id).await.unwrap();
    assert_eq!(refreshed.user_symptom, None);
}

#[tokio::test]
async fn explicit_retry_requires_fresh_checks_and_records_the_retry_attempt() {
    use shaprint_desktop::ipc::dto::LocalTroubleshootingDto;
    let (flow, native) = fixture().await;
    let ready = flow.open(target()).await.unwrap();
    let first = flow.send(ready.session_id).await.unwrap();
    assert_eq!(first.submission, SubmissionOutcome::Accepted);
    assert_eq!(first.attempts, vec![SubmissionOutcome::Accepted]);
    // No automatic retry: an immediate second send is refused until checks rerun.
    assert!(flow.send(ready.session_id).await.is_err());
    assert_eq!(native.sends.load(Ordering::SeqCst), 1);
    flow.confirm(ready.session_id, PhysicalResult::NoOutput, None)
        .unwrap();
    let fresh = flow.refresh(ready.session_id).await.unwrap();
    assert!(fresh.can_send(), "{:?}", fresh.diagnostics.checks);
    let retry = flow.send(ready.session_id).await.unwrap();
    assert_eq!(retry.submission, SubmissionOutcome::Accepted);
    assert_eq!(retry.user_symptom, None);
    assert_eq!(retry.attempts.len(), 2);
    assert_eq!(native.sends.load(Ordering::SeqCst), 2);
    let public = LocalTroubleshootingDto::from(retry);
    assert_eq!(
        public
            .attempted_actions
            .iter()
            .map(|action| (action.action.as_str(), action.outcome.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("send-test-print", "accepted"),
            ("retry-test-print", "accepted"),
        ]
    );
    // Still no third send without another explicit retry after fresh checks.
    assert!(flow.send(ready.session_id).await.is_err());
    assert_eq!(native.sends.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn retry_is_blocked_by_newly_busy_unknown_queue_or_changed_trust() {
    // Each mutation is applied after fresh checks passed, so only the recheck
    // that runs inside the explicit send itself can still refuse the retry.
    let block_at_send = [
        ("newly pending jobs", 1u32, 0u32, false),
        ("unreadable pending state", u32::MAX, 0, false),
        ("changed server trust", 0, 3, false),
        ("observed channel rejection", 0, 0, true),
    ];
    for (label, pending, server, rejected) in block_at_send {
        let (flow, native) = fixture().await;
        let ready = flow.open(target()).await.unwrap();
        flow.send(ready.session_id).await.unwrap();
        flow.confirm(ready.session_id, PhysicalResult::Incorrect, None)
            .unwrap();
        let fresh = flow.refresh(ready.session_id).await.unwrap();
        assert!(fresh.can_send());
        if pending != 0 {
            native.pending.store(pending, Ordering::SeqCst);
        }
        if server != 0 {
            native.server.store(server, Ordering::SeqCst);
        }
        native.rejected.store(rejected, Ordering::SeqCst);
        let blocked = flow.send(ready.session_id).await.unwrap();
        assert_eq!(blocked.submission, SubmissionOutcome::Blocked, "{label}");
        assert_eq!(native.sends.load(Ordering::SeqCst), 1, "{label}");
        assert_eq!(
            blocked.attempts.last(),
            Some(&SubmissionOutcome::Blocked),
            "{label}: the refused retry is recorded, not hidden"
        );
    }
    // Blocked before submission when the rerun checks already observe the problem.
    let block_at_refresh = [
        ("newly pending jobs", 1u32, 0u32),
        ("unreadable pending state", u32::MAX, 0),
        ("changed server trust", 0, 3),
    ];
    for (label, pending, server) in block_at_refresh {
        let (flow, native) = fixture().await;
        let ready = flow.open(target()).await.unwrap();
        flow.send(ready.session_id).await.unwrap();
        if pending != 0 {
            native.pending.store(pending, Ordering::SeqCst);
        }
        if server != 0 {
            native.server.store(server, Ordering::SeqCst);
        }
        let stale = flow.refresh(ready.session_id).await.unwrap();
        assert!(!stale.can_send(), "{label}: {:?}", stale.diagnostics.checks);
        assert!(flow.send(ready.session_id).await.is_err(), "{label}");
        assert_eq!(native.sends.load(Ordering::SeqCst), 1, "{label}");
    }
}
