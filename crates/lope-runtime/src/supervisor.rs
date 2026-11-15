//! The provider supervisor (`supervisor.py`), run as `lope-supervisor
//! RESULT_PATH`. Reads one JSON spec line on stdin and keeps the pipe open as
//! a parent-liveness signal; `cancel` cancels; EOF means the owner died.
//!
//! Event-driven: one thread per pipe and one blocked in `waitpid`, all
//! feeding a channel. The only timed polling is the bounded post-exit
//! group-emptiness confirmation (NFR3).

use crate::bounded::BoundedStream;
use crate::platform::{self, Group};
use lope_core::pyjson::{self, Opts};
use lope_state::process;
use lope_state::registry::RunRegistry;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub const READ_SIZE: usize = 64 * 1024;
pub const HEARTBEAT_SECONDS: f64 = 5.0;
pub const TERM_GRACE: f64 = 0.2;
pub const KILL_GRACE: f64 = 0.5;
pub const DRAIN_AFTER_EXIT: f64 = 0.5;

enum Ev {
    Out(Vec<u8>),
    OutEof,
    Err(Vec<u8>),
    ErrEof,
    Exit(i32),
    Ctl(Vec<u8>),
    CtlEof,
}

struct Io {
    stdout: BoundedStream,
    stderr: BoundedStream,
    stderr_hard_limit: usize,
    out_open: bool,
    err_open: bool,
    exited: Option<i32>,
    last_byte: Option<Instant>,
    control: Vec<Vec<u8>>,
    control_eof: bool,
}

impl Io {
    /// Apply one event; returns true if it was a control event.
    fn apply(&mut self, ev: Ev, over: &mut Option<&'static str>) -> bool {
        match ev {
            Ev::Out(c) => {
                self.last_byte = Some(Instant::now());
                if self.stdout.add(&c) && over.is_none() {
                    *over = Some("stdout");
                }
            }
            Ev::Err(c) => {
                self.last_byte = Some(Instant::now());
                let _ = self.stderr.add(&c);
                if self.stderr.total > self.stderr_hard_limit && over.is_none() {
                    *over = Some("stderr");
                }
            }
            Ev::OutEof => self.out_open = false,
            Ev::ErrEof => self.err_open = false,
            Ev::Exit(rc) => self.exited = Some(rc),
            Ev::Ctl(c) => {
                self.control.push(c);
                return true;
            }
            Ev::CtlEof => {
                self.control_eof = true;
                return true;
            }
        }
        false
    }

    /// Process events until `done(self)` or `until`. Returns true if done.
    fn pump_until(&mut self, rx: &Receiver<Ev>, until: Instant, done: impl Fn(&Io) -> bool) -> bool {
        let mut over = None;
        loop {
            if done(self) {
                return true;
            }
            let now = Instant::now();
            if now >= until {
                return done(self);
            }
            match rx.recv_timeout(until - now) {
                Ok(ev) => {
                    self.apply(ev, &mut over);
                }
                Err(RecvTimeoutError::Timeout) => return done(self),
                Err(RecvTimeoutError::Disconnected) => return done(self),
            }
        }
    }
}

fn returncode(status: i32) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        -libc::WTERMSIG(status)
    } else {
        -1
    }
}

fn spawn_reader(mut r: impl Read + Send + 'static, tx: Sender<Ev>, out: bool) {
    std::thread::spawn(move || {
        let mut buf = vec![0u8; READ_SIZE];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(if out { Ev::OutEof } else { Ev::ErrEof });
                    return;
                }
                Ok(n) => {
                    let _ = tx.send(if out { Ev::Out(buf[..n].to_vec()) } else { Ev::Err(buf[..n].to_vec()) });
                }
            }
        }
    });
}

struct Registry {
    reg: RunRegistry,
    run_id: String,
    call_id: String,
    identity: Value,
}

impl Registry {
    fn open(spec: &Value) -> Option<Self> {
        let root = spec.get("registry_root")?.as_str().filter(|s| !s.is_empty())?;
        let run_id = spec.get("run_id")?.as_str().filter(|s| !s.is_empty())?.to_string();
        let call_id = spec.get("call_id")?.as_str().filter(|s| !s.is_empty())?.to_string();
        let reg = RunRegistry::open(PathBuf::from(root)).ok()?;
        let identity = process::identity_timeout(std::process::id() as i64, Duration::from_millis(300));
        Some(Registry { reg, run_id, call_id, identity })
    }

    fn update(&self, mut changes: Map<String, Value>) {
        changes.entry("supervisor").or_insert_with(|| self.identity.clone());
        let _ = self.reg.update_call(&self.run_id, &self.call_id, changes);
    }
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

struct Provider {
    pid: i32,
    pgid: i32,
}

/// `_terminate_and_reap` with the DEC-4 budget. Returns (cleanup, targeted).
fn terminate_and_reap(p: &Provider, io: &mut Io, rx: &Receiver<Ev>, budget: f64) -> (String, i64) {
    let deadline = Instant::now() + Duration::from_secs_f64(budget);
    let left = |cap: f64| {
        Instant::now() + Duration::from_secs_f64(cap).min(deadline.saturating_duration_since(Instant::now()))
    };
    let already_exited = io.exited.is_some();
    let targeted = i64::from(!already_exited);
    let mut group_absent;
    if !already_exited {
        group_absent = platform::signal_group(p.pgid, libc::SIGTERM) == "already_exited";
        io.pump_until(rx, left(TERM_GRACE), |io| io.exited.is_some());
    } else {
        // Leader gone. Fast path: nothing left in the group.
        match platform::probe(p.pgid) {
            Group::Empty => group_absent = true,
            _ => {
                // Spec 01 §7: give lingering descendants a TERM grace first.
                platform::signal_group(p.pgid, libc::SIGTERM);
                group_absent = wait_group_empty(p.pgid, left(TERM_GRACE)) == Group::Empty;
            }
        }
    }
    let mut kill_result = "already_exited".to_string();
    if !group_absent {
        kill_result = platform::signal_group(p.pgid, libc::SIGKILL);
        group_absent = kill_result == "already_exited";
    }
    if io.exited.is_none() {
        unsafe { libc::kill(p.pid, libc::SIGKILL) };
    }
    if !io.pump_until(rx, left(KILL_GRACE), |io| io.exited.is_some()) {
        return ("cleanup_failed:child_not_reaped".into(), targeted);
    }
    let remaining = if group_absent { Group::Empty } else { wait_group_empty(p.pgid, deadline) };
    match remaining {
        Group::Unknown => ("cleanup_failed:inspection_unknown".into(), targeted),
        Group::Present => ("cleanup_failed:descendants_live".into(), targeted),
        Group::Empty => (if already_exited { "clean".into() } else { format!("clean:{kill_result}") }, targeted),
    }
}

/// State captured at the moment a call is cut off (FR12).
struct Snapshot {
    leader_alive: bool,
    leader_state: String,
    group: Group,
    since_last_byte: Option<f64>,
}

fn snapshot(io: &Io, pid: i32, pgid: i32) -> Snapshot {
    let leader_alive = io.exited.is_none();
    Snapshot {
        leader_alive,
        leader_state: if leader_alive { platform::leader_state(pid) } else { String::new() },
        group: platform::probe(pgid),
        since_last_byte: io.last_byte.map(|t| t.elapsed().as_secs_f64()),
    }
}

fn write_diag(registry: &Option<Registry>, kind: &str, timeout: f64, s: Snapshot, io: &Io) -> Option<PathBuf> {
    let r = registry.as_ref()?;
    let d = crate::diag::TimeoutDiag {
        kind: kind.to_string(),
        timeout_seconds: timeout,
        stdout_bytes: io.stdout.total,
        stderr_bytes: io.stderr.total,
        seconds_since_last_byte: s.since_last_byte,
        leader_alive: s.leader_alive,
        leader_state: s.leader_state,
        group_members_present: match s.group {
            Group::Empty => Some(false),
            Group::Present => Some(true),
            Group::Unknown => None,
        },
        stderr_tail: io.stderr.value().to_vec(),
    };
    crate::diag::write(&r.reg.root, &r.run_id, &r.call_id, &d)
}

/// Bounded back-off confirmation that the group is empty (5 ms → 50 ms).
pub fn wait_group_empty(pgid: i32, deadline: Instant) -> Group {
    let mut delay = 0.005f64;
    loop {
        let g = platform::probe(pgid);
        if g == Group::Empty || Instant::now() >= deadline {
            return g;
        }
        let rem = deadline.saturating_duration_since(Instant::now()).as_secs_f64();
        std::thread::sleep(Duration::from_secs_f64(delay.min(rem)));
        delay = (delay * 2.0).min(0.05);
    }
}

fn provider_env(spec: &Value) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .collect();
    let mut set = |k: &str, v: Option<String>| {
        env.retain(|(ek, _)| ek != k);
        if let Some(v) = v {
            env.push((k.to_string(), v));
        }
    };
    if let Some(Value::Object(m)) = spec.get("env") {
        for (k, v) in m {
            set(k, if v.is_null() { None } else { Some(lope_core::config::py_str(v)) });
        }
    }
    let s = |k: &str| spec.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    if let Some(m) = s("ownership_marker") {
        set("LOPE_PROCESS_MARKER", Some(m));
    }
    if let Some(r) = s("run_id") {
        set("LOPE_RUN_ID", Some(r));
    }
    if let Some(c) = s("call_id") {
        set("LOPE_CALL_ID", Some(c));
    }
    env
}

/// Python-style spawn error text (`FileNotFoundError: [Errno 2] ...: 'x'`).
fn spawn_error(e: &std::io::Error, target: &str) -> String {
    let code = e.raw_os_error().unwrap_or(0);
    let class = match code {
        libc::ENOENT => "FileNotFoundError",
        libc::EACCES | libc::EPERM => "PermissionError",
        libc::ENOTDIR => "NotADirectoryError",
        _ => "OSError",
    };
    format!(
        "{class}: [Errno {code}] {}: {}",
        lope_state::registry::strerror(code),
        lope_state::registry::py_repr_str(target)
    )
}

fn str_of(spec: &Value, k: &str) -> String {
    spec.get(k).map(lope_core::config::py_str).unwrap_or_default()
}

fn num_of(spec: &Value, k: &str, d: f64) -> f64 {
    spec.get(k).and_then(Value::as_f64).filter(|v| *v != 0.0).unwrap_or(d)
}

fn output_limit_reason(label: &str, stdout_limit: usize, stderr_limit: usize) -> String {
    let limit = if label == "stdout" { stdout_limit } else { stderr_limit };
    format!("{label} exceeded {limit} bytes")
}

/// Run one provider per the spec, feeding control events from `ctl`.
pub fn run_provider(spec: &Value, ctl: Box<dyn Read + Send>) -> Value {
    let started = Instant::now();
    let timeout = num_of(spec, "timeout", 0.0);
    let stdout_limit = num_of(spec, "stdout_limit", 2.0 * 1024.0 * 1024.0) as usize;
    let stderr_limit = num_of(spec, "stderr_limit", 512.0 * 1024.0) as usize;
    let stderr_hard_limit = (num_of(spec, "stderr_hard_limit", 32.0 * 1024.0 * 1024.0) as usize).max(stderr_limit);
    let budget = crate::cleanup_budget();
    let command: Vec<String> = match spec.get("command") {
        Some(Value::Array(a)) if !a.is_empty() && a.iter().all(Value::is_string) => {
            a.iter().map(|x| x.as_str().unwrap().to_string()).collect()
        }
        _ => return launch_error("ValueError: supervisor command must be a non-empty argv list"),
    };
    if timeout <= 0.0 {
        return launch_error("ValueError: supervisor timeout must be positive");
    }
    let input = str_of(spec, "input_text").into_bytes();
    let cwd = spec.get("cwd").and_then(Value::as_str).filter(|s| !s.is_empty()).map(PathBuf::from);
    let registry = Registry::open(spec);

    let mut cmd = Command::new(&command[0]);
    cmd.args(&command[1..]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.env_clear().envs(provider_env(spec));
    if let Some(c) = &cwd {
        cmd.current_dir(c);
    }
    unsafe {
        cmd.pre_exec(|| {
            // start_new_session=True, plus Python's restore_signals.
            libc::setsid();
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            libc::signal(libc::SIGXFSZ, libc::SIG_DFL);
            Ok(())
        });
    }
    if let Some(c) = &cwd {
        if !c.is_dir() {
            let e = std::io::Error::from_raw_os_error(if c.exists() { libc::ENOTDIR } else { libc::ENOENT });
            return launch_error(&spawn_error(&e, &c.display().to_string()));
        }
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return launch_error(&spawn_error(&e, &command[0])),
    };
    let pid = child.id() as i32;
    let pgid = {
        let g = unsafe { libc::getpgid(pid) };
        if g > 0 {
            g
        } else {
            pid
        }
    };
    let provider = Provider { pid, pgid };
    // FR13 + parent death: TERM to the supervisor kills the provider group.
    SUPERVISED_PGID.store(pgid, std::sync::atomic::Ordering::SeqCst);

    let (tx, rx) = channel::<Ev>();
    spawn_reader(child.stdout.take().unwrap(), tx.clone(), true);
    spawn_reader(child.stderr.take().unwrap(), tx.clone(), false);
    let mut stdin = child.stdin.take().unwrap();
    if input.is_empty() {
        drop(stdin);
    } else {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut status = 0;
            loop {
                let r = unsafe { libc::waitpid(pid, &mut status, 0) };
                if r == pid {
                    let _ = tx.send(Ev::Exit(returncode(status)));
                    return;
                }
                if r < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    let _ = tx.send(Ev::Exit(-1));
                    return;
                }
            }
        });
        // `child` is reaped by the thread above; forget the std handle.
        std::mem::forget(child);
    }
    {
        let tx = tx.clone();
        let mut ctl = ctl;
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match ctl.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Ev::CtlEof);
                        return;
                    }
                    Ok(n) => {
                        let _ = tx.send(Ev::Ctl(buf[..n].to_vec()));
                    }
                }
            }
        });
    }
    drop(tx);

    let marker = str_of(spec, "ownership_marker");
    if let Some(r) = &registry {
        let child_identity = process::identity_timeout(pid as i64, Duration::from_millis(300));
        let owned: Vec<Value> = match spec.get("owned_paths") {
            Some(Value::Array(a)) if !a.is_empty() => a.clone(),
            _ => vec![json!(str_of(spec, "result_path"))],
        };
        r.update(obj(json!({
            "child": child_identity,
            "pgid": pgid,
            "state": "active",
            "heartbeat_at": lope_core::time::now(),
            "ownership_marker_hash": if marker.is_empty() { String::new() } else { sha256(&marker) },
            "owned_paths": owned,
        })));
    }

    let mut io = Io {
        stdout: BoundedStream::new(stdout_limit),
        stderr: BoundedStream::new(stderr_limit),
        stderr_hard_limit,
        out_open: true,
        err_open: true,
        exited: None,
        last_byte: None,
        control: Vec::new(),
        control_eof: false,
    };
    let mut deadline = started + Duration::from_secs_f64(timeout);
    if let Some(at) = spec.get("deadline_at").and_then(Value::as_f64) {
        // Finish a little before the caller gives up on us.
        let left = at - lope_core::time::now() - 0.05;
        deadline = deadline.min(Instant::now() + Duration::from_secs_f64(left.max(0.0)));
    }
    let mut next_heartbeat = started + Duration::from_secs_f64(HEARTBEAT_SECONDS);
    let mut outcome = "ok".to_string();
    let mut reason = String::new();
    let mut parent_dead = false;
    let mut diag_path: Option<PathBuf> = None;
    let mut cleanup_seconds: Option<f64> = None;
    // Every exit from the loop below has already run terminate_and_reap.
    let fin: (String, i64);

    loop {
        let now = Instant::now();
        if now >= next_heartbeat {
            if let Some(r) = &registry {
                r.update(obj(json!({"state": "active", "heartbeat_at": lope_core::time::now(), "heartbeat_source": "supervisor"})));
            }
            next_heartbeat = now + Duration::from_secs_f64(HEARTBEAT_SECONDS);
        }
        if now >= deadline {
            outcome = "provider_timeout".into();
            reason = format!("provider timed out after {}s", py_g(timeout));
            let snap = snapshot(&io, pid, pgid);
            fin = terminate_and_reap(&provider, &mut io, &rx, budget);
            diag_path = write_diag(&registry, &outcome, timeout, snap, &io);
            break;
        }
        if io.exited.is_some() {
            let t = Instant::now();
            fin = terminate_and_reap(&provider, &mut io, &rx, budget);
            cleanup_seconds = Some(t.elapsed().as_secs_f64());
            let cleanup = &fin.0;
            let drain_until = deadline.min(Instant::now() + Duration::from_secs_f64(DRAIN_AFTER_EXIT));
            io.pump_until(&rx, drain_until, |io| !io.out_open && !io.err_open);
            if io.stdout.total > stdout_limit || io.stderr.total > stderr_hard_limit {
                outcome = "output_limit".into();
                let label = if io.stdout.total > stdout_limit { "stdout" } else { "stderr" };
                reason = output_limit_reason(label, stdout_limit, stderr_hard_limit);
            } else if !(cleanup == "clean" || cleanup.starts_with("clean:")) {
                outcome = "cleanup_failed".into();
                reason = "provider exited but owned descendants could not be reaped".into();
            } else if io.out_open || io.err_open {
                outcome = "output_drain_timeout".into();
                reason = "provider exited but output pipes did not close after bounded drain".into();
            }
            break;
        }
        let wait_until = deadline.min(next_heartbeat);
        let ev = match rx.recv_timeout(wait_until.saturating_duration_since(Instant::now())) {
            Ok(ev) => ev,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => continue,
        };
        let mut over = None;
        if io.apply(ev, &mut over) {
            if io.control_eof {
                parent_dead = true;
                outcome = "cancelled".into();
                reason = "parent control pipe closed".into();
                fin = terminate_and_reap(&provider, &mut io, &rx, budget);
                break;
            }
            if io.control.iter().any(|c| String::from_utf8_lossy(c).to_lowercase().contains("cancel")) {
                outcome = "cancelled".into();
                reason = "parent requested cancellation".into();
                let snap = snapshot(&io, pid, pgid);
                fin = terminate_and_reap(&provider, &mut io, &rx, budget);
                diag_path = write_diag(&registry, &outcome, timeout, snap, &io);
                break;
            }
            io.control.clear();
        }
        if let Some(label) = over {
            outcome = "output_limit".into();
            reason = output_limit_reason(label, stdout_limit, stderr_hard_limit);
            fin = terminate_and_reap(&provider, &mut io, &rx, budget);
            break;
        }
    }
    let (mut cleanup, targeted) = fin;
    if io.exited.is_none() {
        // Python: proc.wait(timeout=0) raising after cleanup was attempted.
        cleanup = "cleanup_failed:wait".into();
    }
    if matches!(outcome.as_str(), "provider_timeout" | "cancelled" | "output_limit") {
        io.pump_until(&rx, Instant::now() + Duration::from_millis(100), |io| !io.out_open && !io.err_open);
    }
    let rc = io.exited.unwrap_or(-9);
    if outcome == "ok" && rc != 0 {
        outcome = "nonzero_exit".into();
        reason = format!("provider exited {rc}");
    }
    let mut result = json!({
        "returncode": rc,
        "stdout_complete": !io.out_open && io.stdout.total <= stdout_limit,
        "stdout_truncated": io.stdout.total > stdout_limit,
        "stdout": String::from_utf8_lossy(io.stdout.value()),
        "stderr": String::from_utf8_lossy(io.stderr.value()),
        "outcome": outcome,
        "reason": reason,
        "cleanup_result": cleanup,
        "elapsed_seconds": started.elapsed().as_secs_f64(),
        "child_pid": pid,
        "pgid": pgid,
        "parent_dead": parent_dead,
        "processes_targeted": targeted,
        "platform_cleanup": "not_applicable",
    });
    if let Some(d) = &diag_path {
        result["diag_path"] = json!(d.display().to_string());
    }
    if let Some(c) = cleanup_seconds {
        // Extra (Python ignores unknown result keys): post-exit cleanup time.
        result["cleanup_seconds"] = json!(c);
    }
    if let Some(r) = &registry {
        let mut fin = obj(json!({
            "state": "finished",
            "outcome": result["outcome"],
            "reason": result["reason"],
            "cleanup_result": result["cleanup_result"],
            "ended_at": lope_core::time::now(),
            "processes_targeted": targeted,
            "platform_cleanup": "not_applicable",
        }));
        if let Some(d) = &diag_path {
            fin.insert("diag_path".into(), json!(d.display().to_string()));
        }
        r.update(fin);
    }
    SUPERVISED_PGID.store(0, std::sync::atomic::Ordering::SeqCst);
    result
}

/// Python `format(x, "g")`: 6 significant digits, trailing zeros removed,
/// scientific notation when the exponent is < -4 or >= 6.
pub fn py_g(x: f64) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x > 0.0 {
            "inf".into()
        } else {
            "-inf".into()
        };
    }
    let sci = format!("{:.5e}", x);
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    if (-4..6).contains(&exp) {
        let decimals = (5 - exp).max(0) as usize;
        let s = format!("{:.*}", decimals, x);
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    } else {
        let m = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.') } else { mant };
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

fn sha256(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn launch_error(reason: &str) -> Value {
    json!({
        "returncode": -1, "stdout": "", "stderr": "", "outcome": "launch_error",
        "reason": reason.chars().take(500).collect::<String>(), "cleanup_result": "unknown", "elapsed_seconds": 0.0,
    })
}

static SUPERVISED_PGID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

extern "C" fn on_term(_sig: libc::c_int) {
    // Syscall-only path (supervisor.py:_run_provider): kill the owned group.
    let pgid = SUPERVISED_PGID.load(std::sync::atomic::Ordering::SeqCst);
    if pgid > 0 {
        unsafe { libc::killpg(pgid, libc::SIGKILL) };
    }
    unsafe { libc::_exit(143) };
}

/// `_write_result`: `.<name>.tmp` (0600), fsync, replace.
pub fn write_result(path: &Path, payload: &Value) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_file_name(format!(".{}.tmp", path.file_name().unwrap().to_string_lossy()));
    let text = pyjson::dumps(payload, &Opts { separators: Some((",", ":")), ..Opts::default() });
    let r = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    let _ = std::fs::remove_file(&tmp);
    r
}

/// `lope-supervisor RESULT_PATH` entry point.
pub fn main(args: &[String]) -> i32 {
    if args.len() != 1 {
        eprintln!("usage: lope-supervisor RESULT_PATH");
        return 2;
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t);
    }
    let result_path = PathBuf::from(&args[0]);
    let mut stdin = std::io::stdin();
    // Read exactly one line without buffering past it: the rest of stdin is
    // the control channel.
    let mut line = Vec::new();
    let mut b = [0u8; 1];
    loop {
        match stdin.read(&mut b) {
            Ok(0) => break,
            Ok(_) => {
                if b[0] == b'\n' {
                    break;
                }
                line.push(b[0]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    if line.is_empty() {
        return 3;
    }
    let mut spec = match pyjson::loads(&String::from_utf8_lossy(&line)) {
        Ok(v @ Value::Object(_)) => v,
        Ok(_) => {
            let _ = write_result(&result_path, &launch_error("ValueError: spec must be an object"));
            return 1;
        }
        Err(e) => {
            let _ = write_result(&result_path, &launch_error(&format!("JSONDecodeError: {e}")));
            return 1;
        }
    };
    spec["result_path"] = json!(result_path.display().to_string());
    let ctl: Box<dyn Read + Send> = Box::new(std::io::stdin());
    let result = run_provider(&spec, ctl);
    if result.get("parent_dead") != Some(&Value::Bool(true)) {
        let _ = write_result(&result_path, &result);
    } else {
        let _ = std::fs::remove_file(&result_path);
        if let Some(parent) = result_path.parent() {
            if parent.file_name().is_some_and(|n| n.to_string_lossy().starts_with("lope-call-")) {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }
    0
}
