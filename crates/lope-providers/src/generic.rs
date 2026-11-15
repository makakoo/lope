//! Validator trait, `_run_with_group_kill`, generic subprocess providers, and
//! Tier 1 in-process HTTP providers (`generic_validators.py`).

use crate::budget::Context;
use crate::diagnostics::{diagnostic_suffix, redact_diagnostic};
use crate::errors::{tool_permission_denial, ProviderError};
use crate::verdict::{infra_error, parse_opencode_verdict, ValidatorResult};
use lope_core::config::{py_int, py_str};
use lope_core::pyjson;
use lope_runtime::call::{run_subprocess_group, CallError, Request};
use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub trait Validator: Send + Sync {
    fn name(&self) -> &str;
    fn available(&self) -> bool;
    fn generate(&self, prompt: &str, timeout: f64, ctx: Option<&Context>) -> Result<String, ProviderError>;
    fn validate(&self, prompt: &str, timeout: f64, ctx: Option<&Context>) -> ValidatorResult {
        let t0 = lope_core::time::now();
        match self.generate(prompt, timeout, ctx) {
            Ok(raw) => {
                let d = (lope_core::time::now() - t0).max(0.0);
                let v = parse_opencode_verdict(&raw, self.name(), d);
                ValidatorResult {
                    validator_name: self.name().into(),
                    verdict: v,
                    raw_response: raw,
                    error: String::new(),
                    flag_error_hint: String::new(),
                    repair_attempted: false,
                    repair_status: None,
                    repaired_response: String::new(),
                    initial_parse_status: None,
                    parse_error_category: None,
                }
            }
            Err(e) => infra_error(self.name(), &e.to_string(), (lope_core::time::now() - t0).max(0.0)),
        }
    }
    /// request_plan transport: argv | stdin | file | http
    fn transport(&self) -> &'static str;
    fn timeout_override(&self) -> Option<i64> {
        None
    }
    fn execution_profile(&self, _mode: &str) -> Option<Value> {
        None
    }
    fn command_template(&self) -> Vec<String> {
        vec![self.name().to_string()]
    }
}

pub fn respect_provider_timeout() -> bool {
    matches!(
        std::env::var("LOPE_RESPECT_PROVIDER_TIMEOUT").unwrap_or_default().trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Python `str(float)` for timeouts in messages (leases are floats).
pub fn num(x: f64) -> String {
    lope_core::pyjson::float_repr(x)
}

pub fn workdir() -> String {
    std::env::var("LOPE_WORKDIR")
        .ok()
        .unwrap_or_else(|| std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default())
}

pub fn which_or(name: &str) -> String {
    lope_core::paths::which(name).map(|p| p.display().to_string()).unwrap_or_else(|| name.to_string())
}

pub struct Run {
    pub returncode: i64,
    pub stdout: String,
    pub stderr: String,
}

/// Raw supervised call with the call context wired in.
pub fn run(
    cmd: Vec<String>,
    input: Option<String>,
    timeout: f64,
    cwd: &str,
    env: Vec<(String, Option<String>)>,
    ctx: Option<&Context>,
) -> Result<Run, CallError> {
    let mut req = Request::new(cmd, timeout);
    req.input_text = input;
    req.cwd = Some(cwd.into());
    req.env = env;
    let mut cc = ctx.map(Context::call_context);
    let r = run_subprocess_group(&req, cc.as_mut());
    if let (Some(c), Some(cc)) = (ctx, cc) {
        *c.cleanup_result.lock().unwrap() = cc.cleanup_result;
    }
    r.map(|c| Run { returncode: c.returncode, stdout: c.stdout, stderr: c.stderr })
}

/// `_bounded_stream` + `_format_process_diagnostics` (unredacted, 500 chars).
pub fn process_diagnostics(stdout: &str, stderr: &str) -> String {
    let b = |v: &str| {
        let t = crate::pyre::strip(v);
        if crate::pyre::char_len(t) <= 500 {
            t.to_string()
        } else {
            format!("...[truncated]...{}", crate::pyre::tail(t, 500))
        }
    };
    let mut parts = Vec::new();
    let (o, e) = (b(stdout), b(stderr));
    if !o.is_empty() {
        parts.push(format!("stdout: {o}"));
    }
    if !e.is_empty() {
        parts.push(format!("stderr: {e}"));
    }
    parts.join("; ")
}

/// `_run_with_group_kill`: translate runtime errors to provider errors.
pub fn run_with_group_kill(
    cmd: Vec<String>,
    input: Option<String>,
    timeout: f64,
    cwd: &str,
    ctx: Option<&Context>,
) -> Result<Run, ProviderError> {
    let c0 = cmd[0].clone();
    run(cmd, input, timeout, cwd, vec![], ctx).map_err(|e| match e {
        CallError::Timeout { stdout, stderr, .. } => {
            let d = process_diagnostics(&stdout, &stderr);
            let suffix = if d.is_empty() { String::new() } else { format!("; {d}") };
            ProviderError::Infra(format!("{c0} run timed out after {}s (process group killed){suffix}", num(timeout)))
        }
        CallError::FileNotFound(_) => ProviderError::Infra(format!("{c0} binary not found")),
        CallError::OutputLimit { stream, limit, .. } => {
            ProviderError::Infra(format!("{c0} output limit: {stream} exceeded {limit} bytes"))
        }
        CallError::Unverified(c) => ProviderError::Infra(format!(
            "{c0} subprocess failure: {}{}",
            redact_diagnostic(c.message()),
            diagnostic_suffix(&c.stdout, &c.stderr)
        )),
        other => ProviderError::Infra(format!("{c0} subprocess failure: {}", redact_diagnostic(&other.to_string()))),
    })
}

/// `_validate_provider_config(entry)` (`generic_validators.py:120-169`).
pub fn validate_provider_config(entry: &Value) -> Result<(), String> {
    let Some(obj) = entry.as_object() else {
        let ty = match entry {
            Value::Array(_) => "list",
            Value::String(_) => "str",
            Value::Bool(_) => "bool",
            Value::Number(n) if n.is_f64() => "float",
            Value::Number(_) => "int",
            Value::Null => "NoneType",
            Value::Object(_) => "dict",
        };
        return Err(format!("provider entry must be a dict, got {ty}"));
    };
    let Some(name) = obj.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()) else {
        return Err("provider missing 'name' field".into());
    };
    let ptype = obj.get("type").and_then(Value::as_str);
    match ptype {
        Some("subprocess") => {
            let Some(cmd) = obj.get("command").and_then(Value::as_array) else {
                return Err(format!("provider {name:?} command must be a list of strings"));
            };
            if !cmd.iter().all(Value::is_string) {
                return Err(format!("provider {name:?} command must be a list of strings"));
            }
            let cmd_strs: Vec<&str> = cmd.iter().filter_map(Value::as_str).collect();
            if cmd_strs.join(" ").contains("${") {
                return Err(format!(
                    "provider {name:?}: ${{VAR}} not allowed in command (API keys in argv are visible via ps)"
                ));
            }
            let review_cmd = obj.get("review_command").filter(|v| !v.is_null());
            let mut rev_strs: Option<Vec<&str>> = None;
            if let Some(rc) = review_cmd {
                let Some(arr) = rc.as_array() else {
                    return Err(format!("provider {name:?} review_command must be a nonempty argv list"));
                };
                if arr.is_empty()
                    || !arr.iter().all(Value::is_string)
                    || arr[0].as_str().unwrap_or("").trim().is_empty()
                {
                    return Err(format!("provider {name:?} review_command must be a nonempty argv list"));
                }
                let rs: Vec<&str> = arr.iter().filter_map(Value::as_str).collect();
                if rs.join(" ").contains("${") {
                    return Err(format!("provider {name:?}: ${{VAR}} not allowed in review_command"));
                }
                rev_strs = Some(rs);
            }
            let rwd = obj.get("review_workdir").and_then(Value::as_str).unwrap_or("caller");
            if !matches!(rwd, "caller" | "isolated") {
                return Err(format!("provider {name:?} review_workdir must be caller or isolated"));
            }
            if let Some(rs) = obj.get("review_stdin") {
                if !rs.is_boolean() {
                    return Err(format!("provider {name:?} review_stdin must be a boolean"));
                }
            }
            let selected_review: &[&str] = rev_strs.as_deref().unwrap_or(&cmd_strs);
            let has_review_profile = obj.contains_key("review_command")
                || obj.contains_key("review_stdin")
                || obj.contains_key("review_workdir");
            let stdin_flag = obj.get("stdin").and_then(Value::as_bool).unwrap_or(false);
            let review_stdin = obj.get("review_stdin").and_then(Value::as_bool).unwrap_or(stdin_flag);
            if has_review_profile && !review_stdin && !selected_review.iter().any(|a| a.contains("{prompt}")) {
                return Err(format!("provider {name:?}: review profile needs stdin or a {{prompt}} argument"));
            }
            if rwd == "isolated" && !selected_review.is_empty() {
                let exe = selected_review[0];
                let has_dir = std::path::Path::new(exe)
                    .parent()
                    .is_some_and(|p| !p.as_os_str().is_empty());
                if has_dir && !std::path::Path::new(exe).is_absolute() {
                    return Err(format!(
                        "provider {name:?}: isolated review requires an absolute executable or PATH name"
                    ));
                }
            }
            Ok(())
        }
        Some("http") => {
            let Some(url) = obj.get("url").and_then(Value::as_str) else {
                return Err(format!("provider {name:?} url must be http:// or https://"));
            };
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(format!("provider {name:?} url must be http:// or https://"));
            }
            if url.contains("${") {
                return Err(format!("provider {name:?}: ${{VAR}} not allowed in url (leaks to server logs)"));
            }
            if let Some(rl) = obj.get("response_limit") {
                let lim = py_int(rl).unwrap_or(0);
                if lim <= 0 {
                    return Err(format!("provider {name:?} response_limit must be positive"));
                }
            }
            Ok(())
        }
        other => {
            let got = other.map(|s| format!("{s:?}")).unwrap_or_else(|| "None".into());
            Err(format!("provider {name:?} type must be 'subprocess' or 'http', got {got}"))
        }
    }
}

pub fn expand_env_str(s: &str) -> String {
    let re = regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)\}").unwrap();
    re.replace_all(s, |caps: &regex::Captures<'_>| {
        std::env::var(&caps[1]).unwrap_or_default()
    })
    .into_owned()
}

pub fn expand_env_value(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(expand_env_str(s)),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, val) in m {
                out.insert(k.clone(), expand_env_value(val));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(expand_env_value).collect()),
        other => other.clone(),
    }
}

pub fn substitute_prompt(obj: &Value, prompt: &str, max_tokens: Option<i64>) -> Value {
    match obj {
        Value::String(s) => {
            let mut r = s.replace("{prompt}", prompt);
            if let Some(mt) = max_tokens {
                if r.contains("{max_tokens}") {
                    r = r.replace("{max_tokens}", &mt.to_string());
                }
            }
            Value::String(r)
        }
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                out.insert(k.clone(), substitute_prompt(v, prompt, max_tokens));
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| substitute_prompt(v, prompt, max_tokens)).collect())
        }
        other => other.clone(),
    }
}

pub fn extract_response(data: &Value, path: Option<&str>) -> String {
    let Some(path) = path else {
        return match data {
            Value::String(s) => s.clone(),
            other => py_str(other),
        };
    };
    let mut cur = data;
    for segment in path.split('.') {
        if segment.chars().all(|c| c.is_ascii_digit()) && !segment.is_empty() {
            if let Value::Array(arr) = cur {
                let Ok(idx) = segment.parse::<usize>() else { return String::new() };
                let Some(next) = arr.get(idx) else { return String::new() };
                cur = next;
                continue;
            }
        }
        if let Value::Object(map) = cur {
            let Some(next) = map.get(segment) else { return String::new() };
            cur = next;
        } else {
            return String::new();
        }
    }
    match cur {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => py_str(other),
    }
}

/// `GenericSubprocessValidator`.
pub struct Subprocess {
    pub name: String,
    pub command: Vec<String>,
    pub stdin: bool,
    pub prompt_wrapper: Option<String>,
    pub timeout_override: Option<i64>,
    pub review_command: Option<Vec<String>>,
    pub review_workdir: String,
    pub review_stdin: bool,
    pub has_review_profile: bool,
}

impl Subprocess {
    /// Why this provider cannot run natively yet (the verb then delegates).
    pub fn unsupported(&self) -> Option<String> {
        match &self.prompt_wrapper {
            Some(w) if w.replace("{prompt}", "").contains(['{', '}']) => {
                Some(format!("{}: prompt_wrapper format fields", self.name))
            }
            _ => None,
        }
    }

    pub fn from_config(e: &Value) -> Result<Self, String> {
        validate_provider_config(e)?;
        let o = e.as_object().unwrap();
        let name = o.get("name").and_then(Value::as_str).unwrap().to_string();
        let command: Vec<String> = o
            .get("command")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        let stdin = crate::validity::truthy(o.get("stdin"));
        let review_command = match o.get("review_command") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_str().unwrap().to_string())
                    .collect(),
            ),
        };
        let review_workdir = o
            .get("review_workdir")
            .and_then(Value::as_str)
            .unwrap_or("caller")
            .to_string();
        let review_stdin = match o.get("review_stdin") {
            None => stdin,
            Some(Value::Bool(b)) => *b,
            Some(_) => stdin,
        };
        let has_review_profile =
            o.contains_key("review_command") || o.contains_key("review_stdin") || o.contains_key("review_workdir");
        Ok(Subprocess {
            name,
            command,
            stdin,
            prompt_wrapper: o.get("prompt_wrapper").and_then(Value::as_str).map(str::to_string),
            timeout_override: o.get("timeout").and_then(lope_core::config::py_int),
            review_command,
            review_workdir,
            review_stdin,
            has_review_profile,
        })
    }

    fn effective_timeout(&self, call: f64) -> f64 {
        match self.timeout_override {
            None => call,
            Some(p) if respect_provider_timeout() => p as f64,
            Some(p) => (p as f64).min(call),
        }
    }

    fn permission(&self, stderr: &str) -> Option<ProviderError> {
        let p = tool_permission_denial(stderr);
        (!p.is_empty()).then(|| {
            ProviderError::ToolPermission(format!(
                "{} tool_permission_denied: {p} requires approval unavailable in headless mode. Lope will not retry, bypass, or grant this permission. Use another validator or review the requested action manually. The CLI diagnostic does not identify the requested command.",
                self.name
            ))
        })
    }
}

impl Validator for Subprocess {
    fn name(&self) -> &str {
        &self.name
    }
    fn available(&self) -> bool {
        !self.command.is_empty() && lope_core::paths::which(&self.command[0]).is_some()
    }
    fn transport(&self) -> &'static str {
        if self.stdin {
            "stdin"
        } else {
            "argv"
        }
    }
    fn timeout_override(&self) -> Option<i64> {
        self.timeout_override
    }
    fn command_template(&self) -> Vec<String> {
        self.command.clone()
    }
    fn execution_profile(&self, mode: &str) -> Option<Value> {
        let review = mode == "review";
        let cmd = if review { self.review_command.as_ref().unwrap_or(&self.command) } else { &self.command };
        let available = cmd.first().is_some_and(|b| lope_core::paths::which(b).is_some());
        let isolated = review && self.review_workdir == "isolated";
        let configured = review && self.has_review_profile;
        let mut warnings = Vec::new();
        if !available {
            warnings.push(format!("{}: selected provider executable is unavailable", self.name));
        }
        if review {
            warnings.push(format!(
                "{}: review uses {} cwd; CLI tool permissions are unchanged (not a tool sandbox). Headless tool approval can fail; no automatic permission bypass.",
                self.name,
                if isolated { "isolated temporary" } else { "caller" }
            ));
        }
        Some(serde_json::json!({
            "validator": self.name, "review_profile_configured": configured,
            "workdir": if isolated { "isolated" } else { "caller" }, "launchable": available,
            "tool_policy": "provider_owned_unchanged", "warnings": warnings,
        }))
    }
    fn generate(&self, prompt: &str, timeout: f64, ctx: Option<&Context>) -> Result<String, ProviderError> {
        let wrapped;
        let prompt = match &self.prompt_wrapper {
            Some(w) => {
                wrapped = w.replace("{prompt}", prompt);
                wrapped.as_str()
            }
            None => prompt,
        };
        let review = ctx.is_some_and(|c| c.mode == "review");
        let command = if review { self.review_command.as_ref().unwrap_or(&self.command) } else { &self.command };
        let stdin = if review { self.review_stdin } else { self.stdin };
        let (cmd, input) = if stdin {
            (command.clone(), Some(prompt.to_string()))
        } else {
            (command.iter().map(|a| a.replace("{prompt}", prompt)).collect(), None)
        };
        let eff = self.effective_timeout(timeout);
        let n = &self.name;
        struct TempReviewDir(std::path::PathBuf);
        impl Drop for TempReviewDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let iso = if review && self.review_workdir == "isolated" {
            let hex = &uuid::Uuid::new_v4().simple().to_string()[..16];
            let dir = std::env::temp_dir().join(format!("lope-review-cwd-{hex}"));
            std::fs::create_dir_all(&dir).map_err(|e| {
                ProviderError::Runtime(format!("{n} subprocess error: {}", redact_diagnostic(&e.to_string())))
            })?;
            Some(TempReviewDir(dir))
        } else {
            None
        };
        let cwd_str = iso
            .as_ref()
            .map(|d| d.0.display().to_string())
            .unwrap_or_else(workdir);
        let env = iso
            .as_ref()
            .map(|d| vec![("LOPE_WORKDIR".to_string(), Some(d.0.display().to_string()))])
            .unwrap_or_default();
        let res = run(cmd, input, eff, &cwd_str, env, ctx);
        drop(iso);
        match res {
            Err(CallError::Timeout { stdout, stderr, .. }) => {
                if let Some(d) = self.permission(&stderr) {
                    return Err(d);
                }
                Err(ProviderError::Runtime(format!(
                    "{n} timed out after {}s{}",
                    num(eff),
                    diagnostic_suffix(&stdout, &stderr)
                )))
            }
            Err(CallError::FileNotFound(_)) => {
                let exe = command.first().map(String::as_str).unwrap_or("");
                Err(ProviderError::Runtime(format!("{n} binary not found: {}", redact_diagnostic(exe))))
            }
            Err(CallError::Unverified(c)) => {
                if let Some(d) = self.permission(&c.stderr) {
                    return Err(d);
                }
                Err(ProviderError::Unverified {
                    message: c.message().to_string(),
                    stdout: c.stdout.clone(),
                    cleanup: c.cleanup_result.clone(),
                    stdout_complete: c.stdout_complete,
                    stdout_truncated: c.stdout_truncated,
                    returncode: c.returncode.clone(),
                })
            }
            Err(CallError::OutputLimit { stream, limit, stdout, stderr }) => {
                if let Some(d) = self.permission(&stderr) {
                    return Err(d);
                }
                Err(ProviderError::Runtime(format!(
                    "{n} subprocess error: {stream} exceeded {limit} bytes{}",
                    diagnostic_suffix(&stdout, &stderr)
                )))
            }
            Err(e) => {
                Err(ProviderError::Runtime(format!("{n} subprocess error: {}", redact_diagnostic(&e.to_string()))))
            }
            Ok(r) => {
                if let Some(d) = self.permission(&r.stderr) {
                    return Err(d);
                }
                if r.returncode != 0 {
                    return Err(ProviderError::Runtime(format!(
                        "{n} exited {}{}",
                        r.returncode,
                        diagnostic_suffix(&r.stdout, &r.stderr)
                    )));
                }
                if r.stdout.trim().is_empty() {
                    return Err(ProviderError::Runtime(format!(
                        "{n} returned empty output{}",
                        diagnostic_suffix(&r.stdout, &r.stderr)
                    )));
                }
                Ok(r.stdout)
            }
        }
    }
}

/// `GenericHttpValidator` (Tier 1 in-process HTTP).
pub struct Http {
    pub name: String,
    pub url: String,
    pub headers: Map<String, Value>,
    pub body: Value,
    pub response_path: Option<String>,
    pub prompt_wrapper: Option<String>,
    pub timeout_override: Option<i64>,
    pub max_tokens: Option<i64>,
    pub response_limit: usize,
}

impl Http {
    pub fn from_config(entry: &Value) -> Result<Self, String> {
        validate_provider_config(entry)?;
        let o = entry.as_object().unwrap();
        let name = o.get("name").and_then(Value::as_str).unwrap().to_string();
        let url = o.get("url").and_then(Value::as_str).unwrap().to_string();
        let headers = o.get("headers").and_then(Value::as_object).cloned().unwrap_or_default();
        let body = o.get("body").cloned().unwrap_or_else(|| Value::Object(Map::new()));
        let response_path = o.get("response_path").and_then(Value::as_str).map(String::from);
        let prompt_wrapper = o.get("prompt_wrapper").and_then(Value::as_str).map(String::from);
        let timeout_override = o.get("timeout").and_then(py_int);
        let max_tokens = o.get("max_tokens").and_then(py_int);
        let response_limit = o.get("response_limit").and_then(py_int).unwrap_or(2 * 1024 * 1024) as usize;
        Ok(Http {
            name,
            url,
            headers,
            body,
            response_path,
            prompt_wrapper,
            timeout_override,
            max_tokens,
            response_limit,
        })
    }

    fn effective_timeout(&self, call: f64) -> f64 {
        match self.timeout_override {
            None => call,
            Some(p) if respect_provider_timeout() => p as f64,
            Some(p) => (p as f64).min(call),
        }
    }

    pub fn unsupported(&self) -> Option<String> {
        match &self.prompt_wrapper {
            Some(w) if w.replace("{prompt}", "").contains(['{', '}']) => {
                return Some(format!("{}: prompt_wrapper format fields", self.name));
            }
            _ => {}
        }
        if self.body.is_null() || self.body.as_object().is_some_and(|m| m.is_empty()) {
            return Some(format!("{}: http provider without body", self.name));
        }
        if !self.url.starts_with("http://") {
            return Some(format!("{}: https provider delegated", self.name));
        }
        None
    }

    fn request_http(&self, payload: &[u8], headers: &Map<String, Value>, timeout_secs: f64) -> Result<String, String> {
        let deadline = Instant::now() + Duration::from_secs_f64(timeout_secs.max(0.01));
        let rest = self
            .url
            .strip_prefix("http://")
            .ok_or_else(|| "unsupported scheme for in-process HTTP".to_string())?;
        let (host_port, path) = match rest.find('/') {
            Some(idx) => (&rest[..idx], &rest[idx..]),
            None => (rest, "/"),
        };
        let (host, port): (&str, u16) = match host_port.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().map_err(|_| format!("invalid port in URL: {host_port}"))?),
            None => (host_port, 80),
        };
        let rem = || -> Result<Duration, String> {
            let now = Instant::now();
            if now >= deadline {
                Err(format!("timed out after {}s", num(timeout_secs)))
            } else {
                Ok(deadline - now)
            }
        };

        let addrs: Vec<SocketAddr> = (host, port)
            .to_socket_addrs()
            .map_err(|e| format!("URLError: {e}"))?
            .collect();
        let addr = addrs.first().ok_or_else(|| format!("URLError: could not resolve {host}"))?;
        let mut stream = TcpStream::connect_timeout(addr, rem()?).map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                format!("timed out after {}s", num(timeout_secs))
            } else if let Some(code) = e.raw_os_error() {
                let msg = unsafe {
                    let p = libc::strerror(code);
                    if p.is_null() {
                        e.to_string()
                    } else {
                        std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
                    }
                };
                format!("URLError: <urlopen error [Errno {code}] {msg}>")
            } else {
                format!("URLError: <urlopen error {e}>")
            }
        })?;

        let mut req = Vec::with_capacity(payload.len() + 512);
        let _ = write!(
            req,
            "POST {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\nContent-Length: {}\r\n",
            payload.len()
        );
        let mut has_ct = false;
        for (k, v) in headers {
            if k.eq_ignore_ascii_case("host")
                || k.eq_ignore_ascii_case("connection")
                || k.eq_ignore_ascii_case("content-length")
            {
                continue;
            }
            if k.eq_ignore_ascii_case("content-type") {
                has_ct = true;
            }
            let val_str = v.as_str().map(String::from).unwrap_or_else(|| py_str(v));
            let _ = write!(req, "{k}: {val_str}\r\n");
        }
        if !has_ct {
            let _ = write!(req, "Content-Type: application/json\r\n");
        }
        let _ = write!(req, "\r\n");
        req.extend_from_slice(payload);

        stream.set_write_timeout(Some(rem()?)).map_err(|e| e.to_string())?;
        stream.write_all(&req).map_err(|e| {
            if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) {
                format!("timed out after {}s", num(timeout_secs))
            } else {
                format!("IOError: {e}")
            }
        })?;

        let mut buf = Vec::new();
        let mut header_end: Option<usize> = None;
        let mut chunk = [0u8; 8192];
        while header_end.is_none() {
            stream.set_read_timeout(Some(rem()?)).map_err(|e| e.to_string())?;
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                    } else if buf.len() > 64 * 1024 {
                        return Err("HTTP response headers exceeded 64 KiB".into());
                    }
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => {
                    return Err(format!("timed out after {}s", num(timeout_secs)));
                }
                Err(e) => return Err(format!("IOError: {e}")),
            }
        }
        let Some(hend) = header_end else {
            return Err("incomplete HTTP response headers".into());
        };
        let header_str = String::from_utf8_lossy(&buf[..hend]).into_owned();
        let mut lines = header_str.split("\r\n");
        let status_line = lines.next().unwrap_or("");
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("malformed HTTP status line: {status_line:?}"))?;
        let mut content_length: Option<usize> = None;
        let mut retry_after: Option<String> = None;
        let mut chunked = false;
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim();
                let v = v.trim();
                if k.eq_ignore_ascii_case("content-length") {
                    if let Ok(cl) = v.parse::<usize>() {
                        if cl > self.response_limit {
                            return Err(format!(
                                "ValueError: response Content-Length exceeds {} bytes",
                                self.response_limit
                            ));
                        }
                        content_length = Some(cl);
                    }
                } else if k.eq_ignore_ascii_case("retry-after") {
                    retry_after = Some(v.to_string());
                } else if k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked") {
                    chunked = true;
                }
            }
        }

        let mut body_bytes = buf[hend..].to_vec();
        if body_bytes.len() > self.response_limit {
            return Err(format!("ValueError: response body exceeds {} bytes", self.response_limit));
        }

        loop {
            if let Some(cl) = content_length {
                if body_bytes.len() >= cl {
                    body_bytes.truncate(cl);
                    break;
                }
            }
            stream.set_read_timeout(Some(rem()?)).map_err(|e| e.to_string())?;
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    body_bytes.extend_from_slice(&chunk[..n]);
                    if body_bytes.len() > self.response_limit {
                        return Err(format!(
                            "ValueError: response body exceeds {} bytes",
                            self.response_limit
                        ));
                    }
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => {
                    return Err(format!("timed out after {}s", num(timeout_secs)));
                }
                Err(e) => return Err(format!("IOError: {e}")),
            }
        }

        let raw_bytes = if chunked {
            decode_chunked(&body_bytes, self.response_limit)?
        } else {
            body_bytes
        };
        let raw = String::from_utf8_lossy(&raw_bytes).into_owned();
        if !(200..300).contains(&status) {
            let retry_detail = retry_after.map(|r| format!(" Retry-After: {r};")).unwrap_or_default();
            let head: String = raw.chars().take(300).collect();
            return Err(format!("HTTP {status}:{retry_detail} {head}"));
        }
        Ok(raw)
    }
}

fn decode_chunked(mut input: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    while !input.is_empty() {
        let Some(pos) = input.windows(2).position(|w| w == b"\r\n") else { break };
        let len_str = std::str::from_utf8(&input[..pos]).unwrap_or("").trim();
        let len_hex = len_str.split(';').next().unwrap_or("");
        let Ok(chunk_len) = usize::from_str_radix(len_hex, 16) else { break };
        input = &input[pos + 2..];
        if chunk_len == 0 {
            break;
        }
        if input.len() < chunk_len {
            break;
        }
        out.extend_from_slice(&input[..chunk_len]);
        if out.len() > limit {
            return Err(format!("ValueError: response body exceeds {limit} bytes"));
        }
        input = &input[chunk_len..];
        if input.starts_with(b"\r\n") {
            input = &input[2..];
        }
    }
    Ok(out)
}

impl Validator for Http {
    fn name(&self) -> &str {
        &self.name
    }
    fn available(&self) -> bool {
        true
    }
    fn transport(&self) -> &'static str {
        "http"
    }
    fn timeout_override(&self) -> Option<i64> {
        self.timeout_override
    }
    fn command_template(&self) -> Vec<String> {
        Vec::new()
    }
    fn generate(&self, prompt: &str, timeout: f64, _ctx: Option<&Context>) -> Result<String, ProviderError> {
        let prompt = match &self.prompt_wrapper {
            Some(w) => w.replace("{prompt}", prompt),
            None => prompt.to_string(),
        };
        let headers_val = expand_env_value(&Value::Object(self.headers.clone()));
        let headers = headers_val.as_object().cloned().unwrap_or_default();
        let mut body = substitute_prompt(&expand_env_value(&self.body), &prompt, self.max_tokens);
        if let Some(mt) = self.max_tokens {
            if let Some(obj) = body.as_object_mut() {
                if !obj.contains_key("max_tokens") {
                    obj.insert("max_tokens".into(), Value::from(mt));
                }
            }
        }
        let eff = self.effective_timeout(timeout);
        let payload = pyjson::dumps(&body, &pyjson::Opts::default());
        let raw = self
            .request_http(payload.as_bytes(), &headers, eff)
            .map_err(|e| ProviderError::Runtime(format!("{} http error: {e}", self.name)))?;
        let data = pyjson::loads(&raw).unwrap_or(Value::String(raw));
        let text = extract_response(&data, self.response_path.as_deref());
        if text.trim().is_empty() {
            return Err(ProviderError::Runtime(format!("{} returned empty output", self.name)));
        }
        Ok(text)
    }
}
