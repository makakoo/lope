//! Validator trait, `_run_with_group_kill`, and generic subprocess providers
//! (`generic_validators.py`). HTTP providers are not native yet: the pool
//! reports them so the verb can delegate before any side effect.

use crate::budget::Context;
use crate::diagnostics::{diagnostic_suffix, redact_diagnostic};
use crate::errors::{tool_permission_denial, ProviderError};
use lope_runtime::call::{run_subprocess_group, CallError, Request};
use serde_json::Value;

pub trait Validator: Send + Sync {
    fn name(&self) -> &str;
    fn available(&self) -> bool;
    fn generate(&self, prompt: &str, timeout: f64, ctx: Option<&Context>) -> Result<String, ProviderError>;
    /// request_plan transport: argv | stdin | file | http
    fn transport(&self) -> &'static str;
    fn timeout_override(&self) -> Option<i64> {
        None
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

/// `GenericSubprocessValidator` (generate path).
pub struct Subprocess {
    pub name: String,
    pub command: Vec<String>,
    pub stdin: bool,
    pub prompt_wrapper: Option<String>,
    pub timeout_override: Option<i64>,
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
        let name = e
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("provider missing 'name' field")?
            .to_string();
        let command: Vec<String> = e
            .get("command")
            .and_then(Value::as_array)
            .filter(|a| a.iter().all(Value::is_string))
            .ok_or_else(|| format!("provider '{name}' command must be a list of strings"))?
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        Ok(Subprocess {
            name,
            command,
            stdin: crate::validity::truthy(e.get("stdin")),
            prompt_wrapper: e.get("prompt_wrapper").and_then(Value::as_str).map(str::to_string),
            timeout_override: e.get("timeout").and_then(lope_core::config::py_int),
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
    fn generate(&self, prompt: &str, timeout: f64, ctx: Option<&Context>) -> Result<String, ProviderError> {
        let wrapped;
        let prompt = match &self.prompt_wrapper {
            // Pool only admits wrappers whose sole fields are {prompt}.
            Some(w) => {
                wrapped = w.replace("{prompt}", prompt);
                wrapped.as_str()
            }
            None => prompt,
        };
        let (cmd, input) = if self.stdin {
            (self.command.clone(), Some(prompt.to_string()))
        } else {
            (self.command.iter().map(|a| a.replace("{prompt}", prompt)).collect(), None)
        };
        let eff = self.effective_timeout(timeout);
        let n = &self.name;
        match run(cmd, input, eff, &workdir(), vec![], ctx) {
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
                Err(ProviderError::Runtime(format!("{n} binary not found: {}", redact_diagnostic(&self.command[0]))))
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
