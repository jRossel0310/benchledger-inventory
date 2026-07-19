//! Close-time publish glue (Phase 6 Task 6): the re-entry guard behind
//! `main.rs`'s `on_window_event` close interception.
//!
//! The design keeps the Rust side deliberately THIN — the frontend owns the
//! whole close-flow state machine (`features/shell/ClosePublishDialog.tsx`).
//! Rust contributes exactly three things:
//!
//! 1. On the FIRST `WindowEvent::CloseRequested`: `api.prevent_close()` and
//!    emit `close-publish-requested` to the window, handing control to the
//!    frontend dialog.
//! 2. On any FURTHER close request while the flow is active: prevent the
//!    close again and emit nothing — the frontend is mid-flow (publishing,
//!    or showing Retry / Close anyway), and a second event would only
//!    double-drive its state machine.
//! 3. The `finalize_close` command (`commands.rs`): `app.exit(0)`. Exiting
//!    through `AppHandle::exit` never raises another `CloseRequested`, so
//!    the guard needs no "finalizing" escape hatch — once the frontend
//!    decides to close, the close-requested path is simply bypassed.
//!
//! The one failure mode this must never have: trapping the user in an app
//! that cannot close. `main.rs` therefore exits directly if the emit itself
//! fails (a dead webview can't run the dialog that would otherwise call
//! `finalize_close`).

use std::sync::atomic::{AtomicBool, Ordering};

/// Process-wide "a close flow has started" flag. Never reset: the only exit
/// from the close flow is `finalize_close` (process exit), so a stale
/// `true` can't outlive the flow it describes.
pub static CLOSE_FLOW_STARTED: AtomicBool = AtomicBool::new(false);

/// Atomically mark the close flow started. Returns `true` exactly once per
/// flag — the caller that wins the swap is the one that should emit
/// `close-publish-requested`; every later caller (the user clicking X again
/// while the dialog is up) gets `false` and emits nothing.
pub fn begin_close_flow(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_close_request_wins_the_swap() {
        let flag = AtomicBool::new(false);
        assert!(begin_close_flow(&flag));
    }

    #[test]
    fn subsequent_close_requests_do_not_re_emit() {
        let flag = AtomicBool::new(false);
        assert!(begin_close_flow(&flag));
        assert!(!begin_close_flow(&flag));
        assert!(!begin_close_flow(&flag));
    }
}
