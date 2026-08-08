//! Sliding-window rate limits for same-UID IPC spam (Phase 18 B).
//!
//! Pure logic — no Wayland. Used by the compositor (socket + event subscribe),
//! protocol command-file writers, and shell command-file pollers.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Sliding window length for all Phase 18 B budgets.
pub const RATE_WINDOW: Duration = Duration::from_secs(1);

/// Global compositor command-IPC admits per window.
pub const IPC_REQUESTS_PER_SEC: usize = 120;

/// Max peer-accepted streams processed in one `drain_ipc` tick.
pub const IPC_MAX_ACCEPTS_PER_DRAIN: usize = 32;

/// Max long-lived event-bus subscribers.
pub const EVENT_SUBSCRIBER_CAP: usize = 16;

/// Event-socket subscribe accepts per window.
pub const EVENT_SUBSCRIBE_ACCEPTS_PER_SEC: usize = 16;

/// `write_runtime_command*` admits per window (per process).
pub const RUNTIME_CMD_WRITES_PER_SEC: usize = 60;

/// Shell command-file dispatches per window (per file / channel).
pub const RUNTIME_CMD_DISPATCH_PER_SEC: usize = 30;

/// Fixed-size sliding window of admit timestamps.
#[derive(Debug, Clone)]
pub struct SlidingWindow {
    window: Duration,
    limit: usize,
    admits: VecDeque<Instant>,
}

impl SlidingWindow {
    pub fn new(window: Duration, limit: usize) -> Self {
        Self {
            window,
            limit,
            admits: VecDeque::with_capacity(limit.saturating_add(1)),
        }
    }

    pub fn ipc_requests() -> Self {
        Self::new(RATE_WINDOW, IPC_REQUESTS_PER_SEC)
    }

    pub fn event_subscribes() -> Self {
        Self::new(RATE_WINDOW, EVENT_SUBSCRIBE_ACCEPTS_PER_SEC)
    }

    /// Admit one event at `now`. Returns `false` when the window is full.
    pub fn try_admit(&mut self, now: Instant) -> bool {
        self.evict(now);
        if self.admits.len() >= self.limit {
            return false;
        }
        self.admits.push_back(now);
        true
    }

    /// Current admits still inside the window (after eviction at `now`).
    pub fn len(&mut self, now: Instant) -> usize {
        self.evict(now);
        self.admits.len()
    }

    pub fn is_empty(&mut self, now: Instant) -> bool {
        self.len(now) == 0
    }

    fn evict(&mut self, now: Instant) {
        while let Some(&front) = self.admits.front() {
            if now.saturating_duration_since(front) >= self.window {
                self.admits.pop_front();
            } else {
                break;
            }
        }
    }
}

fn runtime_write_window() -> &'static Mutex<SlidingWindow> {
    static W: OnceLock<Mutex<SlidingWindow>> = OnceLock::new();
    W.get_or_init(|| {
        Mutex::new(SlidingWindow::new(
            RATE_WINDOW,
            RUNTIME_CMD_WRITES_PER_SEC,
        ))
    })
}

fn runtime_write_widgets_window() -> &'static Mutex<SlidingWindow> {
    static W: OnceLock<Mutex<SlidingWindow>> = OnceLock::new();
    W.get_or_init(|| {
        Mutex::new(SlidingWindow::new(
            RATE_WINDOW,
            RUNTIME_CMD_WRITES_PER_SEC,
        ))
    })
}

fn runtime_dispatch_window() -> &'static Mutex<SlidingWindow> {
    static W: OnceLock<Mutex<SlidingWindow>> = OnceLock::new();
    W.get_or_init(|| {
        Mutex::new(SlidingWindow::new(
            RATE_WINDOW,
            RUNTIME_CMD_DISPATCH_PER_SEC,
        ))
    })
}

fn runtime_dispatch_widgets_window() -> &'static Mutex<SlidingWindow> {
    static W: OnceLock<Mutex<SlidingWindow>> = OnceLock::new();
    W.get_or_init(|| {
        Mutex::new(SlidingWindow::new(
            RATE_WINDOW,
            RUNTIME_CMD_DISPATCH_PER_SEC,
        ))
    })
}

fn try_admit_locked(lock: &Mutex<SlidingWindow>) -> bool {
    match lock.lock() {
        Ok(mut w) => w.try_admit(Instant::now()),
        // Fail open if poisoned so a panic elsewhere cannot wedge the session.
        Err(poisoned) => poisoned.into_inner().try_admit(Instant::now()),
    }
}

/// Rate-limit bar `command` file writes in this process.
pub fn try_admit_runtime_command_write() -> bool {
    try_admit_locked(runtime_write_window())
}

/// Rate-limit `command-widgets` file writes in this process.
pub fn try_admit_runtime_command_widgets_write() -> bool {
    try_admit_locked(runtime_write_widgets_window())
}

/// Rate-limit bar command-file dispatch in the shell poller.
pub fn try_admit_runtime_command_dispatch() -> bool {
    try_admit_locked(runtime_dispatch_window())
}

/// Rate-limit widgets command-file dispatch in the widgets process.
pub fn try_admit_runtime_command_widgets_dispatch() -> bool {
    try_admit_locked(runtime_dispatch_widgets_window())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_under_limit() {
        let mut w = SlidingWindow::new(Duration::from_secs(1), 3);
        let t0 = Instant::now();
        assert!(w.try_admit(t0));
        assert!(w.try_admit(t0 + Duration::from_millis(10)));
        assert!(w.try_admit(t0 + Duration::from_millis(20)));
        assert_eq!(w.len(t0 + Duration::from_millis(20)), 3);
    }

    #[test]
    fn rejects_when_full() {
        let mut w = SlidingWindow::new(Duration::from_secs(1), 2);
        let t0 = Instant::now();
        assert!(w.try_admit(t0));
        assert!(w.try_admit(t0));
        assert!(!w.try_admit(t0 + Duration::from_millis(1)));
    }

    #[test]
    fn allows_again_after_window_slides() {
        let mut w = SlidingWindow::new(Duration::from_secs(1), 2);
        let t0 = Instant::now();
        assert!(w.try_admit(t0));
        assert!(w.try_admit(t0 + Duration::from_millis(100)));
        assert!(!w.try_admit(t0 + Duration::from_millis(200)));
        // Both admits expire after 1s from t0 / t0+100ms.
        let later = t0 + Duration::from_secs(1) + Duration::from_millis(150);
        assert!(w.try_admit(later));
        assert_eq!(w.len(later), 1);
    }

    #[test]
    fn exact_limit_edge() {
        let cases = [(1usize, true, false), (5, true, false), (0, false, false)];
        for (limit, first_ok, second_ok) in cases {
            let mut w = SlidingWindow::new(Duration::from_secs(1), limit);
            let t0 = Instant::now();
            assert_eq!(w.try_admit(t0), first_ok, "limit={limit} first");
            if limit > 0 {
                // Fill to limit.
                for _ in 1..limit {
                    assert!(w.try_admit(t0));
                }
                assert_eq!(
                    w.try_admit(t0),
                    second_ok,
                    "limit={limit} over",
                );
                assert_eq!(w.len(t0), limit);
            }
        }
    }

    #[test]
    fn preset_constructors_use_constants() {
        let mut ipc = SlidingWindow::ipc_requests();
        let t0 = Instant::now();
        for _ in 0..IPC_REQUESTS_PER_SEC {
            assert!(ipc.try_admit(t0));
        }
        assert!(!ipc.try_admit(t0));
    }
}
