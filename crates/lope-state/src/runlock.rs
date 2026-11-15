//! `runlock.py`: one process-level flock for commands that spawn validators.
//! Fail fast with exit 75 (EX_TEMPFAIL) by default; `LOPE_RUN_LOCK_WAIT`
//! queues; `LOPE_RUN_LOCK=off` disables.

use crate::process::identity_matches;
use crate::registry::RunRegistry;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub fn lockfile_path() -> PathBuf {
    if let Some(p) = std::env::var_os("LOPE_RUN_LOCK_PATH").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    // runlock.py: Path(os.environ.get("LOPE_HOME") or Path.home() / ".lope") (no expanduser)
    match std::env::var_os("LOPE_HOME").filter(|v| !v.is_empty()) {
        Some(h) => PathBuf::from(h).join("run.lock"),
        None => lope_core::paths::home_dir().join(".lope").join("run.lock"),
    }
}

fn read_holder(path: &std::path::Path) -> String {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn holder_run_ids(holder: &str) -> Vec<String> {
    let Some(pid) = holder.split_whitespace().next().and_then(lope_core::config::py_int_str) else {
        return vec![];
    };
    let Ok(reg) = RunRegistry::open_default() else {
        return vec![];
    };
    reg.list_active(false)
        .into_iter()
        .filter(|row| {
            let owner_pid = row.get("owner").and_then(|o| o.get("pid")).and_then(|p| p.as_i64()).unwrap_or(0);
            row.get("run_id").is_some_and(|r| !r.is_null()) && owner_pid == pid && identity_matches(row.get("owner"))
        })
        .filter_map(|row| row.get("run_id").map(lope_core::config::py_str))
        .collect()
}

fn job_advice(holder: &str) -> String {
    let ids = holder_run_ids(holder);
    if ids.is_empty() {
        return "    lope jobs list".into();
    }
    let mut lines = vec![format!("    lope jobs list  # owner run: {}", ids.join(", "))];
    lines.extend(ids.iter().map(|id| format!("    lope jobs kill {id}")));
    lines.join("\n")
}

/// Holds the run lock until dropped.
pub struct RunLock {
    file: Option<std::fs::File>,
}

impl Drop for RunLock {
    fn drop(&mut self) {
        if let Some(f) = &self.file {
            let _ = f.set_len(0);
            unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

/// Outcome when the lock is busy: the stderr text Python prints and exit 75.
#[derive(Debug)]
pub struct LockBusy {
    pub message: String,
}

pub const EXIT_TEMPFAIL: i32 = 75;

/// `acquire(command)`.
pub fn acquire(command: &str) -> Result<RunLock, LockBusy> {
    if std::env::var("LOPE_RUN_LOCK").map(|v| v.to_lowercase() == "off").unwrap_or(false) {
        return Ok(RunLock { file: None });
    }
    let path = lockfile_path();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .map_err(|e| LockBusy { message: e.to_string() })?;
    let wait_env = std::env::var("LOPE_RUN_LOCK_WAIT").ok();
    let deadline = wait_env.as_ref().and_then(|w| {
        let secs = w.trim().parse::<f64>().unwrap_or(0.0);
        (secs != 0.0).then(|| Instant::now() + Duration::from_secs_f64(secs.max(0.0)))
    });
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break;
        }
        let holder = read_holder(&path);
        match &wait_env {
            None => {
                return Err(LockBusy {
                    message: format!(
                        "\nlope {command}: another lope run is already active (holder: {holder}).\n  Inspect the recorded run safely:\n{}\n  `jobs kill` signals only the exact fingerprint-verified run ID.\n  To queue instead of failing, set LOPE_RUN_LOCK_WAIT=0 (wait forever) or LOPE_RUN_LOCK_WAIT=300 (5 min).\n  To disable locking entirely: LOPE_RUN_LOCK=off\n",
                        job_advice(&holder)
                    ),
                })
            }
            Some(w) => {
                if deadline.is_some_and(|d| Instant::now() >= d) {
                    return Err(LockBusy {
                        message: format!(
                            "\nlope {command}: timed out waiting for run lock after {w}s (holder: {holder}).\n  Inspect or cancel only by verified run ID:\n{}\n",
                            job_advice(&holder)
                        ),
                    });
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    let _ = file.set_len(0);
    let mut f = &file;
    let _ = f.write_all(format!("{} {command}\n", std::process::id()).as_bytes());
    let _ = file.sync_all();
    Ok(RunLock { file: Some(file) })
}
