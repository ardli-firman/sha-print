//! In-flight print job tracking and safe restart drain guard (Issue #55).
//!
//! Tracks active in-flight Print jobs across both the local Client proxy and the Server IPPS
//! sharing endpoint, providing a drain guard that prevents update restarts or lifecycle interruptions
//! from cutting off documents mid-transfer or while the Windows spooler is flushing data.

use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::Instant;

/// Drain cooldown buffer holding restart readiness after the last in-flight job completes (8 seconds).
pub const DRAIN_COOLDOWN: Duration = Duration::from_secs(8);

/// Quiet window of zero print jobs required for background auto-updates (15 minutes).
pub const QUIET_WINDOW: Duration = Duration::from_secs(15 * 60);

/// An RAII request lease promoted to an in-flight Print job once IPP identifies its operation.
#[derive(Debug)]
pub struct RequestLease {
    tracker: Arc<PrintJobTrackerInner>,
    is_print_job: bool,
}

/// Closes admission for new jobs after the tracker atomically confirms the drain is clear.
#[derive(Debug)]
pub struct RestartPermit {
    tracker: Arc<PrintJobTrackerInner>,
}

impl Drop for RestartPermit {
    fn drop(&mut self) {
        if let Ok(mut state) = self.tracker.state.write() {
            state.accepting_jobs = true;
        }
    }
}

impl RequestLease {
    /// Marks an accepted IPP request as a Print job after its operation is decoded.
    pub fn mark_print_job(&mut self) -> bool {
        if self.is_print_job {
            return true;
        }
        if !self.tracker.promote_request() {
            return false;
        }
        self.is_print_job = true;
        true
    }
}

impl Drop for RequestLease {
    fn drop(&mut self) {
        self.tracker.release(self.is_print_job);
    }
}

#[derive(Debug)]
struct TrackerState {
    active_jobs: usize,
    active_requests: usize,
    accepting_jobs: bool,
    last_completed_at: Option<Instant>,
    last_busy_at: Instant,
}

#[derive(Debug)]
struct PrintJobTrackerInner {
    state: RwLock<TrackerState>,
}

impl PrintJobTrackerInner {
    fn promote_request(&self) -> bool {
        let Ok(mut state) = self.state.write() else {
            return false;
        };
        state.active_requests = state.active_requests.saturating_sub(1);
        state.active_jobs += 1;
        state.last_busy_at = Instant::now();
        true
    }

    fn release(&self, is_print_job: bool) {
        if let Ok(mut state) = self.state.write() {
            if is_print_job {
                state.active_jobs = state.active_jobs.saturating_sub(1);
                if state.active_jobs == 0 {
                    let now = Instant::now();
                    state.last_completed_at = Some(now);
                    state.last_busy_at = now;
                }
            } else {
                state.active_requests = state.active_requests.saturating_sub(1);
            }
        }
    }
}

/// Tracks in-flight print jobs across client proxy and server endpoints.
#[derive(Debug, Clone)]
pub struct PrintJobTracker {
    inner: Arc<PrintJobTrackerInner>,
}

impl Default for PrintJobTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl PrintJobTracker {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            inner: Arc::new(PrintJobTrackerInner {
                state: RwLock::new(TrackerState {
                    active_jobs: 0,
                    active_requests: 0,
                    accepting_jobs: true,
                    last_completed_at: None,
                    last_busy_at: now,
                }),
            }),
        }
    }

    /// Number of active print jobs currently in flight.
    pub fn active_count(&self) -> usize {
        self.inner
            .state
            .read()
            .map(|state| state.active_jobs)
            .unwrap_or(0)
    }

    /// Acquires an IPP request lease before reading its body; returns `None` while restarting.
    /// Promote it with `mark_print_job` after decoding a Print job operation.
    pub fn try_acquire_request(&self) -> Option<RequestLease> {
        let mut state = self.inner.state.write().ok()?;
        if !state.accepting_jobs {
            return None;
        }
        state.active_requests += 1;
        Some(RequestLease {
            tracker: Arc::clone(&self.inner),
            is_print_job: false,
        })
    }

    /// Atomically reserves a restart window once all jobs and the drain cooldown have cleared.
    /// The returned permit blocks new jobs until it is dropped after restart or recovery.
    pub fn try_begin_restart(&self) -> Option<RestartPermit> {
        self.try_begin_restart_with_cooldown(DRAIN_COOLDOWN)
    }

    fn try_begin_restart_with_cooldown(&self, cooldown: Duration) -> Option<RestartPermit> {
        let mut state = self.inner.state.write().ok()?;
        if !state.accepting_jobs || state.active_jobs > 0 || state.active_requests > 0 {
            return None;
        }
        if state
            .last_completed_at
            .is_some_and(|finished| finished.elapsed() < cooldown)
        {
            return None;
        }
        state.accepting_jobs = false;
        Some(RestartPermit {
            tracker: Arc::clone(&self.inner),
        })
    }

    /// Reports whether it is currently safe to restart.
    /// Returns false if any print jobs are in flight or if the drain cooldown buffer (8s) has not elapsed.
    pub fn is_restart_safe(&self) -> bool {
        self.is_restart_safe_with_cooldown(DRAIN_COOLDOWN)
    }

    /// Reports whether it is currently safe to restart given a custom cooldown duration.
    pub fn is_restart_safe_with_cooldown(&self, cooldown: Duration) -> bool {
        let state = match self.inner.state.read() {
            Ok(s) => s,
            Err(_) => return false,
        };
        if state.active_jobs > 0 || state.active_requests > 0 {
            return false;
        }
        if let Some(completed_at) = state.last_completed_at {
            if completed_at.elapsed() < cooldown {
                return false;
            }
        }
        true
    }

    /// Reports whether the application has been continuously idle of print jobs for the quiet window (15m).
    pub fn is_idle_for_quiet_window(&self) -> bool {
        self.is_idle_for(QUIET_WINDOW)
    }

    /// Reports whether the application has been continuously idle of print jobs for the specified duration.
    pub fn is_idle_for(&self, duration: Duration) -> bool {
        let state = match self.inner.state.read() {
            Ok(s) => s,
            Err(_) => return false,
        };
        if state.active_jobs > 0 {
            return false;
        }
        state.last_busy_at.elapsed() >= duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print_job(tracker: &PrintJobTracker) -> RequestLease {
        let mut lease = tracker
            .try_acquire_request()
            .expect("tracker accepts requests");
        assert!(lease.mark_print_job());
        lease
    }

    #[tokio::test]
    async fn lease_increments_and_decrements_atomically_even_on_drop() {
        let tracker = PrintJobTracker::new();
        assert_eq!(tracker.active_count(), 0);
        assert!(tracker.is_restart_safe());

        let lease1 = print_job(&tracker);
        assert_eq!(tracker.active_count(), 1);
        assert!(!tracker.is_restart_safe());

        {
            let _lease2 = print_job(&tracker);
            assert_eq!(tracker.active_count(), 2);
            assert!(!tracker.is_restart_safe());
        }

        // After lease2 drops
        assert_eq!(tracker.active_count(), 1);
        assert!(!tracker.is_restart_safe());

        drop(lease1);
        assert_eq!(tracker.active_count(), 0);
    }

    #[tokio::test]
    async fn drain_cooldown_holds_restart_readiness() {
        let tracker = PrintJobTracker::new();
        let short_cooldown = Duration::from_millis(50);

        let lease = print_job(&tracker);
        assert!(!tracker.is_restart_safe_with_cooldown(short_cooldown));

        drop(lease);
        // Active count is 0, but cooldown has not passed yet
        assert_eq!(tracker.active_count(), 0);
        assert!(!tracker.is_restart_safe_with_cooldown(short_cooldown));

        tokio::time::sleep(short_cooldown + Duration::from_millis(10)).await;
        // After cooldown passes
        assert!(tracker.is_restart_safe_with_cooldown(short_cooldown));
    }

    #[tokio::test]
    async fn restart_permit_closes_job_admission_until_released() {
        let tracker = PrintJobTracker::new();
        let short_cooldown = Duration::from_millis(20);
        let mut lease = tracker
            .try_acquire_request()
            .expect("tracker accepts requests");
        assert_eq!(tracker.active_count(), 0);
        assert!(!tracker.is_restart_safe());
        assert!(tracker
            .try_begin_restart_with_cooldown(short_cooldown)
            .is_none());
        assert!(lease.mark_print_job());
        assert_eq!(tracker.active_count(), 1);
        drop(lease);
        assert!(tracker
            .try_begin_restart_with_cooldown(short_cooldown)
            .is_none());
        tokio::time::sleep(short_cooldown + Duration::from_millis(5)).await;

        let permit = tracker
            .try_begin_restart_with_cooldown(short_cooldown)
            .expect("drain guard is clear");
        assert!(tracker.try_acquire_request().is_none());
        drop(permit);
        assert!(tracker.try_acquire_request().is_some());
    }

    #[tokio::test]
    async fn provisional_ipp_requests_do_not_reset_print_job_idle_time() {
        let tracker = PrintJobTracker::new();
        let short_window = Duration::from_millis(20);
        tokio::time::sleep(short_window + Duration::from_millis(5)).await;
        let lease = tracker
            .try_acquire_request()
            .expect("tracker accepts requests");
        assert!(tracker.is_idle_for(short_window));
        assert!(!tracker.is_restart_safe());
        drop(lease);
        assert!(tracker.is_idle_for(short_window));
    }

    #[tokio::test]
    async fn idle_duration_tracking() {
        let tracker = PrintJobTracker::new();
        let short_window = Duration::from_millis(30);

        // Sleep to pass short window
        tokio::time::sleep(short_window + Duration::from_millis(5)).await;
        assert!(tracker.is_idle_for(short_window));

        // Starting a job resets the idle window
        let lease = print_job(&tracker);
        assert!(!tracker.is_idle_for(short_window));

        drop(lease);
        // Right after drop, idle window is reset to now
        assert!(!tracker.is_idle_for(short_window));

        tokio::time::sleep(short_window + Duration::from_millis(5)).await;
        assert!(tracker.is_idle_for(short_window));
    }
}
