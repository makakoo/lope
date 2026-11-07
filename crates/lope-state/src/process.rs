//! Process identity and resources (`jobs.py:62-219`). PID alone is never
//! identity: a process is "the same" only if its start fingerprint matches.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

pub fn sha256_text(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn ps_bin() -> PathBuf {
    lope_core::paths::which("ps").unwrap_or_else(|| {
        if std::path::Path::new("/bin/ps").is_file() {
            PathBuf::from("/bin/ps")
        } else {
            PathBuf::from("/usr/bin/ps")
        }
    })
}

/// Run a command with a timeout; returns (exit ok, stdout) or None on
/// spawn failure / timeout.
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Option<(bool, String)> {
    let child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) => Some((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned())),
        Ok(Err(_)) => None,
        Err(_) => {
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
            None
        }
    }
}

/// `process_start_fingerprint(pid)`: `proc:<starttime>` on Linux, else
/// `ps:<lstart, whitespace-normalised>`.
pub fn start_fingerprint(pid: i64) -> Option<String> {
    start_fingerprint_timeout(pid, Duration::from_secs(2))
}

/// `process_start_fingerprint(pid, timeout=...)`.
pub fn start_fingerprint_timeout(pid: i64, timeout: Duration) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    if let Ok(raw) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        if let Some((_, tail)) = raw.rsplit_once(')') {
            if let Some(v) = tail.split_whitespace().nth(19) {
                return Some(format!("proc:{v}"));
            }
        }
    }
    let (ok, out) = run_with_timeout(Command::new(ps_bin()).args(["-p", &pid.to_string(), "-o", "lstart="]), timeout)?;
    let value = out.trim();
    if ok && !value.is_empty() {
        Some(format!("ps:{}", value.split_whitespace().collect::<Vec<_>>().join(" ")))
    } else {
        None
    }
}

/// `process_state(pid)`: `ps -o stat=`, "" if unknown.
pub fn state(pid: i64) -> String {
    if pid <= 0 {
        return String::new();
    }
    run_with_timeout(Command::new(ps_bin()).args(["-p", &pid.to_string(), "-o", "stat="]), Duration::from_secs(2))
        .map(|(_, out)| out.trim().to_string())
        .unwrap_or_default()
}

pub fn hostname() -> String {
    let mut buf = [0u8; 256];
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc != 0 {
        return String::new();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// `boot_fingerprint()`: sha256("hostname|boot identity"). Cached per process
/// (Python recomputes it on every call; the value cannot change in between).
pub fn boot_fingerprint() -> String {
    static FP: OnceLock<String> = OnceLock::new();
    FP.get_or_init(|| {
        let mut boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if boot.is_empty() {
            boot = start_fingerprint(1).unwrap_or_else(python_platform);
        }
        sha256_text(&format!("{}|{}", hostname(), boot))
    })
    .clone()
}

/// Fallback for `platform.platform()` (only reached if `ps` fails for pid 1).
fn python_platform() -> String {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    unsafe { libc::uname(&mut u) };
    let f = |a: &[libc::c_char]| unsafe { std::ffi::CStr::from_ptr(a.as_ptr()) }.to_string_lossy().into_owned();
    format!("{}-{}-{}", f(&u.sysname), f(&u.release), f(&u.machine))
}

/// `process_identity(pid)`.
pub fn identity(pid: i64) -> Value {
    json!({ "pid": pid, "start_fingerprint": start_fingerprint(pid) })
}

/// `process_identity(pid, timeout=...)` (the supervisor uses 0.3 s).
pub fn identity_timeout(pid: i64, timeout: Duration) -> Value {
    json!({ "pid": pid, "start_fingerprint": start_fingerprint_timeout(pid, timeout) })
}

fn as_pid(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => lope_core::config::py_int_str(s),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

/// `identity_matches(identity)`.
pub fn identity_matches(identity: Option<&Value>) -> bool {
    let Some(Value::Object(id)) = identity else {
        return false;
    };
    let Some(pid) = as_pid(id.get("pid").or(Some(&json!(0)))) else {
        return false;
    };
    let expected = match id.get("start_fingerprint") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => return false,
    };
    if pid <= 0 {
        return false;
    }
    let st = state(pid);
    if st.is_empty() || st.starts_with('Z') {
        return false;
    }
    start_fingerprint(pid).as_deref() == Some(expected.as_str())
}

/// Python `round(x, 3)`.
fn round3(x: f64) -> f64 {
    format!("{x:.3}").parse().unwrap_or(x)
}

/// `process_resources(manifest)`.
pub fn resources(manifest: &Value) -> Value {
    let mut identities: Vec<Option<&Value>> = vec![manifest.get("owner")];
    let mut pgids = std::collections::BTreeSet::new();
    if let Some(Value::Object(calls)) = manifest.get("calls") {
        for call in calls.values() {
            identities.push(call.get("supervisor"));
            identities.push(call.get("child"));
            if identity_matches(call.get("child")) {
                let pgid = match call.get("pgid") {
                    None | Some(Value::Null) => Some(0),
                    other => as_pid(other),
                };
                if let Some(p) = pgid.filter(|p| *p > 0) {
                    pgids.insert(p);
                }
            }
        }
    }
    let owned: std::collections::BTreeSet<i64> = identities
        .into_iter()
        .filter(|id| identity_matches(*id))
        .filter_map(|id| as_pid(id.and_then(|v| v.get("pid"))))
        .collect();
    let mut rows: std::collections::BTreeMap<i64, (f64, f64)> = Default::default();
    match run_with_timeout(Command::new(ps_bin()).args(["-axo", "pid=,pgid=,%cpu=,rss="]), Duration::from_secs(2)) {
        Some((_, out)) => {
            for line in out.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() != 4 {
                    continue;
                }
                let (Ok(pid), Ok(pgid), Ok(cpu), Ok(rss)) = (
                    parts[0].parse::<i64>(),
                    parts[1].parse::<i64>(),
                    parts[2].parse::<f64>(),
                    parts[3].parse::<f64>(),
                ) else {
                    continue;
                };
                if owned.contains(&pid) || pgids.contains(&pgid) {
                    rows.insert(pid, (cpu, rss * 1024.0));
                }
            }
        }
        None => {
            for p in &owned {
                rows.insert(*p, (0.0, 0.0));
            }
        }
    }
    let cpu: f64 = rows.values().map(|r| r.0).sum();
    let rss: f64 = rows.values().map(|r| r.1).sum();
    json!({
        "process_count": rows.len(),
        // round(sum([]), 3) is the int 0 in Python; any row makes it a float.
        "cpu_percent": if rows.is_empty() { json!(0) } else { json!(round3(cpu)) },
        "rss_bytes": rss as i64,
        "pids": rows.keys().collect::<Vec<_>>(),
        "available": !rows.is_empty() || owned.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_identity_matches_and_dead_pid_does_not() {
        let me = identity(std::process::id() as i64);
        assert!(me["start_fingerprint"].as_str().unwrap().starts_with("ps:") || cfg!(target_os = "linux"));
        assert!(identity_matches(Some(&me)));
        let bogus = json!({"pid": std::process::id(), "start_fingerprint": "ps:Thu Jan  1 00:00:00 1970"});
        assert!(!identity_matches(Some(&bogus)));
        assert!(!identity_matches(Some(&json!({"pid": 0, "start_fingerprint": "x"}))));
        assert!(!identity_matches(None));
    }

    #[test]
    fn round3_is_python_round() {
        assert_eq!(round3(0.0005), 0.001); // binary 0.0005 is slightly above the tie
        assert_eq!(round3(1.23449), 1.234);
    }
}
