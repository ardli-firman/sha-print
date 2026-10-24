//! Session-only orchestration. Native adapters own page generation and submission.
use super::{LocalTroubleshooting, LocalTroubleshootingSnapshot};
use crate::domain::{AppError, RecognisedClientQueue};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

/// A catalog media size, never arbitrary driver text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestMedia(crate::adapters::printers::media::MediaSpec);
impl TestMedia {
    pub fn from_keyword(keyword: &str) -> Option<Self> {
        crate::adapters::printers::media::MEDIA_CATALOG
            .iter()
            .find(|m| m.keyword == keyword)
            .copied()
            .map(Self)
    }
    pub fn keyword(self) -> &'static str {
        self.0.keyword
    }
    pub fn dimensions_tenths_mm(self) -> (i16, i16) {
        (self.0.short_tenths_mm, self.0.long_tenths_mm)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionOutcome {
    NotSent,
    Checking,
    Accepted,
    Rejected,
    Uncertain,
    Blocked,
}
impl SubmissionOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotSent => "not-sent",
            Self::Checking => "checking",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Uncertain => "uncertain",
            Self::Blocked => "blocked",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalResult {
    Unconfirmed,
    Correct,
    NoOutput,
    Incorrect,
}
impl PhysicalResult {
    pub fn parse(s: &str) -> Result<Self, AppError> {
        match s {
            "correct" => Ok(Self::Correct),
            "no-output" => Ok(Self::NoOutput),
            "incorrect" => Ok(Self::Incorrect),
            _ => Err(AppError::invalid_input(
                "Choose the observed physical result.",
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unconfirmed => "unconfirmed",
            Self::Correct => "correct",
            Self::NoOutput => "no-output",
            Self::Incorrect => "incorrect",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserSymptom {
    Clipping,
    Shifted,
    IncorrectSize,
    Blank,
}
impl UserSymptom {
    pub fn parse(s: &str) -> Result<Self, AppError> {
        match s {
            "clipping" => Ok(Self::Clipping),
            "shifted" | "shifted-position" => Ok(Self::Shifted),
            "size" | "incorrect-size" => Ok(Self::IncorrectSize),
            "blank" | "blank-output" => Ok(Self::Blank),
            _ => Err(AppError::invalid_input("Choose an observed symptom.")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clipping => "clipping",
            Self::Shifted => "shifted",
            Self::IncorrectSize => "incorrect-size",
            Self::Blank => "blank",
        }
    }
}
#[async_trait::async_trait]
pub trait NativeTestPrint: Send + Sync + 'static {
    async fn default_media(
        &self,
        target: &RecognisedClientQueue,
    ) -> Result<Option<TestMedia>, AppError>;
    /// Recheck native destination, pending state and media before opening a document.
    /// Return Rejected only when no document could have been accepted.
    async fn submit(&self, target: &RecognisedClientQueue, media: TestMedia) -> SubmissionOutcome;
}
#[derive(Clone)]
pub struct TestPrintSnapshot {
    pub session_id: u64,
    pub diagnostics: LocalTroubleshootingSnapshot,
    pub requested_media: Option<TestMedia>,
    pub submission: SubmissionOutcome,
    pub physical_result: PhysicalResult,
    pub user_symptom: Option<UserSymptom>,
    pub attempts: Vec<SubmissionOutcome>,
    pub supported: bool,
    ready: bool,
    run: u64,
}
impl TestPrintSnapshot {
    pub fn can_send(&self) -> bool {
        self.supported
            && self.ready
            && self.requested_media.is_some()
            && self.diagnostics.is_repair_eligible()
    }
}
pub struct GuidedTestPrint {
    checks: LocalTroubleshooting,
    native: Option<Arc<dyn NativeTestPrint>>,
    session: Mutex<Option<TestPrintSnapshot>>,
    next_id: AtomicU64,
    submitting: Arc<AtomicBool>,
}
fn stale() -> AppError {
    AppError::invalid_state("This test print session is closed, busy, or superseded. Check again.")
}
struct SubmissionGuard(Arc<AtomicBool>);
impl Drop for SubmissionGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
impl GuidedTestPrint {
    pub fn new(checks: LocalTroubleshooting, native: Option<Arc<dyn NativeTestPrint>>) -> Self {
        Self {
            checks,
            native,
            session: Mutex::new(None),
            next_id: AtomicU64::new(1),
            submitting: Arc::new(AtomicBool::new(false)),
        }
    }
    pub async fn open(&self, target: RecognisedClientQueue) -> Result<TestPrintSnapshot, AppError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *session = Some(TestPrintSnapshot {
                session_id: id,
                diagnostics: LocalTroubleshootingSnapshot {
                    target,
                    checks: vec![],
                },
                requested_media: None,
                submission: SubmissionOutcome::NotSent,
                physical_result: PhysicalResult::Unconfirmed,
                user_symptom: None,
                attempts: vec![],
                supported: self.native.is_some(),
                ready: false,
                run: 0,
            });
        }
        self.refresh(id).await
    }
    pub fn close(&self, id: u64) {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if session.as_ref().is_some_and(|s| s.session_id == id) {
            *session = None;
        }
    }
    async fn inspect(
        &self,
        target: RecognisedClientQueue,
    ) -> (LocalTroubleshootingSnapshot, Option<TestMedia>) {
        let deadline =
            tokio::time::Instant::now() + super::LOCAL_DIAGNOSTIC_TIMEOUT - Duration::from_secs(1);
        let diagnostics = self.checks.inspect(target.clone()).await;
        let media = if let Some(native) = &self.native {
            tokio::time::timeout_at(deadline, native.default_media(&target))
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
        } else {
            None
        };
        let mut diagnostics = diagnostics;
        self.checks.refresh_live_prerequisites(&mut diagnostics);
        (diagnostics, media)
    }
    pub async fn refresh(&self, id: u64) -> Result<TestPrintSnapshot, AppError> {
        let (target, run) = {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let s = session
                .as_mut()
                .filter(|s| s.session_id == id && s.submission != SubmissionOutcome::Checking)
                .ok_or_else(stale)?;
            s.ready = false;
            s.run += 1;
            (s.diagnostics.target.clone(), s.run)
        };
        let (diagnostics, media) = self.inspect(target).await;
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let s = session
            .as_mut()
            .filter(|s| s.session_id == id && s.run == run)
            .ok_or_else(stale)?;
        s.diagnostics = diagnostics;
        s.requested_media = media;
        s.ready = !self.submitting.load(Ordering::SeqCst);
        Ok(s.clone())
    }
    pub async fn send(&self, id: u64) -> Result<TestPrintSnapshot, AppError> {
        let native = self.native.clone().ok_or_else(|| {
            AppError::unsupported("Native test print is not available on this platform.")
        })?;
        let (target, preview_media, run) = {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let s = session
                .as_mut()
                .filter(|s| s.session_id == id && s.can_send())
                .ok_or_else(stale)?;
            if self.submitting.swap(true, Ordering::SeqCst) {
                return Err(stale());
            }
            s.ready = false;
            s.run += 1;
            s.submission = SubmissionOutcome::Checking;
            s.physical_result = PhysicalResult::Unconfirmed;
            s.user_symptom = None;
            (s.diagnostics.target.clone(), s.requested_media, s.run)
        };
        let guard = SubmissionGuard(Arc::clone(&self.submitting));
        let (diagnostics, media) = self.inspect(target.clone()).await;
        let mut worker = {
            let mut session = self
                .session
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let s = session
                .as_mut()
                .filter(|s| s.session_id == id && s.run == run)
                .ok_or_else(stale)?;
            s.diagnostics = diagnostics;
            s.requested_media = media;
            if !s.diagnostics.is_repair_eligible() || media.is_none() || media != preview_media {
                s.submission = SubmissionOutcome::Blocked;
                s.attempts.push(s.submission);
                return Ok(s.clone());
            }
            let Some(media) = media else {
                return Err(stale());
            };
            // Spawn while holding the session lock: close cannot race the decision to send.
            tokio::spawn(async move {
                let _guard = guard;
                native.submit(&target, media).await
            })
        };
        // Dropping a timed-out JoinHandle does not cancel a spooler operation. Its guard
        // keeps all sessions blocked until that operation really finishes.
        let outcome = match tokio::time::timeout(Duration::from_secs(30), &mut worker).await {
            Ok(Ok(SubmissionOutcome::Accepted)) => SubmissionOutcome::Accepted,
            Ok(Ok(SubmissionOutcome::Rejected)) => SubmissionOutcome::Rejected,
            _ => SubmissionOutcome::Uncertain,
        };
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let s = session
            .as_mut()
            .filter(|s| s.session_id == id && s.run == run)
            .ok_or_else(stale)?;
        s.submission = outcome;
        s.physical_result = PhysicalResult::Unconfirmed;
        s.user_symptom = None;
        s.attempts.push(outcome);
        Ok(s.clone())
    }
    pub fn confirm(
        &self,
        id: u64,
        result: PhysicalResult,
        symptom: Option<UserSymptom>,
    ) -> Result<TestPrintSnapshot, AppError> {
        let mut session = self
            .session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let s = session
            .as_mut()
            .filter(|s| {
                s.session_id == id
                    && matches!(
                        s.submission,
                        SubmissionOutcome::Accepted | SubmissionOutcome::Uncertain
                    )
            })
            .ok_or_else(stale)?;
        if result != PhysicalResult::Incorrect && symptom.is_some() {
            return Err(AppError::invalid_input(
                "Symptoms apply only to incorrect output.",
            ));
        }
        s.physical_result = result;
        s.user_symptom = symptom;
        Ok(s.clone())
    }
}
