//! macOS: no procfs. Group membership is observed with `killpg(pgid, 0)`
//! only; Python returns "present" here too (`supervisor.py:117-120`), because
//! `ps` startup can exceed the whole cleanup budget. Zombies awaiting
//! launchd's reaping still count as members, so confirmation may take a few
//! scheduler ticks under load: that is what the cleanup budget absorbs.

/// Leader state as `ps -o stat=` prints it (for FR12 diagnostics).
pub fn leader_state(pid: i32) -> String {
    lope_state::process::state(pid as i64)
}
