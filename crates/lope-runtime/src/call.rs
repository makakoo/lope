//! Parent side of a supervised call (`processes.py:run_subprocess_group`).

use lope_core::pyjson;
use lope_state::registry::RunRegistry;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_STDOUT_LIMIT: usize = 2 * 1024 * 1024;
pub const DEFAULT_STDERR_LIMIT: usize = 512 * 1024;
pub const SUPERVISOR_CLEANUP_RESERVE: f64 = 1.0;
pub const ARGV_POLICY_LIMIT: usize = 128 * 1024;
pub const ARG_MAX_RESERVE: usize = 32 * 1024;

/// Registry context for a call (`context.metadata` in Python).
#[derive(Debug, Clone, Default)]
pub struct CallContext {
    pub registry_root: Option<PathBuf>,
    pub run_id: String,
    pub call_id: String,
    pub owned_paths: Vec<String>,
    /// Implementation writers get LOPE_IMPLEMENTATION_DEPTH + 1 (FR4).
    pub implementation: bool,
    /// Set after the call: the supervisor's cleanup result.
    pub cleanup_result: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub command: Vec<String>,
    pub input_text: Option<String>,
    pub timeout: f64,
    pub cwd: Option<PathBuf>,
    /// Overrides; `None` removes the variable.
    pub env: Vec<(String, Option<String>)>,
    pub stdout_limit: usize,
    pub stderr_limit: usize,
}

impl Request {
    pub fn new(command: Vec<String>, timeout: f64) -> Self {
        Request {
            command,
            input_text: None,
            timeout,
            cwd: None,
            env: vec![],
            stdout_limit: DEFAULT_STDOUT_LIMIT,
            stderr_limit: DEFAULT_STDERR_LIMIT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Completed {
    pub returncode: i64,
    pub stdout: String,
    pub stderr: String,
    pub cleanup_result: String,
}

/// UNVERIFIED completion metadata (`ProcessCompletionError`).
#[derive(Debug, Clone)]
pub struct Completion {
    pub reason: String,
    pub stdout: String,
    pub stderr: String,
    pub cleanup_result: String,
    pub stdout_complete: bool,
    pub stdout_truncated: bool,
    pub returncode: Value,
}

impl Completion {
    /// `str(ProcessCompletionError)`.
    pub fn message(&self) -> &str {
        const SAFE: [&str; 2] = [
            "provider exited but output pipes did not close after bounded drain",
            "provider exited but owned descendants could not be reaped",
        ];
        if SAFE.contains(&self.reason.as_str()) {
            &self.reason
        } else {
            "provider completion unverified"
        }
    }
}

#[derive(Debug, Clone)]
pub enum CallError {
    Invalid(String),
    InputLimit(String),
    /// `subprocess.TimeoutExpired(cmd, timeout)`
    Timeout {
        timeout: f64,
        stdout: String,
        stderr: String,
        diag_path: Option<String>,
    },
    OutputLimit {
        stream: String,
        limit: usize,
        stdout: String,
        stderr: String,
    },
    FileNotFound(String),
    Os(String),
    Unverified(Box<Completion>),
    BadResult(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Invalid(m) | CallError::InputLimit(m) | CallError::FileNotFound(m) | CallError::Os(m) => {
                f.write_str(m)
            }
            CallError::Timeout { timeout, .. } => {
                write!(f, "Command '...' timed out after {} seconds", crate::supervisor::py_g(*timeout))
            }
            CallError::OutputLimit { stream, limit, .. } => write!(f, "{stream} exceeded {limit} bytes"),
            CallError::Unverified(c) => f.write_str(c.message()),
            CallError::BadResult(m) => write!(f, "invalid supervisor result: {m}"),
        }
    }
}

fn check_argv_size(command: &[String], env: &[(String, Option<String>)]) -> Result<(), CallError> {
    let argv_bytes: usize = command.iter().map(|c| c.len() + 1).sum();
    if argv_bytes > ARGV_POLICY_LIMIT {
        return Err(CallError::InputLimit(format!(
            "argv payload {argv_bytes} bytes exceeds Lope policy limit {ARGV_POLICY_LIMIT}; use stdin, a prompt file, chunking, or rejection before launch"
        )));
    }
    let mut environment: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .collect();
    for (k, v) in env {
        environment.retain(|(ek, _)| ek != k);
        if let Some(v) = v {
            environment.push((k.clone(), v.clone()));
        }
    }
    let env_bytes: usize = environment.iter().map(|(k, v)| k.len() + v.len() + 2).sum();
    let arg_max = match unsafe { libc::sysconf(libc::_SC_ARG_MAX) } {
        n if n > 0 => n as usize,
        _ => 1024 * 1024,
    };
    let safe = arg_max.saturating_sub(ARG_MAX_RESERVE).max(1);
    if argv_bytes + env_bytes > safe {
        return Err(CallError::InputLimit(format!(
            "environment plus argv requires {} bytes; safe exec limit is {safe} bytes",
            argv_bytes + env_bytes
        )));
    }
    Ok(())
}

/// Supervisor executable: `LOPE_SUPERVISOR_EXE` (tests), else this binary.
fn supervisor_exe() -> PathBuf {
    std::env::var_os("LOPE_SUPERVISOR_EXE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_exe().unwrap_or_else(|_| PathBuf::from("lope")))
}

fn sha256(s: &[u8]) -> String {
    Sha256::digest(s).iter().map(|b| format!("{b:02x}")).collect()
}

/// Write all bytes with poll(2) until `deadline`; true if complete.
fn write_deadline(fd: i32, data: &[u8], deadline: Instant) -> bool {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    let mut off = 0;
    while off < data.len() {
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let ms = deadline.saturating_duration_since(now).as_millis().clamp(1, 100) as i32;
        let mut pfd = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, ms) };
        if r <= 0 {
            continue;
        }
        if pfd.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
            return false;
        }
        let end = (off + 64 * 1024).min(data.len());
        let n = unsafe { libc::write(fd, data[off..end].as_ptr() as *const libc::c_void, end - off) };
        if n > 0 {
            off += n as usize;
        } else {
            let e = std::io::Error::last_os_error().raw_os_error();
            if e != Some(libc::EAGAIN) && e != Some(libc::EINTR) {
                return false;
            }
        }
    }
    true
}

/// Wait for the supervisor to exit, without polling (thread + channel).
fn wait_exit(rx: &std::sync::mpsc::Receiver<()>, deadline: Instant) -> bool {
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_ok()
}

fn kill_supervisor(child: &Child, rx: &std::sync::mpsc::Receiver<()>) {
    let pgid = child.id() as i32;
    unsafe { libc::killpg(pgid, libc::SIGTERM) };
    if wait_exit(rx, Instant::now() + Duration::from_millis(500)) {
        return;
    }
    unsafe { libc::killpg(pgid, libc::SIGKILL) };
    let _ = wait_exit(rx, Instant::now() + Duration::from_millis(500));
}

struct Workspace {
    base: PathBuf,
    temporary: bool,
    registry: Option<(RunRegistry, String, String)>,
}

fn workspace(ctx: Option<&CallContext>) -> Result<Workspace, CallError> {
    if let Some(c) = ctx {
        if let Some(root) = &c.registry_root {
            if !c.run_id.is_empty() && !c.call_id.is_empty() {
                let reg = RunRegistry::open(root.clone()).map_err(|e| CallError::Os(e.to_string()))?;
                let base = reg.run_work_dir(&c.run_id).map_err(|e| CallError::Os(e.0))?.join(&c.call_id);
                std::fs::create_dir_all(&base).map_err(|e| CallError::Os(e.to_string()))?;
                let _ = std::fs::set_permissions(&base, std::os::unix::fs::PermissionsExt::from_mode(0o700));
                return Ok(Workspace {
                    base,
                    temporary: false,
                    registry: Some((reg, c.run_id.clone(), c.call_id.clone())),
                });
            }
        }
    }
    let tmp = std::env::temp_dir();
    let base = tmp.join(format!("lope-call-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
    std::fs::DirBuilder::new().mode(0o700).create(&base).map_err(|e| CallError::Os(e.to_string()))?;
    Ok(Workspace { base, temporary: true, registry: None })
}

use std::os::unix::fs::DirBuilderExt;

/// Run `req` behind a supervisor (FR10, FR11). `timeout` is a total wall
/// deadline covering supervisor startup, launch, I/O and cleanup.
pub fn run_subprocess_group(req: &Request, mut ctx: Option<&mut CallContext>) -> Result<Completed, CallError> {
    if req.command.is_empty() {
        return Err(CallError::Invalid("command must be a non-empty argv list".into()));
    }
    if req.timeout.is_nan() || req.timeout <= 0.0 {
        return Err(CallError::Invalid("timeout must be positive".into()));
    }
    if req.stdout_limit == 0 || req.stderr_limit == 0 {
        return Err(CallError::Invalid("output limits must be positive".into()));
    }
    if let Some(c) = ctx.as_deref_mut() {
        c.cleanup_result = Some("unknown".into());
    }
    let mut env = req.env.clone();
    if ctx.as_deref().is_some_and(|c| c.implementation) {
        let depth = std::env::var("LOPE_IMPLEMENTATION_DEPTH")
            .ok()
            .and_then(|v| lope_core::config::py_int_str(&v))
            .unwrap_or(0);
        env.retain(|(k, _)| k != "LOPE_IMPLEMENTATION_DEPTH");
        env.push(("LOPE_IMPLEMENTATION_DEPTH".into(), Some((depth + 1).to_string())));
    }
    check_argv_size(&req.command, &env)?;

    let started = Instant::now();
    let call_deadline = started + Duration::from_secs_f64(req.timeout);
    let ws = workspace(ctx.as_deref())?;
    let result_path = ws.base.join("supervisor-result.json");
    let marker = uuid::Uuid::new_v4().simple().to_string();
    let mut owned: Vec<String> = ctx.as_deref().map(|c| c.owned_paths.clone()).unwrap_or_default();
    owned.push(result_path.display().to_string());
    if let Some((reg, run_id, call_id)) = &ws.registry {
        let mut ch = Map::new();
        ch.insert("state".into(), json!("supervising"));
        ch.insert("owned_paths".into(), json!(owned));
        ch.insert("command_hash".into(), json!(sha256(format!("{}\0<redacted-args>", req.command[0]).as_bytes())));
        ch.insert("executable_hash".into(), json!(sha256(req.command[0].as_bytes())));
        let _ = reg.update_call(run_id, call_id, ch);
    }
    let env_obj: Map<String, Value> =
        env.iter().map(|(k, v)| (k.clone(), v.clone().map(Value::from).unwrap_or(Value::Null))).collect();
    let spec = json!({
        "command": req.command,
        "input_text": req.input_text.clone().unwrap_or_default(),
        "timeout": req.timeout,
        "cwd": req.cwd.as_ref().map(|p| p.display().to_string()),
        "env": env_obj,
        "stdout_limit": req.stdout_limit,
        "stderr_limit": req.stderr_limit,
        "ownership_marker": marker,
        "registry_root": ws.registry.as_ref().map(|(r, _, _)| r.root.display().to_string()).unwrap_or_default(),
        "run_id": ws.registry.as_ref().map(|(_, r, _)| r.clone()).unwrap_or_default(),
        "call_id": ws.registry.as_ref().map(|(_, _, c)| c.clone()).unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()),
        "owned_paths": owned,
        // Lope v2 addition: the caller's absolute wall deadline, so the
        // supervisor classifies provider_timeout itself instead of racing
        // the caller's cancel (Python ignores unknown spec keys).
        "deadline_at": lope_core::time::now() + call_deadline.saturating_duration_since(Instant::now()).as_secs_f64(),
    });
    let wire = pyjson::dumps(&spec, &pyjson::Opts { separators: Some((",", ":")), ..Default::default() }) + "\n";

    let cleanup_ws = |ws: &Workspace| {
        let _ = std::fs::remove_file(&result_path);
        if ws.temporary {
            let _ = std::fs::remove_dir_all(&ws.base);
        } else {
            let _ = std::fs::remove_dir(&ws.base);
        }
    };

    let mut cmd = Command::new(supervisor_exe());
    cmd.arg0("lope-supervisor").arg(&result_path).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            cleanup_ws(&ws);
            return Err(CallError::Os(format!("cannot start supervisor: {e}")));
        }
    };
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let pid = child.id() as i32;
    std::thread::spawn(move || {
        let mut st = 0;
        loop {
            let r = unsafe { libc::waitpid(pid, &mut st, 0) };
            if r == pid || (r < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR)) {
                let _ = tx.send(());
                return;
            }
        }
    });
    let stdin = child.stdin.take().unwrap();
    let fd = stdin.as_raw_fd();
    let wire_complete = write_deadline(fd, wire.as_bytes(), call_deadline);
    let exited = wire_complete && wait_exit(&rx, call_deadline);
    let parent_timed_out = !exited;
    if !exited {
        write_deadline(fd, b"cancel\n", Instant::now() + Duration::from_millis(100));
        // Python reserves 1.0 s for a 0.7 s cleanup; keep the same margin
        // over the DEC-4 budget.
        let reserve = SUPERVISOR_CLEANUP_RESERVE.max(crate::cleanup_budget() + 0.5);
        if !wait_exit(&rx, Instant::now() + Duration::from_secs_f64(reserve)) {
            kill_supervisor(&child, &rx);
        }
    }
    drop(stdin);
    std::mem::forget(child); // reaped by the waiter thread

    let result = (|| -> Result<Completed, CallError> {
        if !result_path.is_file() {
            return Err(CallError::Timeout {
                timeout: req.timeout,
                stdout: String::new(),
                stderr: String::new(),
                diag_path: None,
            });
        }
        let text = std::fs::read_to_string(&result_path).map_err(|e| CallError::BadResult(e.to_string()))?;
        let r = pyjson::loads(&text).map_err(|e| CallError::BadResult(e.to_string()))?;
        if std::env::var("LOPE_DEBUG_SUPERVISOR").as_deref() == Ok("1") {
            eprintln!("lope supervisor result: {text}");
        }
        let s = |k: &str| r.get(k).filter(|v| !v.is_null()).map(lope_core::config::py_str).unwrap_or_default();
        let outcome = s("outcome");
        let cleanup = match r.get("cleanup_result") {
            Some(v) if !v.is_null() && v != "" => lope_core::config::py_str(v),
            _ => "unknown".into(),
        };
        if let Some(c) = ctx.as_deref_mut() {
            c.cleanup_result = Some(cleanup.clone());
        }
        let (stdout, stderr, reason) = (s("stdout"), s("stderr"), s("reason"));
        if parent_timed_out || outcome == "provider_timeout" || outcome == "cancelled" {
            return Err(CallError::Timeout {
                timeout: req.timeout,
                stdout,
                stderr,
                diag_path: r.get("diag_path").and_then(Value::as_str).map(str::to_string),
            });
        }
        if outcome == "output_limit" {
            let stream = if reason.contains("stderr") { "stderr" } else { "stdout" };
            let limit = if stream == "stderr" { req.stderr_limit } else { req.stdout_limit };
            return Err(CallError::OutputLimit { stream: stream.into(), limit, stdout, stderr });
        }
        if outcome == "launch_error" {
            if reason.contains("FileNotFoundError") || reason.contains("No such file") {
                return Err(CallError::FileNotFound(reason));
            }
            return Err(CallError::Os(if reason.is_empty() { "provider launch failed".into() } else { reason }));
        }
        if outcome == "cleanup_failed" || outcome == "output_drain_timeout" {
            return Err(CallError::Unverified(Box::new(Completion {
                reason: if reason.is_empty() { outcome.clone() } else { reason },
                stdout,
                stderr,
                cleanup_result: cleanup,
                stdout_complete: r.get("stdout_complete").and_then(Value::as_bool).unwrap_or(false),
                stdout_truncated: r.get("stdout_truncated").and_then(Value::as_bool).unwrap_or(false),
                returncode: r.get("returncode").cloned().unwrap_or(Value::Null),
            })));
        }
        let rc = r.get("returncode").and_then(Value::as_i64).unwrap_or(-1);
        Ok(Completed { returncode: rc, stdout, stderr, cleanup_result: cleanup })
    })();
    cleanup_ws(&ws);
    result
}

/// True when this process was started as the supervisor.
pub fn is_supervisor_invocation(argv0: &std::ffi::OsStr) -> bool {
    Path::new(argv0).file_name().is_some_and(|n| n == "lope-supervisor")
}
