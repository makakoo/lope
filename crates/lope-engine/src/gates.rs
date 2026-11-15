//! Generic objective evidence gates (`gates.py`, Sprint S7, `FR34`).

use lope_core::pyjson::{self, Opts};
use lope_core::redaction::redact_text;
use lope_providers::budget::Context;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEFAULT_CONFIG_REL: &str = ".lope/rules.json";
pub const DEFAULT_BASELINE_REL: &str = ".lope/gate-baseline.json";
pub const TAIL_CHARS: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateConfigError(pub String);

impl std::fmt::Display for GateConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GateConfigError {}

#[derive(Debug, Clone)]
pub struct GateSpec {
    pub name: String,
    pub cmd: String,
    pub gate_type: String,
    pub required: bool,
    pub path: Option<String>,
    pub regex: Option<String>,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub min_delta: Option<f64>,
    pub max_delta_drop: Option<f64>,
    pub timeout: Option<i64>,
}

impl GateSpec {
    pub fn from_value(data: &Value) -> Result<Self, GateConfigError> {
        let Some(obj) = data.as_object() else {
            return Err(GateConfigError("gate entry must be an object".to_string()));
        };
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let cmd = obj
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            return Err(GateConfigError("gate missing non-empty name".to_string()));
        }
        if cmd.is_empty() {
            return Err(GateConfigError(format!(
                "gate {name:?} missing non-empty cmd"
            )));
        }
        let gate_type = obj
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("exit")
            .trim()
            .to_string();
        if !matches!(gate_type.as_str(), "exit" | "json_number" | "regex_number") {
            return Err(GateConfigError(format!(
                "gate {name:?} has unsupported type {gate_type:?}"
            )));
        }
        let path = obj
            .get("path")
            .and_then(|v| if v.is_null() { None } else { v.as_str() })
            .map(str::to_string);
        let regex = obj
            .get("regex")
            .and_then(|v| if v.is_null() { None } else { v.as_str() })
            .map(str::to_string);
        if gate_type == "json_number" && path.as_deref().unwrap_or("").is_empty() {
            return Err(GateConfigError(format!(
                "gate {name:?} type json_number requires path"
            )));
        }
        if gate_type == "regex_number" && regex.as_deref().unwrap_or("").is_empty() {
            return Err(GateConfigError(format!(
                "gate {name:?} type regex_number requires regex"
            )));
        }
        Ok(Self {
            name,
            cmd,
            gate_type,
            required: obj.get("required").and_then(Value::as_bool).unwrap_or(true),
            path,
            regex,
            min_value: float_or_none(obj.get("min_value")),
            max_value: float_or_none(obj.get("max_value")),
            min_delta: float_or_none(obj.get("min_delta")),
            max_delta_drop: float_or_none(obj.get("max_delta_drop")),
            timeout: int_or_none(obj.get("timeout")),
        })
    }

    pub fn to_value(&self) -> Value {
        json!({
            "name": self.name,
            "cmd": self.cmd,
            "type": self.gate_type,
            "required": self.required,
            "path": self.path,
            "regex": self.regex,
            "min_value": self.min_value,
            "max_value": self.max_value,
            "min_delta": self.min_delta,
            "max_delta_drop": self.max_delta_drop,
            "timeout": self.timeout,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GateResult {
    pub name: String,
    pub ok: bool,
    pub required: bool,
    pub gate_type: String,
    pub value: Option<f64>,
    pub exit_code: i32,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

impl GateResult {
    pub fn to_value(&self) -> Value {
        json!({
            "name": self.name,
            "ok": self.ok,
            "required": self.required,
            "type": self.gate_type,
            "value": self.value,
            "exit_code": self.exit_code,
            "stdout_tail": self.stdout_tail,
            "stderr_tail": self.stderr_tail,
            "duration_ms": self.duration_ms,
            "error": self.error,
        })
    }

    pub fn from_value(data: &Value) -> Self {
        let obj = data.as_object();
        Self {
            name: obj
                .and_then(|o| o.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            ok: obj
                .and_then(|o| o.get("ok"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            required: obj
                .and_then(|o| o.get("required"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
            gate_type: obj
                .and_then(|o| o.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("exit")
                .to_string(),
            value: float_or_none(obj.and_then(|o| o.get("value"))),
            exit_code: obj
                .and_then(|o| o.get("exit_code"))
                .and_then(Value::as_i64)
                .unwrap_or(0) as i32,
            stdout_tail: obj
                .and_then(|o| o.get("stdout_tail"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            stderr_tail: obj
                .and_then(|o| o.get("stderr_tail"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            duration_ms: obj
                .and_then(|o| o.get("duration_ms"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            error: obj
                .and_then(|o| o.get("error"))
                .and_then(Value::as_str)
                .map(str::to_string),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateComparison {
    pub name: String,
    pub passed: bool,
    pub required: bool,
    pub before: Option<GateResult>,
    pub after: GateResult,
    pub delta: Option<f64>,
    pub reason: String,
}

impl GateComparison {
    pub fn to_value(&self) -> Value {
        json!({
            "name": self.name,
            "passed": self.passed,
            "required": self.required,
            "before": self.before.as_ref().map(GateResult::to_value),
            "after": self.after.to_value(),
            "delta": self.delta,
            "reason": self.reason,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GateRun {
    pub mode: String,
    pub project_root: String,
    pub config_path: Option<String>,
    pub baseline_path: String,
    pub passed: bool,
    pub results: Vec<GateResult>,
    pub comparisons: Vec<GateComparison>,
    pub duration_ms: u64,
}

impl GateRun {
    pub fn to_value(&self) -> Value {
        json!({
            "mode": self.mode,
            "project_root": self.project_root,
            "config_path": self.config_path,
            "baseline_path": self.baseline_path,
            "passed": self.passed,
            "duration_ms": self.duration_ms,
            "results": self.results.iter().map(GateResult::to_value).collect::<Vec<_>>(),
            "comparisons": self.comparisons.iter().map(GateComparison::to_value).collect::<Vec<_>>(),
        })
    }

    pub fn blocking_failures(&self) -> Vec<String> {
        if !self.comparisons.is_empty() {
            return self
                .comparisons
                .iter()
                .filter(|c| c.required && !c.passed)
                .map(|c| {
                    if c.reason.is_empty() {
                        c.name.clone()
                    } else {
                        c.reason.clone()
                    }
                })
                .collect();
        }
        self.results
            .iter()
            .filter(|r| r.required && !r.ok)
            .map(|r| {
                r.error
                    .clone()
                    .unwrap_or_else(|| format!("{} failed", r.name))
            })
            .collect()
    }
}

pub fn load_gate_specs(
    config_path: Option<&str>,
    cwd: Option<&Path>,
) -> Result<(Vec<GateSpec>, PathBuf), GateConfigError> {
    let root_buf = match cwd {
        Some(c) => c.to_path_buf(),
        None => std::env::current_dir().unwrap_or_default(),
    };
    let root = fs::canonicalize(&root_buf).unwrap_or(root_buf);
    let mut path = match config_path {
        Some(p) => expand_tilde(p),
        None => root.join(DEFAULT_CONFIG_REL),
    };
    if !path.is_absolute() {
        path = root.join(path);
    }
    if !path.exists() {
        return Ok((Vec::new(), path));
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| GateConfigError(format!("{}: cannot read: {e}", path.display())))?;
    let payload: Value = serde_json::from_str(&raw)
        .map_err(|e| GateConfigError(format!("{}: invalid JSON: {e}", path.display())))?;
    let Some(obj) = payload.as_object() else {
        return Err(GateConfigError(format!(
            "{}: root must be an object",
            path.display()
        )));
    };
    let gates_val = obj.get("gates").unwrap_or(&Value::Null);
    if gates_val.is_null() {
        return Ok((Vec::new(), path));
    }
    let Some(arr) = gates_val.as_array() else {
        return Err(GateConfigError(format!(
            "{}: gates must be a list",
            path.display()
        )));
    };
    let mut specs = Vec::with_capacity(arr.len());
    for g in arr {
        specs.push(GateSpec::from_value(g)?);
    }
    Ok((specs, path))
}

pub fn default_baseline_path(cwd: Option<&Path>) -> PathBuf {
    let root_buf = match cwd {
        Some(c) => c.to_path_buf(),
        None => std::env::current_dir().unwrap_or_default(),
    };
    let root = fs::canonicalize(&root_buf).unwrap_or(root_buf);
    root.join(DEFAULT_BASELINE_REL)
}

fn skipped_gate(spec: &GateSpec, reason: &str) -> GateResult {
    GateResult {
        name: spec.name.clone(),
        ok: false,
        required: spec.required,
        gate_type: spec.gate_type.clone(),
        value: None,
        exit_code: 124,
        stdout_tail: String::new(),
        stderr_tail: String::new(),
        duration_ms: 0,
        error: Some(reason.to_string()),
    }
}

pub fn run_gates(
    specs: &[GateSpec],
    cwd: Option<&Path>,
    default_timeout: i64,
    ctx: Option<&Context>,
) -> Vec<GateResult> {
    let root_buf = match cwd {
        Some(c) => c.to_path_buf(),
        None => std::env::current_dir().unwrap_or_default(),
    };
    let root = fs::canonicalize(&root_buf).unwrap_or(root_buf);
    let mut results = Vec::with_capacity(specs.len());
    for spec in specs {
        if let Some(c) = ctx {
            if c.budget.remaining(false) <= 0.0 {
                results.push(skipped_gate(
                    spec,
                    "run_budget_exhausted: gate skipped after command deadline",
                ));
                continue;
            }
        }
        results.push(run_gate(spec, &root, default_timeout, ctx));
    }
    results
}

pub fn run_gate(
    spec: &GateSpec,
    cwd: &Path,
    default_timeout: i64,
    ctx: Option<&Context>,
) -> GateResult {
    let started = Instant::now();
    let configured_timeout = spec.timeout.unwrap_or(default_timeout).max(1) as f64;
    let mut timeout = configured_timeout;
    let mut clamped_to_budget = false;
    if let Some(c) = ctx {
        let remaining = c.budget.remaining(false);
        if remaining <= 0.0 {
            return skipped_gate(
                spec,
                "run_budget_exhausted: gate skipped after command deadline",
            );
        }
        if remaining < timeout {
            timeout = remaining;
            clamped_to_budget = true;
        }
    }

    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", &spec.cmd])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return GateResult {
                name: spec.name.clone(),
                ok: false,
                required: spec.required,
                gate_type: spec.gate_type.clone(),
                value: None,
                exit_code: 127,
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                duration_ms: started.elapsed().as_millis() as u64,
                error: Some(redact_text(&format!("OSError: {e}")).trim().to_string()),
            };
        }
    };
    let pgid = child.id() as i32;
    let mut stdout_pipe = child.stdout.take().unwrap();
    let mut stderr_pipe = child.stderr.take().unwrap();
    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });

    let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.1));
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if Instant::now() >= deadline {
                    timed_out = true;
                    unsafe {
                        libc::killpg(pgid, libc::SIGTERM);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                    unsafe {
                        libc::killpg(pgid, libc::SIGKILL);
                    }
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break None,
        }
    };
    unsafe {
        libc::killpg(pgid, libc::SIGKILL);
    }
    let raw_stdout = out_handle.join().unwrap_or_default();
    let raw_stderr = err_handle.join().unwrap_or_default();
    let duration_ms = started.elapsed().as_millis() as u64;

    if timed_out {
        return GateResult {
            name: spec.name.clone(),
            ok: false,
            required: spec.required,
            gate_type: spec.gate_type.clone(),
            value: None,
            exit_code: 124,
            stdout_tail: tail(&raw_stdout),
            stderr_tail: tail(&raw_stderr),
            duration_ms,
            error: Some(if clamped_to_budget {
                format!("run_budget_exhausted: gate deadline reached after {timeout}s")
            } else {
                format!("timeout after {timeout}s")
            }),
        };
    }

    let rc = status.and_then(|s| s.code()).unwrap_or(1);
    let mut ok = rc == 0;
    let mut value: Option<f64> = None;
    let mut error: Option<String> = None;

    if ok && spec.gate_type == "json_number" {
        match extract_json_number(&raw_stdout, spec.path.as_deref().unwrap_or("")) {
            Ok(v) => value = Some(v),
            Err(e) => {
                ok = false;
                error = Some(e.0);
            }
        }
    } else if ok && spec.gate_type == "regex_number" {
        let combined = format!("{raw_stdout}\n{raw_stderr}");
        match extract_regex_number(&combined, spec.regex.as_deref().unwrap_or("")) {
            Ok(v) => value = Some(v),
            Err(e) => {
                ok = false;
                error = Some(e.0);
            }
        }
    }

    if ok {
        if let Some(val) = value {
            let (thresh_ok, thresh_err) = check_value_thresholds(spec, val);
            ok = thresh_ok;
            if let Some(te) = thresh_err {
                error = Some(te);
            }
        }
    }
    if rc != 0 && error.is_none() {
        error = Some(format!("exit code {rc}"));
    }

    GateResult {
        name: spec.name.clone(),
        ok,
        required: spec.required,
        gate_type: spec.gate_type.clone(),
        value,
        exit_code: rc,
        stdout_tail: tail(&raw_stdout),
        stderr_tail: tail(&raw_stderr),
        duration_ms,
        error: error.map(|e| redact_text(&e).trim().to_string()),
    }
}

pub fn save_baseline(
    results: &[GateResult],
    path: Option<&Path>,
    cwd: Option<&Path>,
) -> std::io::Result<PathBuf> {
    let baseline = match path {
        Some(p) => p.to_path_buf(),
        None => default_baseline_path(cwd),
    };
    if let Some(parent) = baseline.parent() {
        fs::create_dir_all(parent)?;
    }
    let now_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = json!({
        "version": 1,
        "created_at": now_s,
        "results": results.iter().map(GateResult::to_value).collect::<Vec<_>>(),
    });
    let tmp = baseline.with_extension("json.tmp");
    let text = format!("{}\n", pyjson::dumps(&payload, &Opts::pretty_sorted()));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &baseline)?;
    Ok(baseline)
}

pub fn load_baseline(
    path: Option<&Path>,
    cwd: Option<&Path>,
) -> Result<BTreeMap<String, GateResult>, GateConfigError> {
    let baseline = match path {
        Some(p) => p.to_path_buf(),
        None => default_baseline_path(cwd),
    };
    if !baseline.exists() {
        return Err(GateConfigError(format!(
            "baseline not found at {}; run `lope gate save` first",
            baseline.display()
        )));
    }
    let raw = fs::read_to_string(&baseline)
        .map_err(|e| GateConfigError(format!("{}: cannot read: {e}", baseline.display())))?;
    let payload: Value = serde_json::from_str(&raw)
        .map_err(|e| GateConfigError(format!("{}: invalid baseline JSON: {e}", baseline.display())))?;
    let Some(results) = payload.get("results").and_then(Value::as_array) else {
        return Err(GateConfigError(format!(
            "{}: baseline results must be a list",
            baseline.display()
        )));
    };
    let mut out = BTreeMap::new();
    for item in results {
        let r = GateResult::from_value(item);
        if !r.name.is_empty() {
            out.insert(r.name.clone(), r);
        }
    }
    Ok(out)
}

pub fn compare_results(
    specs: &[GateSpec],
    before: &BTreeMap<String, GateResult>,
    after: &[GateResult],
) -> Vec<GateComparison> {
    let by_name: BTreeMap<&str, &GateSpec> = specs.iter().map(|s| (s.name.as_str(), s)).collect();
    let mut out = Vec::with_capacity(after.len());
    for current in after {
        let spec = by_name.get(current.name.as_str()).copied();
        let required = spec.map(|s| s.required).unwrap_or(current.required);
        let prior = before.get(&current.name).cloned();
        let mut passed = true;
        let mut reason = String::new();
        let mut delta: Option<f64> = None;

        if !current.ok {
            passed = false;
            reason = current
                .error
                .clone()
                .unwrap_or_else(|| format!("{} failed", current.name));
        } else if prior.is_none() {
            passed = true;
            reason = "no baseline for gate".to_string();
        } else if let (Some(cv), Some(pv)) = (current.value, prior.as_ref().and_then(|p| p.value))
        {
            let d = cv - pv;
            delta = Some(d);
            if let Some(sp) = spec {
                if let Some(min_d) = sp.min_delta {
                    if d < min_d {
                        passed = false;
                        reason = format!(
                            "delta {} below min_delta {}",
                            fmt4g(d),
                            fmt4g(min_d)
                        );
                    }
                }
                if let Some(max_drop) = sp.max_delta_drop {
                    if (pv - cv) > max_drop {
                        passed = false;
                        reason = format!(
                            "drop {} exceeds max_delta_drop {}",
                            fmt4g(pv - cv),
                            fmt4g(max_drop)
                        );
                    }
                }
            }
            if reason.is_empty() {
                reason = format!("delta {}", fmt4g(d));
            }
        } else {
            reason = if current.ok {
                "exit status unchanged".to_string()
            } else {
                current.error.clone().unwrap_or_else(|| "failed".to_string())
            };
        }

        out.push(GateComparison {
            name: current.name.clone(),
            passed,
            required,
            before: prior,
            after: current.clone(),
            delta,
            reason,
        });
    }
    out
}

pub fn build_run(
    mode: &str,
    _specs: &[GateSpec],
    config_path: Option<&Path>,
    baseline_path: &Path,
    results: Vec<GateResult>,
    comparisons: Vec<GateComparison>,
    started: Instant,
    cwd: Option<&Path>,
) -> GateRun {
    let passed = if !comparisons.is_empty() {
        comparisons.iter().all(|c| c.passed || !c.required)
    } else {
        results.iter().all(|r| r.ok || !r.required)
    };
    let root_buf = match cwd {
        Some(c) => c.to_path_buf(),
        None => std::env::current_dir().unwrap_or_default(),
    };
    let root = fs::canonicalize(&root_buf).unwrap_or(root_buf);
    GateRun {
        mode: mode.to_string(),
        project_root: root.display().to_string(),
        config_path: config_path.map(|p| p.display().to_string()),
        baseline_path: baseline_path.display().to_string(),
        passed,
        results,
        comparisons,
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

pub fn prompt_summary(run: &GateRun) -> String {
    let mut lines = vec!["## Objective gate report".to_string()];
    if run.results.is_empty() && run.comparisons.is_empty() {
        lines.push("- No gates configured.".to_string());
        return lines.join("\n");
    }
    if !run.comparisons.is_empty() {
        for c in &run.comparisons {
            let status = if c.passed { "PASS" } else { "FAIL" };
            if let (Some(bv), Some(av)) = (
                c.before.as_ref().and_then(|b| b.value),
                c.after.value,
            ) {
                lines.push(format!(
                    "- {}: {status}, {} -> {} ({})",
                    c.name,
                    fmt4g(bv),
                    fmt4g(av),
                    c.reason
                ));
            } else {
                lines.push(format!("- {}: {status}, {}", c.name, c.reason));
            }
        }
    } else {
        for r in &run.results {
            let status = if r.ok { "PASS" } else { "FAIL" };
            let value = r
                .value
                .map(|v| format!(" value={}", fmt4g(v)))
                .unwrap_or_default();
            let reason = if r.ok {
                String::new()
            } else {
                format!(" ({})", r.error.as_deref().unwrap_or("failed"))
            };
            lines.push(format!("- {}: {status}{value}{reason}", r.name));
        }
    }
    lines.join("\n")
}

pub fn fmt4g(v: f64) -> String {
    lope_providers::progress::g(v)
}

fn float_or_none(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    if v.is_null() {
        return None;
    }
    if let Some(n) = v.as_f64() {
        return Some(n);
    }
    if let Some(s) = v.as_str() {
        if s.is_empty() {
            return None;
        }
        return s.parse().ok();
    }
    None
}

fn int_or_none(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    if v.is_null() {
        return None;
    }
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    if let Some(s) = v.as_str() {
        if s.is_empty() {
            return None;
        }
        return s.parse().ok();
    }
    None
}

fn tail(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = chars.len().saturating_sub(TAIL_CHARS);
    let sliced: String = chars[start..].iter().collect();
    redact_text(&sliced).trim_end().to_string()
}

fn extract_json_number(text: &str, path: &str) -> Result<f64, GateConfigError> {
    let data: Value = serde_json::from_str(text)
        .map_err(|e| GateConfigError(format!("JSON parse failed: {e}")))?;
    let mut cur = &data;
    for part in path.split('.') {
        if let Some(obj) = cur.as_object() {
            if let Some(next) = obj.get(part) {
                cur = next;
                continue;
            }
        } else if let Some(arr) = cur.as_array() {
            if let Ok(idx) = part.parse::<usize>() {
                if let Some(next) = arr.get(idx) {
                    cur = next;
                    continue;
                }
            }
        }
        return Err(GateConfigError(format!("JSON path {path:?} not found")));
    }
    cur.as_f64()
        .ok_or_else(|| GateConfigError(format!("JSON path {path:?} did not resolve to number")))
}

fn extract_regex_number(text: &str, pattern: &str) -> Result<f64, GateConfigError> {
    let re = Regex::new(&format!("(?m){pattern}"))
        .map_err(|e| GateConfigError(format!("invalid regex {pattern:?}: {e}")))?;
    let caps = re
        .captures(text)
        .ok_or_else(|| GateConfigError("regex did not match".to_string()))?;
    let raw = caps
        .get(1)
        .or_else(|| caps.get(0))
        .map(|m| m.as_str())
        .unwrap_or("");
    raw.parse::<f64>()
        .map_err(|_| GateConfigError(format!("regex capture is not numeric: {raw:?}")))
}

fn check_value_thresholds(spec: &GateSpec, value: f64) -> (bool, Option<String>) {
    if let Some(min_v) = spec.min_value {
        if value < min_v {
            return (
                false,
                Some(format!("value {} below min_value {}", fmt4g(value), fmt4g(min_v))),
            );
        }
    }
    if let Some(max_v) = spec.max_value {
        if value > max_v {
            return (
                false,
                Some(format!("value {} above max_value {}", fmt4g(value), fmt4g(max_v))),
            );
        }
    }
    (true, None)
}

fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(p)
}
