//! `RunRegistry` (`jobs.py:220-931`): atomic run/call manifests under
//! `$LOPE_HOME/runs`, liveness classification, safe reaping and closeout.
//! Manifests are written exactly as Python writes them
//! (`json.dump(sort_keys=True, separators=(",", ":"))`, 0600, atomic).

use crate::process::{self, boot_fingerprint, identity_matches};
use lope_core::pyjson::{self, Opts};
use serde_json::{json, Map, Value};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const REGISTRY_SCHEMA_VERSION: i64 = 1;
pub const COMPLETED_RETENTION_SECONDS: f64 = 7.0 * 24.0 * 60.0 * 60.0;
pub const COMPLETED_RETENTION_COUNT: usize = 1000;
pub const STALE_AFTER_SECONDS: f64 = 45.0;

pub const FORBIDDEN_MANIFEST_KEYS: [&str; 10] = [
    "prompt",
    "prompt_text",
    "response",
    "response_body",
    "raw_response",
    "secret",
    "credentials",
    "argv",
    "environment",
    "env",
];

pub const CALL_FIELDS: [&str; 21] = [
    "call_id",
    "validator",
    "stage",
    "state",
    "supervisor",
    "child",
    "pgid",
    "started_at",
    "heartbeat_at",
    "deadline_at",
    "transport",
    "ownership_marker_hash",
    "executable_hash",
    "command_hash",
    "owned_paths",
    "cleanup_result",
    "outcome",
    "ended_at",
    "reason",
    "processes_targeted",
    "platform_cleanup",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError(pub String);

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for RegistryError {}

type R<T> = Result<T, RegistryError>;

fn now() -> f64 {
    lope_core::time::now()
}

fn fnum(f: f64) -> Value {
    serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
}

/// Python `float(x)` for manifest numbers (None/""/0 handled by callers).
fn as_f64(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(*b as i64 as f64),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

fn int_of(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Some(Value::Bool(b)) => *b as i64,
        Some(Value::String(s)) => lope_core::config::py_int_str(s).unwrap_or(0),
        _ => 0,
    }
}

fn validate_manifest_value(value: &Value, path: &str) -> R<()> {
    match value {
        Value::Object(m) => {
            for (k, nested) in m {
                let key = k.to_lowercase();
                if FORBIDDEN_MANIFEST_KEYS.contains(&key.as_str()) {
                    return Err(RegistryError(format!(
                        "manifest field {} is forbidden",
                        py_repr_str(&format!("{path}{key}"))
                    )));
                }
                validate_manifest_value(nested, &format!("{path}{k}."))?;
            }
            Ok(())
        }
        Value::Array(a) => a.iter().try_for_each(|x| validate_manifest_value(x, path)),
        _ => Ok(()),
    }
}

/// Python `repr(str)` for simple strings (used inside error messages).
pub fn py_repr_str(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{s}\"")
    } else {
        format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

fn valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty() && run_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub struct RunRegistry {
    pub root: PathBuf,
    pub active_dir: PathBuf,
    pub completed_dir: PathBuf,
    pub work_dir: PathBuf,
    pub lock_path: PathBuf,
}

pub struct LockGuard(std::fs::File);

impl Drop for LockGuard {
    fn drop(&mut self) {
        unsafe { libc::flock(std::os::unix::io::AsRawFd::as_raw_fd(&self.0), libc::LOCK_UN) };
    }
}

fn mkdir_0700(p: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(p)?;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
    Ok(())
}

impl RunRegistry {
    /// `RunRegistry()` rooted at `lope_home()/runs` (jobs.py `or` semantics).
    pub fn open_default() -> std::io::Result<Self> {
        Self::open(lope_core::paths::lope_home().join("runs"))
    }

    pub fn open(root: PathBuf) -> std::io::Result<Self> {
        let r = RunRegistry {
            active_dir: root.join("active"),
            completed_dir: root.join("completed"),
            work_dir: root.join("work"),
            lock_path: root.join("registry.lock"),
            root,
        };
        for d in [&r.root, &r.active_dir, &r.completed_dir, &r.work_dir] {
            mkdir_0700(d)?;
        }
        Ok(r)
    }

    pub fn locked(&self) -> R<LockGuard> {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&self.lock_path)
            .map_err(|e| RegistryError(e.to_string()))?;
        unsafe { libc::flock(std::os::unix::io::AsRawFd::as_raw_fd(&f), libc::LOCK_EX) };
        Ok(LockGuard(f))
    }

    pub fn active_path(&self, run_id: &str) -> R<PathBuf> {
        if !valid_run_id(run_id) {
            return Err(RegistryError("invalid run_id".into()));
        }
        Ok(self.active_dir.join(format!("{run_id}.json")))
    }

    pub fn run_work_dir(&self, run_id: &str) -> R<PathBuf> {
        self.active_path(run_id)?;
        let p = self.work_dir.join(run_id);
        mkdir_0700(&p).map_err(|e| RegistryError(e.to_string()))?;
        Ok(p)
    }

    /// `_atomic_write`: validate, `.manifest-XXXX.tmp` (0600), fsync, replace.
    pub fn atomic_write(&self, path: &Path, payload: &Value) -> R<()> {
        validate_manifest_value(payload, "")?;
        let dir = path.parent().unwrap_or(Path::new("."));
        mkdir_0700(dir).map_err(|e| RegistryError(e.to_string()))?;
        let text = pyjson::dumps(payload, &Opts::compact_sorted());
        let tmp = dir.join(format!(".manifest-{}.tmp", &uuid::Uuid::new_v4().simple().to_string()[..8]));
        let res = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.flush()?;
            f.sync_all()?;
            std::fs::rename(&tmp, path)
        })();
        if let Err(e) = res {
            let _ = std::fs::remove_file(&tmp);
            return Err(RegistryError(e.to_string()));
        }
        Ok(())
    }

    pub fn read(path: &Path) -> R<Value> {
        let text = std::fs::read(path).map_err(|e| {
            RegistryError(format!("invalid run manifest {}: {}", path.display(), py_os_error(&e, path)))
        })?;
        let text = String::from_utf8_lossy(&text);
        let v = lope_core::pyjson::loads_py(&text)
            .map_err(|e| RegistryError(format!("invalid run manifest {}: {}", path.display(), e)))?;
        if !v.is_object() {
            return Err(RegistryError(format!("invalid run manifest {}: expected object", path.display())));
        }
        Ok(v)
    }

    pub fn start_run(
        &self,
        mode: &str,
        run_id: Option<&str>,
        run_timeout: Option<f64>,
        owner_pid: Option<i64>,
    ) -> R<Value> {
        let t = now();
        let rid = run_id.map(str::to_string).unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let owner = process::identity(owner_pid.unwrap_or(std::process::id() as i64));
        if !truthy(owner.get("start_fingerprint")) {
            return Err(RegistryError("cannot fingerprint run owner".into()));
        }
        let work = self.run_work_dir(&rid)?;
        let manifest = json!({
            "schema_version": REGISTRY_SCHEMA_VERSION,
            "run_id": rid,
            "mode": mode,
            "state": "active",
            "host_boot_fingerprint": boot_fingerprint(),
            "owner": owner,
            "started_at": fnum(t),
            "heartbeat_at": fnum(t),
            "heartbeat_source": "owner",
            "deadline_at": run_timeout.map(|s| fnum(t + s)).unwrap_or(Value::Null),
            "calls": {},
            "call_counters": {"planned": 0, "started": 0, "finished": 0},
            "work_dir": work.display().to_string(),
            "cleanup_history": [],
        });
        let _g = self.locked()?;
        let path = self.active_path(&rid)?;
        if path.exists() {
            return Err(RegistryError(format!("run already exists: {rid}")));
        }
        self.atomic_write(&path, &manifest)?;
        Ok(manifest)
    }

    pub fn load_active(&self, run_id: &str) -> R<Value> {
        Self::read(&self.active_path(run_id)?)
    }

    pub fn update(&self, run_id: &str, mutator: impl FnOnce(&mut Map<String, Value>) -> R<()>) -> R<Value> {
        let _g = self.locked()?;
        let path = self.active_path(run_id)?;
        let mut v = Self::read(&path)?;
        mutator(v.as_object_mut().unwrap())?;
        if v.get("run_id").and_then(Value::as_str) != Some(run_id) {
            return Err(RegistryError("mutator cannot change run_id".into()));
        }
        self.atomic_write(&path, &v)?;
        Ok(v)
    }

    pub fn heartbeat(&self, run_id: &str, source: &str) -> R<Value> {
        self.update(run_id, |m| {
            let t = fnum(now());
            m.insert("heartbeat_at".into(), t.clone());
            m.insert("heartbeat_source".into(), Value::from(source));
            match source {
                "owner" => {
                    m.insert("owner_heartbeat_at".into(), t);
                }
                "supervisor" => {
                    m.insert("supervisor_heartbeat_at".into(), t);
                }
                _ => {}
            }
            Ok(())
        })
    }

    pub fn register_call(&self, run_id: &str, call: Map<String, Value>) -> R<Value> {
        let mut forbidden: Vec<String> =
            call.keys().map(|k| k.to_lowercase()).filter(|k| FORBIDDEN_MANIFEST_KEYS.contains(&k.as_str())).collect();
        if !forbidden.is_empty() {
            forbidden.sort();
            return Err(RegistryError(format!("forbidden call manifest fields: {}", forbidden.join(", "))));
        }
        let mut unknown: Vec<&String> =
            call.keys().filter(|k| !CALL_FIELDS.contains(&k.as_str()) && k.as_str() != "heartbeat_source").collect();
        if !unknown.is_empty() {
            unknown.sort();
            return Err(RegistryError(format!(
                "unsupported call manifest fields: {}",
                unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            )));
        }
        let call_id = match call.get("call_id") {
            Some(v) if truthy(Some(v)) => lope_core::config::py_str(v),
            _ => String::new(),
        };
        if call_id.is_empty() {
            return Err(RegistryError("call_id is required".into()));
        }
        self.update(run_id, move |m| {
            let calls = m.entry("calls").or_insert_with(|| json!({}));
            calls.as_object_mut().unwrap().insert(call_id, Value::Object(call));
            let counters = m.entry("call_counters").or_insert_with(|| json!({}));
            let c = counters.as_object_mut().unwrap();
            let started = int_of(c.get("started")) + 1;
            c.insert("started".into(), json!(started));
            m.insert("heartbeat_at".into(), fnum(now()));
            Ok(())
        })
    }

    pub fn set_planned_calls(&self, run_id: &str, planned: i64) -> R<Value> {
        self.update(run_id, |m| {
            let counters = m.entry("call_counters").or_insert_with(|| json!({}));
            let c = counters.as_object_mut().unwrap();
            let v = int_of(c.get("planned")).max(planned.max(0));
            c.insert("planned".into(), json!(v));
            Ok(())
        })
    }

    pub fn update_call(&self, run_id: &str, call_id: &str, changes: Map<String, Value>) -> R<Value> {
        self.update(run_id, |m| {
            let t = fnum(now());
            let calls = m.entry("calls").or_insert_with(|| json!({}));
            let calls = calls.as_object_mut().unwrap();
            let Some(call) = calls.get_mut(call_id).and_then(Value::as_object_mut) else {
                return Err(RegistryError(format!("unknown call_id: {call_id}")));
            };
            let previous = call.get("state").and_then(Value::as_str).map(str::to_string);
            for (k, v) in &changes {
                call.insert(k.clone(), v.clone());
            }
            call.insert("heartbeat_at".into(), t.clone());
            m.insert("heartbeat_at".into(), t.clone());
            if changes.get("heartbeat_source").and_then(Value::as_str) == Some("supervisor") {
                m.insert("supervisor_heartbeat_at".into(), t);
                m.insert("heartbeat_source".into(), Value::from("supervisor"));
            }
            let new_state = changes.get("state").and_then(Value::as_str);
            let done = |s: Option<&str>| matches!(s, Some("finished" | "completed"));
            if done(new_state) && !done(previous.as_deref()) {
                let counters = m.entry("call_counters").or_insert_with(|| json!({}));
                let c = counters.as_object_mut().unwrap();
                let f = int_of(c.get("finished")) + 1;
                c.insert("finished".into(), json!(f));
            }
            Ok(())
        })
    }

    pub fn finish_run(&self, run_id: &str, state: &str, reason: &str, cleanup_result: &str) -> R<PathBuf> {
        let dest;
        {
            let _g = self.locked()?;
            let active = self.active_path(run_id)?;
            let mut v = Self::read(&active)?;
            let m = v.as_object_mut().unwrap();
            m.insert("state".into(), Value::from(state));
            m.insert("ended_at".into(), fnum(now()));
            m.insert("reason".into(), Value::from(reason));
            m.insert("cleanup_result".into(), Value::from(cleanup_result));
            dest = self.completed_dir.join(format!("{}-{run_id}.json", now() as i64));
            self.atomic_write(&dest, &v)?;
            std::fs::remove_file(&active).map_err(|e| RegistryError(e.to_string()))?;
        }
        self.prune_completed(COMPLETED_RETENTION_SECONDS, COMPLETED_RETENTION_COUNT);
        Ok(dest)
    }

    fn sorted_json_files(dir: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.ends_with(".json") && !n.starts_with('.'))
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// `list_active(include_invalid)` with the computed fields.
    pub fn list_active(&self, include_invalid: bool) -> Vec<Value> {
        let mut out = Vec::new();
        for path in Self::sorted_json_files(&self.active_dir) {
            match Self::read(&path) {
                Ok(mut v) => {
                    let classification = self.classify(&v);
                    let t = now();
                    let started = match v.get("started_at") {
                        Some(x) if truthy(Some(x)) => as_f64(Some(x)).unwrap_or(t),
                        _ => t,
                    };
                    let deadline = v.get("deadline_at").filter(|d| !d.is_null()).and_then(|d| as_f64(Some(d)));
                    let owner_live = identity_matches(v.get("owner"));
                    let resources = process::resources(&v);
                    let m = v.as_object_mut().unwrap();
                    m.insert("classification".into(), Value::from(classification));
                    m.insert("manifest_path".into(), Value::from(path.display().to_string()));
                    m.insert("age_seconds".into(), fnum((t - started).max(0.0)));
                    m.insert("deadline_remaining_seconds".into(), deadline.map(|d| fnum(d - t)).unwrap_or(Value::Null));
                    m.insert("owner_live".into(), Value::Bool(owner_live));
                    m.insert("resources".into(), resources);
                    out.push(v);
                }
                Err(e) => {
                    if include_invalid {
                        out.push(json!({
                            "state": "invalid",
                            "classification": "ownership_unverified",
                            "manifest_path": path.display().to_string(),
                            "reason": e.0,
                            "resources": {"process_count": 0, "cpu_percent": 0.0, "rss_bytes": 0, "pids": [], "available": false},
                        }));
                    }
                }
            }
        }
        out
    }

    pub fn classify(&self, m: &Value) -> &'static str {
        if m.get("host_boot_fingerprint").and_then(Value::as_str) != Some(boot_fingerprint().as_str()) {
            return "abandoned";
        }
        match m.get("cleanup_result") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if s.is_empty() || s == "pending" || s == "clean" => {}
            _ => return "cleanup_failed",
        }
        if !identity_matches(m.get("owner")) {
            return "abandoned";
        }
        let owner_hb = match m.get("owner_heartbeat_at") {
            Some(v) if !v.is_null() => Some(v),
            _ => {
                if m.get("supervisor_heartbeat_at").is_some_and(|v| !v.is_null()) {
                    m.get("started_at")
                } else {
                    m.get("heartbeat_at")
                }
            }
        };
        let hb = if truthy(owner_hb) { as_f64(owner_hb).unwrap_or(0.0) } else { 0.0 };
        if hb != 0.0 && now() - hb > STALE_AFTER_SECONDS {
            return "unresponsive";
        }
        "active"
    }

    /// `is_owned_path`, reproduced exactly, including the symlinked-ancestor
    /// behaviour recorded as finding F1 / PL-5 (S2 decides whether to fix).
    pub fn is_owned_path(&self, run_id: &str, candidate: &str) -> bool {
        let root = resolve_nonstrict(&self.work_dir.join(run_id));
        let path = PathBuf::from(candidate);
        if !path.is_absolute() {
            return false;
        }
        let resolved = resolve_nonstrict(&path);
        if !resolved.starts_with(&root) {
            return false;
        }
        let mut current = path.clone();
        while current != root && current.parent().is_some_and(|p| p != current) {
            if current.exists() && current.is_symlink() {
                return false;
            }
            match current.parent() {
                Some(p) => current = p.to_path_buf(),
                None => break,
            }
        }
        true
    }

    pub fn cleanup_owned_paths(&self, run_id: &str, paths: &[String]) -> Vec<Value> {
        let mut results = Vec::new();
        for raw in paths {
            if !self.is_owned_path(run_id, raw) {
                results.push(json!({"path": raw, "result": "cleanup_path_rejected"}));
                continue;
            }
            let p = Path::new(raw);
            let r = if p.is_dir() {
                std::fs::remove_dir_all(p)
            } else {
                match std::fs::remove_file(p) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    other => other,
                }
            };
            match r {
                Ok(()) => results.push(json!({"path": raw, "result": "removed"})),
                Err(e) => results.push(
                    json!({"path": raw, "result": "cleanup_failed", "reason": truncate(&py_os_error(&e, p), 200)}),
                ),
            }
        }
        results
    }

    pub fn prune_completed(&self, max_age: f64, max_count: usize) -> usize {
        let mut files: Vec<(PathBuf, f64)> = Self::sorted_json_files(&self.completed_dir)
            .into_iter()
            .filter_map(|p| {
                let m = std::fs::metadata(&p).ok()?.modified().ok()?;
                Some((p, m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64()))
            })
            .collect();
        files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let t = now();
        let mut removed = 0;
        for (i, (p, mtime)) in files.iter().enumerate() {
            if (t - mtime > max_age || i >= max_count) && std::fs::remove_file(p).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    pub fn wait_identity_gone(identity: &Value, timeout: f64) -> bool {
        let deadline = Instant::now() + Duration::from_secs_f64(timeout);
        while Instant::now() < deadline {
            if !identity_matches(Some(identity)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        !identity_matches(Some(identity))
    }

    fn remove_empty_work_dirs(&self, run_id: &str) -> Vec<Value> {
        let root = self.work_dir.join(run_id);
        if !root.exists() {
            return vec![];
        }
        let mut results = Vec::new();
        fn walk(dir: &Path, results: &mut Vec<Value>) {
            let Ok(rd) = std::fs::read_dir(dir) else {
                return;
            };
            let mut subdirs: Vec<PathBuf> = rd
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| e.path())
                .collect();
            subdirs.sort();
            for d in &subdirs {
                walk(d, results);
            }
            for d in subdirs {
                if std::fs::remove_dir(&d).is_ok() {
                    results.push(json!({"path": d.display().to_string(), "result": "removed"}));
                }
            }
        }
        walk(&root, &mut results);
        match std::fs::remove_dir(&root) {
            Ok(()) => results.push(json!({"path": root.display().to_string(), "result": "removed"})),
            Err(e) => results.push(json!({
                "path": root.display().to_string(),
                "result": "cleanup_failed",
                "reason": truncate(&format!("work directory not empty: {}", py_os_error(&e, &root)), 200),
            })),
        }
        results
    }

    fn reap_call(&self, call_id: &str, call: &Value, dry_run: bool) -> Value {
        let empty = json!({});
        let child = call.get("child").filter(|c| truthy(Some(c))).unwrap_or(&empty);
        if !identity_matches(Some(child)) {
            let pid = int_of(child.get("pid"));
            let expected = child.get("start_fingerprint");
            let current = if pid > 0 { process::start_fingerprint(pid) } else { None };
            if let (Some(cur), Some(Value::String(exp))) = (&current, expected) {
                if !exp.is_empty() && cur != exp && !process::state(pid).starts_with('Z') {
                    return json!({"call_id": call_id, "action": "refused", "result": "identity_mismatch"});
                }
            }
            return json!({"call_id": call_id, "action": "none", "result": "not_live_or_identity_mismatch"});
        }
        let pid = int_of(child.get("pid"));
        let pgid = int_of(call.get("pgid"));
        let marker_len = match call.get("ownership_marker_hash") {
            Some(v) if truthy(Some(v)) => lope_core::config::py_str(v).chars().count(),
            _ => 0,
        };
        if pid <= 0 || pgid <= 0 || marker_len != 64 {
            return json!({"call_id": call_id, "action": "refused", "result": "ownership_unverified"});
        }
        let actual = unsafe { libc::getpgid(pid as i32) };
        if actual < 0 {
            return json!({"call_id": call_id, "action": "none", "result": "already_gone"});
        }
        if actual as i64 != pgid {
            return json!({"call_id": call_id, "action": "refused", "result": "pgid_mismatch"});
        }
        if dry_run {
            return json!({"call_id": call_id, "action": "would_reap", "result": "confirmed_owned"});
        }
        if unsafe { libc::killpg(pgid as i32, libc::SIGTERM) } != 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            return json!({"call_id": call_id, "action": "none", "result": "already_gone"});
        }
        if !Self::wait_identity_gone(child, 1.0) {
            unsafe { libc::killpg(pgid as i32, libc::SIGKILL) };
        }
        let gone = Self::wait_identity_gone(child, 3.0);
        json!({
            "call_id": call_id,
            "action": if gone { "reaped" } else { "kill_sent" },
            "result": if gone { "clean" } else { "cleanup_unconfirmed" },
        })
    }

    fn owned_paths(manifest: &Value) -> Vec<String> {
        let mut paths = Vec::new();
        if let Some(Value::Object(calls)) = manifest.get("calls") {
            for call in calls.values() {
                if let Some(Value::Array(a)) = call.get("owned_paths") {
                    paths.extend(a.iter().map(lope_core::config::py_str));
                }
            }
        }
        paths
    }

    pub fn reap_run(&self, run_id: &str, dry_run: bool) -> R<Value> {
        let manifest = self.load_active(run_id)?;
        let classification = self.classify(&manifest);
        if classification != "abandoned" {
            return Ok(json!({
                "run_id": run_id, "classification": classification, "action": "refused",
                "reason": "owner is live or ownership is not abandoned", "calls": [],
            }));
        }
        let calls: Vec<(String, Value)> = match manifest.get("calls") {
            Some(Value::Object(c)) => c.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            _ => vec![],
        };
        let actions: Vec<Value> = calls.iter().map(|(id, c)| self.reap_call(id, c, dry_run)).collect();
        let refused = actions.iter().any(|a| a["action"] == "refused");
        let still_live = calls.iter().any(|(_, c)| identity_matches(c.get("child")));
        let mut cleanup = Vec::new();
        let failed = |cleanup: &Vec<Value>| {
            cleanup.iter().any(|i| !matches!(i.get("result").and_then(Value::as_str), Some("removed" | "already_gone")))
        };
        if !dry_run && !refused && !still_live {
            cleanup = self.cleanup_owned_paths(run_id, &Self::owned_paths(&manifest));
            cleanup.extend(self.remove_empty_work_dirs(run_id));
            let cleanup_failed = failed(&cleanup);
            let (acts, paths) = (actions.clone(), cleanup.clone());
            self.update(run_id, move |m| {
                let hist = m.entry("cleanup_history").or_insert_with(|| json!([]));
                if let Value::Array(a) = hist {
                    a.push(json!({"at": fnum(now()), "source": "reconcile", "calls": acts, "paths": paths}));
                }
                if cleanup_failed {
                    m.insert("state".into(), json!("cleanup_failed"));
                    m.insert("cleanup_result".into(), json!("cleanup_failed"));
                    m.insert("reason".into(), json!("owned resource cleanup incomplete"));
                }
                Ok(())
            })?;
            if !cleanup_failed {
                self.finish_run(run_id, "reaped", "confirmed owner dead", "clean")?;
            }
        }
        let action = if dry_run {
            "dry_run"
        } else if refused || still_live {
            "refused"
        } else if failed(&cleanup) {
            "cleanup_failed"
        } else {
            "reaped"
        };
        Ok(
            json!({"run_id": run_id, "classification": classification, "action": action, "calls": actions, "cleanup": cleanup}),
        )
    }

    pub fn reap_all(&self, dry_run: bool) -> R<Vec<Value>> {
        let mut results = Vec::new();
        for m in self.list_active(true) {
            let run_id = match m.get("run_id") {
                Some(v) if truthy(Some(v)) => lope_core::config::py_str(v),
                _ => String::new(),
            };
            if run_id.is_empty() {
                let reason = match m.get("reason") {
                    Some(r) if truthy(Some(r)) => r.clone(),
                    _ => json!("invalid manifest"),
                };
                results.push(json!({
                    "run_id": null, "classification": "ownership_unverified", "action": "refused",
                    "reason": reason, "calls": [],
                }));
                continue;
            }
            results.push(self.reap_run(&run_id, dry_run)?);
        }
        Ok(results)
    }

    pub fn kill_run(&self, run_id: &str) -> R<Value> {
        let manifest = self.load_active(run_id)?;
        let refuse = |classification: &str, reason: String, calls: Value| json!({"run_id": run_id, "classification": classification, "action": "refused", "reason": reason, "calls": calls});
        if manifest.get("host_boot_fingerprint").and_then(Value::as_str) != Some(boot_fingerprint().as_str()) {
            return Ok(refuse("ownership_unverified", "host boot fingerprint mismatch".into(), json!([])));
        }
        let owner = manifest.get("owner").filter(|o| truthy(Some(o))).cloned().unwrap_or(json!({}));
        if !identity_matches(Some(&owner)) {
            return Ok(refuse(
                self.classify(&manifest),
                "missing, dead, or reused owner fingerprint; run jobs reap instead".into(),
                json!([]),
            ));
        }
        let owner_pid = int_of(owner.get("pid"));
        if owner_pid == std::process::id() as i64 {
            return Ok(refuse("active", "jobs kill cannot terminate its own process".into(), json!([])));
        }
        let calls: Vec<(String, Value)> = match manifest.get("calls") {
            Some(Value::Object(c)) => c.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            _ => vec![],
        };
        let preflight: Vec<Value> = calls.iter().map(|(id, c)| self.reap_call(id, c, true)).collect();
        if preflight.iter().any(|a| a["action"] == "refused") {
            return Ok(refuse(
                "ownership_unverified",
                "one or more live provider identities could not be verified".into(),
                Value::Array(preflight),
            ));
        }
        if unsafe { libc::kill(owner_pid as i32, libc::SIGTERM) } != 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ESRCH) {
                return Ok(refuse(
                    "active",
                    truncate(&format!("cannot signal verified owner: {}", py_errno(&e)), 240),
                    Value::Array(preflight),
                ));
            }
        }
        if !Self::wait_identity_gone(&owner, 2.0) && identity_matches(Some(&owner)) {
            if unsafe { libc::kill(owner_pid as i32, libc::SIGKILL) } != 0 {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() != Some(libc::ESRCH) {
                    return Ok(refuse(
                        "active",
                        truncate(&format!("verified owner ignored TERM and KILL failed: {}", py_errno(&e)), 240),
                        Value::Array(preflight),
                    ));
                }
            }
            Self::wait_identity_gone(&owner, 3.0);
        }
        if identity_matches(Some(&owner)) {
            return Ok(json!({
                "run_id": run_id, "classification": "active", "action": "cleanup_failed",
                "reason": "verified owner remains live after TERM/KILL", "calls": preflight,
            }));
        }
        let reaped = self.reap_run(run_id, false)?;
        Ok(json!({
            "run_id": run_id,
            "classification": reaped.get("classification").cloned().unwrap_or(json!("abandoned")),
            "action": if reaped["action"] == "reaped" { json!("killed") } else { reaped.get("action").cloned().unwrap_or(Value::Null) },
            "reason": "verified owner terminated",
            "calls": reaped.get("calls").cloned().unwrap_or(json!([])),
            "cleanup": reaped.get("cleanup").cloned().unwrap_or(json!([])),
        }))
    }

    /// `close_run`: verify zero live owned work and scratch, then complete.
    pub fn close_run(&self, run_id: &str, state: &str, reason: &str) -> R<Value> {
        let manifest = self.load_active(run_id)?;
        let calls: Vec<(String, Value)> = match manifest.get("calls") {
            Some(Value::Object(c)) => c.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            _ => vec![],
        };
        let mut unresolved = Vec::new();
        for (id, call) in &calls {
            let live: Vec<&str> =
                ["supervisor", "child"].into_iter().filter(|r| identity_matches(call.get(*r))).collect();
            let st = call.get("state").and_then(Value::as_str);
            if !live.is_empty() || !matches!(st, Some("finished" | "completed")) {
                unresolved.push(
                    json!({"call_id": id, "live": live, "state": call.get("state").cloned().unwrap_or(Value::Null)}),
                );
            }
        }
        let mut cleanup = self.cleanup_owned_paths(run_id, &Self::owned_paths(&manifest));
        cleanup.extend(self.remove_empty_work_dirs(run_id));
        let cleanup_failed = cleanup
            .iter()
            .any(|i| !matches!(i.get("result").and_then(Value::as_str), Some("removed" | "already_gone")));
        if !unresolved.is_empty() || cleanup_failed {
            let close_reason = if !unresolved.is_empty() {
                "run closeout found live/unresolved calls"
            } else {
                "run closeout found owned scratch cleanup failure"
            };
            let (u, c) = (unresolved.clone(), cleanup.clone());
            self.update(run_id, move |m| {
                m.insert("state".into(), json!("cleanup_failed"));
                m.insert("cleanup_result".into(), json!("cleanup_failed"));
                m.insert("reason".into(), json!(close_reason));
                let hist = m.entry("cleanup_history").or_insert_with(|| json!([]));
                if let Value::Array(a) = hist {
                    a.push(json!({"at": fnum(now()), "source": "closeout", "unresolved": u, "paths": c}));
                }
                Ok(())
            })?;
            return Ok(
                json!({"run_id": run_id, "action": "cleanup_failed", "reason": close_reason, "unresolved": unresolved, "cleanup": cleanup}),
            );
        }
        let dest = self.finish_run(run_id, state, reason, "clean")?;
        Ok(
            json!({"run_id": run_id, "action": "closed", "completed_manifest": dest.display().to_string(), "unresolved": [], "cleanup": cleanup}),
        )
    }

    pub fn reconcile(&self, dry_run: bool) -> R<Vec<Value>> {
        let mut out = Vec::new();
        for m in self.list_active(false) {
            if m.get("classification").and_then(Value::as_str) == Some("abandoned") {
                if let Some(id) = m.get("run_id").filter(|v| truthy(Some(v))) {
                    out.push(self.reap_run(&lope_core::config::py_str(id), dry_run)?);
                }
            }
        }
        Ok(out)
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// `Path.resolve(strict=False)`: resolve symlinks for the existing prefix.
pub fn resolve_nonstrict(p: &Path) -> PathBuf {
    let mut existing = p.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(c) = std::fs::canonicalize(&existing) {
            let mut out = c;
            for r in rest.iter().rev() {
                out.push(r);
            }
            return out;
        }
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// Python's `str(OSError)` shape: `[Errno N] Message: 'path'`.
pub fn py_os_error(e: &std::io::Error, path: &Path) -> String {
    match e.raw_os_error() {
        Some(code) => format!("[Errno {code}] {}: {}", strerror(code), py_repr_str(&path.display().to_string())),
        None => e.to_string(),
    }
}

fn py_errno(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(code) => format!("[Errno {code}] {}", strerror(code)),
        None => e.to_string(),
    }
}

pub fn strerror(code: i32) -> String {
    unsafe { std::ffi::CStr::from_ptr(libc::strerror(code)) }.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_and_privacy() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().join("runs");
        let r = RunRegistry::open(root).unwrap();
        let m = r.start_run("ask", Some("run-1"), Some(60.0), None).unwrap();
        assert_eq!(m["state"], "active");
        let rows = r.list_active(true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["classification"], "active");
        let mut bad = Map::new();
        bad.insert("call_id".into(), json!("c1"));
        bad.insert("argv".into(), json!(["x"]));
        assert_eq!(r.register_call("run-1", bad).unwrap_err().0, "forbidden call manifest fields: argv");
        let mut call = Map::new();
        call.insert("call_id".into(), json!("c1"));
        call.insert("state".into(), json!("running"));
        r.register_call("run-1", call).unwrap();
        let mut ch = Map::new();
        ch.insert("state".into(), json!("finished"));
        let v = r.update_call("run-1", "c1", ch).unwrap();
        assert_eq!(v["call_counters"]["finished"], 1);
        let closed = r.close_run("run-1", "completed", "").unwrap();
        assert_eq!(closed["action"], "closed", "{closed}");
        assert!(r.list_active(true).is_empty());
        assert!(r.active_path("bad id").is_err());
    }
}
