//! FR12 timeout diagnostics: a 0600 JSON sidecar per timed-out call under
//! `<registry root>/diag/` (outside `runs/work/<run>` so that Python's
//! closeout, which requires empty work dirs, never sees it).

use lope_core::pyjson::{self, Opts};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub const STDERR_TAIL_BYTES: usize = 4096;
pub const DIAG_RETENTION_SECONDS: f64 = 7.0 * 24.0 * 3600.0;

pub struct TimeoutDiag {
    /// `provider_timeout` or `cancelled` (caller deadline).
    pub kind: String,
    pub timeout_seconds: f64,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub seconds_since_last_byte: Option<f64>,
    pub leader_alive: bool,
    pub leader_state: String,
    pub group_members_present: Option<bool>,
    pub stderr_tail: Vec<u8>,
}

pub fn diag_dir(registry_root: &Path) -> PathBuf {
    registry_root.join("diag")
}

/// Write the sidecar; returns its path. Best effort: errors return None.
pub fn write(registry_root: &Path, run_id: &str, call_id: &str, d: &TimeoutDiag) -> Option<PathBuf> {
    let dir = diag_dir(registry_root);
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir).ok()?;
    let path = dir.join(format!("{run_id}-{call_id}.diag"));
    let tail_start = d.stderr_tail.len().saturating_sub(STDERR_TAIL_BYTES);
    let tail = lope_core::redaction::redact_text(&String::from_utf8_lossy(&d.stderr_tail[tail_start..]));
    let v: Value = json!({
        "schema_version": 1,
        "kind": d.kind,
        "run_id": run_id,
        "call_id": call_id,
        "at": lope_core::time::now(),
        "timeout_seconds": d.timeout_seconds,
        "stdout_bytes": d.stdout_bytes,
        "stderr_bytes": d.stderr_bytes,
        "seconds_since_last_byte": d.seconds_since_last_byte,
        "leader_alive_at_deadline": d.leader_alive,
        "leader_state": d.leader_state,
        "group_members_present": d.group_members_present,
        "stderr_tail_redacted": tail,
    });
    let text = pyjson::dumps(&v, &Opts::pretty_sorted()) + "\n";
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&path).ok()?;
    f.write_all(text.as_bytes()).ok()?;
    Some(path)
}

/// Remove diag files older than the retention window (called by prune).
pub fn prune(registry_root: &Path, max_age: f64) -> usize {
    let Ok(rd) = std::fs::read_dir(diag_dir(registry_root)) else { return 0 };
    let now = std::time::SystemTime::now();
    let mut n = 0;
    for e in rd.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age.as_secs_f64() > max_age);
        if old && e.path().extension().is_some_and(|x| x == "diag") && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}
