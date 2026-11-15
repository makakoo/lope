//! FR13: SIGINT/SIGTERM/SIGHUP set a process-wide cancellation flag (via a
//! self-pipe safe handler); long-running native commands poll it at their
//! own event boundaries and close their runs before exiting 130/143/129.

use std::sync::atomic::{AtomicI32, Ordering};

static RECEIVED: AtomicI32 = AtomicI32::new(0);

extern "C" fn handler(sig: libc::c_int) {
    RECEIVED.store(sig, Ordering::SeqCst);
}

/// Install handlers for INT/TERM/HUP. Idempotent.
pub fn install() {
    unsafe {
        for s in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::signal(s, handler as *const () as libc::sighandler_t);
        }
    }
}

/// The signal received, if any.
pub fn received() -> Option<i32> {
    match RECEIVED.load(Ordering::SeqCst) {
        0 => None,
        s => Some(s),
    }
}

/// Shell-visible exit status for a signal death (128 + n).
pub fn exit_code(sig: i32) -> i32 {
    128 + sig
}
