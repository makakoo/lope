//! OS-specific process-group handling (NFR11). Everything else in the
//! runtime is platform-neutral.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
pub use macos::*;

/// Result of probing a process group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Empty,
    /// Members exist (or exist but cannot be signalled).
    Present,
    /// Inspection failed; never treated as empty.
    Unknown,
}

/// `killpg(pgid, 0)`.
pub fn probe(pgid: i32) -> Group {
    if pgid <= 0 {
        return Group::Empty;
    }
    if unsafe { libc::killpg(pgid, 0) } == 0 {
        return Group::Present;
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Group::Empty,
        Some(libc::EPERM) => Group::Present,
        _ => Group::Unknown,
    }
}

/// `killpg(pgid, sig)`: "process_group", "already_exited" or "signal_failed:…".
pub fn signal_group(pgid: i32, sig: i32) -> String {
    if unsafe { libc::killpg(pgid, sig) } == 0 {
        return "process_group".into();
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::ESRCH) => "already_exited".into(),
        Some(code) => {
            let msg = format!("[Errno {code}] {}", lope_state::registry::strerror(code));
            format!("signal_failed:{}", &msg[..msg.len().min(120)])
        }
        None => "signal_failed:unknown".into(),
    }
}
