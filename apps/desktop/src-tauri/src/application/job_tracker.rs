//! In-flight print job tracking and safe restart drain guard (Issue #55).
//!
//! Tracks active in-flight Print jobs across both the local Client proxy and the Server IPPS
//! sharing endpoint, providing a drain guard that prevents update restarts or lifecycle interruptions
//! from cutting off documents mid-transfer or while the Windows spooler is flushing data.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::Instant;

/// Drain cooldown buffer holding restart readiness after the last in-flight job completes (8 seconds).
pub const DRAIN_COOLDOWN: Duration = Duration::from_secs(8);

/// Quiet window of zero print jobs required for background auto-updates (15 minutes).
pub const QUIET_WINDOW: Duration = Duration::from_secs(15 * 60);

/// An RAII lease that tracks an active print job from request arrival until completion or drop.
#[derive(Debug)]
pub struct JobLease {
    tracker: Arc<PrintJobTrackerInner>,
}

impl Drop for JobLease {
    fn drop(&mut self) {
        self.tracker.release();
    }
}

#[derive(Debug)]
struct TrackerState {
    last_completed_at: Option<Instant>,
    last_busy_at: Instant,
}

#[derive(Debug)]
struct PrintJobTrackerInner {
    active_jobs: AtomicUsize,
    state: RwLock<TrackerState>,
}

impl PrintJobTrackerInner {
    fn release(&self) {
        let prev = self.active_jobs.fetch_sub(1, Ordering::SeqCst);
        if prev == 1 {
            // Last active job just completed or dropped
            let now = Instant::now();
            if let Ok(mut state) = self.state.write() {
                state.last_completed_at = Some(now);
                state.last_busy_at = now;
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
                active_jobs: AtomicUsize::new(0),
                state: RwLock::new(TrackerState {
                    last_completed_at: None,
                    last_busy_at: now,
                }),
            }),
        }
    }

    /// Number of active print jobs currently in flight.
    pub fn active_count(&self) -> usize {
        self.inner.active_jobs.load(Ordering::SeqCst)
    }

    /// Acquires a lease for a newly arrived print job.
    /// The lease automatically decrements active jobs and updates completion timestamp on drop.
    pub fn acquire_job(&self) -> JobLease {
        self.inner.active_jobs.fetch_add(1, Ordering::SeqCst);
        JobLease {
            tracker: Arc::clone(&self.inner),
        }
    }

    /// Reports whether it is currently safe to restart.
    /// Returns false if any print jobs are in flight or if the drain cooldown buffer (8s) has not elapsed.
    pub fn is_restart_safe(&self) -> bool {
        self.is_restart_safe_with_cooldown(DRAIN_COOLDOWN)
    }

    /// Reports whether it is currently safe to restart given a custom cooldown duration.
    pub fn is_restart_safe_with_cooldown(&self, cooldown: Duration) -> bool {
        if self.active_count() > 0 {
            return false;
        }
        if let Ok(state) = self.inner.state.read() {
            if let Some(completed_at) = state.last_completed_at {
                if completed_at.elapsed() < cooldown {
                    return false;
                }
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
        if self.active_count() > 0 {
            return false;
        }
        if let Ok(state) = self.inner.state.read() {
            state.last_busy_at.elapsed() >= duration
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lease_increments_and_decrements_atomically_even_on_drop() {
        let tracker = PrintJobTracker::new();
        assert_eq!(tracker.active_count(), 0);
        assert!(tracker.is_restart_safe());

        let lease1 = tracker.acquire_job();
        assert_eq!(tracker.active_count(), 1);
        assert!(!tracker.is_restart_safe());

        {
            let _lease2 = tracker.acquire_job();
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

        let lease = tracker.acquire_job();
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
    async fn idle_duration_tracking() {
        let tracker = PrintJobTracker::new();
        let short_window = Duration::from_millis(30);

        // Sleep to pass short window
        tokio::time::sleep(short_window + Duration::from_millis(5)).await;
        assert!(tracker.is_idle_for(short_window));

        // Starting a job resets the idle window
        let lease = tracker.acquire_job();
        assert!(!tracker.is_idle_for(short_window));

        drop(lease);
        // Right after drop, idle window is reset to now
        assert!(!tracker.is_idle_for(short_window));

        tokio::time::sleep(short_window + Duration::from_millis(5)).await;
        assert!(tracker.is_idle_for(short_window));
    }
}
