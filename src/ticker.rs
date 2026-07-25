//! Thread-queue timer used to drive polling.
//!
//! The subtlety this type exists to contain: `SetTimer` with a NULL `hWnd`
//! *ignores* the `nIDEvent` you pass and generates its own ID, which it
//! returns. `WM_TIMER`'s `wParam` then carries that generated ID, not the one
//! you asked for. Comparing `wParam` against a hand-picked constant silently
//! never matches, and the timer looks dead.
//!
//! From the SetTimer docs:
//!   "If the hWnd parameter is NULL, and the nIDEvent does not match an
//!    existing timer then it is ignored and a new timer ID is generated."
//!   "If the call is not intended to replace an existing timer, nIDEvent
//!    should be 0 if the hWnd is NULL."

use windows::Win32::UI::WindowsAndMessaging::{KillTimer, MSG, SetTimer, WM_TIMER};

pub struct Ticker {
    id: usize,
}

impl Ticker {
    /// Starts a thread-queue timer. Returns None if the timer cannot be created.
    pub fn start(interval_ms: u32) -> Option<Ticker> {
        // nIDEvent must be 0: we are creating, not replacing.
        let id = unsafe { SetTimer(None, 0, interval_ms, None) };
        if id == 0 { None } else { Some(Ticker { id }) }
    }

    /// The system-generated timer ID that WM_TIMER will carry in wParam.
    pub fn id(&self) -> usize {
        self.id
    }

    /// True when this message is our timer firing.
    pub fn is_tick(&self, msg: &MSG) -> bool {
        msg.message == WM_TIMER && msg.wParam.0 == self.id
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(None, self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{LPARAM, WPARAM};

    fn msg(message: u32, wparam: usize) -> MSG {
        MSG {
            message,
            wParam: WPARAM(wparam),
            lParam: LPARAM(0),
            ..Default::default()
        }
    }

    #[test]
    fn start_yields_a_nonzero_generated_id() {
        let t = Ticker::start(60_000).expect("timer should start");
        assert_ne!(t.id(), 0);
    }

    #[test]
    fn matches_wm_timer_carrying_the_generated_id() {
        let t = Ticker::start(60_000).expect("timer should start");
        assert!(t.is_tick(&msg(WM_TIMER, t.id())));
    }

    #[test]
    fn ignores_other_messages_and_other_timers() {
        let t = Ticker::start(60_000).expect("timer should start");
        assert!(!t.is_tick(&msg(WM_TIMER, t.id().wrapping_add(1))));
        assert!(!t.is_tick(&msg(0x0113 + 1, t.id())));
    }

    /// The original bug: assuming wParam equals the nIDEvent we passed in.
    /// With a NULL hWnd the system picks its own ID, so a hardcoded 1 would
    /// only match by pure coincidence.
    #[test]
    fn generated_id_is_not_assumed_to_be_the_requested_one() {
        let t = Ticker::start(60_000).expect("timer should start");
        if t.id() != 1 {
            assert!(
                !t.is_tick(&msg(WM_TIMER, 1)),
                "a hardcoded ID must not match the generated timer"
            );
        }
    }

    #[test]
    fn two_tickers_get_distinct_ids() {
        let a = Ticker::start(60_000).expect("a");
        let b = Ticker::start(60_000).expect("b");
        assert_ne!(a.id(), b.id());
        assert!(!a.is_tick(&msg(WM_TIMER, b.id())));
    }
}
