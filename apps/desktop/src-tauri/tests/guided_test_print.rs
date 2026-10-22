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
    assert!(flow.send(ready.session_id).await.is_err());
    let confirmed = flow
        .confirm(ready.session_id, PhysicalResult::Correct)
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
    flow.confirm(first.session_id, PhysicalResult::NoOutput)
        .unwrap();
    flow.close(first.session_id);
    let second = flow.open(target()).await.unwrap();
    assert_ne!(first.session_id, second.session_id);
    assert_eq!(second.physical_result, PhysicalResult::Unconfirmed);
    assert_eq!(second.submission, SubmissionOutcome::NotSent);
    assert!(second.attempts.is_empty());
    assert!(flow
        .confirm(first.session_id, PhysicalResult::Correct)
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
            let result = flow.confirm(ready.session_id, physical);
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
