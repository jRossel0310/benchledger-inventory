//! Close-time publish glue (Phase 6 Task 6): the decision logic behind
//! `main.rs`'s `on_window_event` close interception.
//!
//! The design keeps the Rust side deliberately THIN — the frontend owns the
//! whole close-flow state machine (`features/shell/ClosePublishDialog.tsx`).
//! Rust contributes exactly three things:
//!
//! 1. On EVERY `WindowEvent::CloseRequested`: `api.prevent_close()` and
//!    emit `close-publish-requested` to the window. Re-emitting on repeat
//!    close requests is deliberate and safe — the frontend's
//!    `flowActiveRef` guard makes duplicate events no-ops (covered by its
//!    "ignores a duplicate close event" test) — and it is what rescues the
//!    case where the FIRST emit was swallowed: Tauri's `emit` returns `Ok`
//!    even with zero listeners, so a close during startup (listener not
//!    registered yet), after a React crash, or after a rejected `listen()`
//!    would otherwise strand the user in a window whose close requests are
//!    all prevented with nothing ever emitted.
//! 2. A wedged-frontend escape hatch: if a close flow started more than
//!    [`WEDGED_FRONTEND_GRACE`] ago and another close request arrives,
//!    exit the process directly instead of re-emitting. A webview that has
//!    been unable to drive the flow to completion for that long (crashed
//!    renderer, hung event loop) must not be able to trap the user; and a
//!    HEALTHY frontend past the grace period is showing the Retry/Close
//!    anyway dialog, where another click on X reads as "close anyway" —
//!    the pending-publish marker is already set before any upload starts
//!    (`inventory_sync::publish`), so exiting loses nothing.
//! 3. The `finalize_close` command (`commands.rs`): `app.exit(0)`. Exiting
//!    through `AppHandle::exit` never raises another `CloseRequested`, so
//!    the guard needs no "finalizing" state — once the frontend decides to
//!    close, the close-requested path is simply bypassed.
//!
//! The one failure mode this must never have: trapping the user in an app
//! that cannot close. `main.rs` therefore also exits directly if an emit
//! itself fails (a dead webview can't run the dialog that would otherwise
//! call `finalize_close`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long after the FIRST close request further close requests keep
/// re-emitting before they force-exit instead. Measured from the first
/// request (never reset by later ones), so a wedged frontend cannot extend
/// its own deadline. 30s comfortably covers the dialog's whole happy path
/// (its publish timeout is 20s) while keeping the worst-case "user stuck
/// clicking X at a dead webview" under half a minute.
pub const WEDGED_FRONTEND_GRACE: Duration = Duration::from_secs(30);

/// What `main.rs` should do with a close request. `First` and `ReEmit` both
/// emit `close-publish-requested` (the split exists for logging/tests);
/// `ForceExit` exits the process directly — see the module doc's escape
/// hatch rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    /// This request started the close flow.
    First,
    /// A flow is already active and started within [`WEDGED_FRONTEND_GRACE`];
    /// emit again (a no-op for a healthy frontend, a rescue for one whose
    /// first event was swallowed).
    ReEmit,
    /// A flow started more than [`WEDGED_FRONTEND_GRACE`] ago and still has
    /// not closed the app: stop trusting the frontend and exit.
    ForceExit,
}

/// Records when the first close request arrived. Never reset: the only exit
/// from the close flow is process exit, so a stale timestamp can't outlive
/// the flow it describes.
pub struct CloseFlowGuard {
    started_at: Mutex<Option<Instant>>,
}

/// The process-wide guard `main.rs` consults.
pub static CLOSE_FLOW: CloseFlowGuard = CloseFlowGuard::new();

impl CloseFlowGuard {
    pub const fn new() -> Self {
        Self {
            started_at: Mutex::new(None),
        }
    }

    /// Classify a close request arriving at `now` (injected so tests
    /// control the clock; production passes `Instant::now()`). The first
    /// call records `now` and returns [`CloseDecision::First`]; later calls
    /// return [`CloseDecision::ReEmit`] until `now` is at least
    /// [`WEDGED_FRONTEND_GRACE`] past the recorded start, then
    /// [`CloseDecision::ForceExit`].
    pub fn begin_or_elapsed(&self, now: Instant) -> CloseDecision {
        let mut started_at = self.started_at.lock().expect("close flow lock poisoned");
        match *started_at {
            None => {
                *started_at = Some(now);
                CloseDecision::First
            }
            // `Instant::duration_since` saturates to zero for an earlier
            // `now`, so a caller-supplied out-of-order timestamp degrades
            // to ReEmit rather than panicking.
            Some(start) if now.duration_since(start) >= WEDGED_FRONTEND_GRACE => {
                CloseDecision::ForceExit
            }
            Some(_) => CloseDecision::ReEmit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_close_request_starts_the_flow() {
        let guard = CloseFlowGuard::new();
        assert_eq!(guard.begin_or_elapsed(Instant::now()), CloseDecision::First);
    }

    #[test]
    fn repeat_requests_within_the_grace_period_re_emit() {
        let guard = CloseFlowGuard::new();
        let t0 = Instant::now();
        assert_eq!(guard.begin_or_elapsed(t0), CloseDecision::First);
        assert_eq!(guard.begin_or_elapsed(t0), CloseDecision::ReEmit);
        assert_eq!(
            guard.begin_or_elapsed(t0 + WEDGED_FRONTEND_GRACE - Duration::from_millis(1)),
            CloseDecision::ReEmit
        );
    }

    #[test]
    fn a_request_at_or_past_the_grace_period_forces_exit() {
        let guard = CloseFlowGuard::new();
        let t0 = Instant::now();
        assert_eq!(guard.begin_or_elapsed(t0), CloseDecision::First);
        assert_eq!(
            guard.begin_or_elapsed(t0 + WEDGED_FRONTEND_GRACE),
            CloseDecision::ForceExit
        );
        assert_eq!(
            guard.begin_or_elapsed(t0 + WEDGED_FRONTEND_GRACE + Duration::from_secs(60)),
            CloseDecision::ForceExit
        );
    }

    #[test]
    fn the_grace_period_is_measured_from_the_first_request_only() {
        // Re-emitting close requests must not push the deadline out: the
        // clock runs from the FIRST request, so a wedged frontend being
        // clicked at repeatedly still force-exits on schedule.
        let guard = CloseFlowGuard::new();
        let t0 = Instant::now();
        assert_eq!(guard.begin_or_elapsed(t0), CloseDecision::First);
        assert_eq!(
            guard.begin_or_elapsed(t0 + Duration::from_secs(29)),
            CloseDecision::ReEmit
        );
        assert_eq!(
            guard.begin_or_elapsed(t0 + Duration::from_secs(31)),
            CloseDecision::ForceExit
        );
    }
}
