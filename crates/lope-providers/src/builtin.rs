//! Built-in CLI validators (generate path): claude, codex, opencode,
//! gemini-cli, aider (`validators.py`). Messages are byte-identical.

use crate::budget::Context;
use crate::errors::ProviderError;
use crate::generic::{process_diagnostics, run_with_group_kill, which_or, workdir, Validator};
use crate::pyre::{head, re, search};
use serde_json::Value;

/// POSIX `shlex.split` (enough for LOPE_*_ARGS).
pub fn shlex_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut have = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                have = true;
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    cur.push(d);
                }
            }
            '"' => {
                have = true;
                while let Some(d) = chars.next() {
                    match d {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`' | '\n')) => {
                            cur.push(chars.next().unwrap())
                        }
                        _ => cur.push(d),
                    }
                }
            }
            '\\' => {
                have = true;
                if let Some(d) = chars.next() {
                    cur.push(d);
                }
            }
            c if c.is_whitespace() => {
                if have {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            c => {
                have = true;
                cur.push(c);
            }
        }
    }
    if have {
        out.push(cur);
    }
    out
}

fn env_args(k: &str) -> Vec<String> {
    let v = std::env::var(k).unwrap_or_default();
    if v.trim().is_empty() {
        vec![]
    } else {
        shlex_split(v.trim())
    }
}

fn infra_exit(provider: &str, stdout: &str, stderr: &str) -> bool {
    let (o, e) = (stdout.trim(), stderr.trim());
    match provider {
        "claude" => [
            r"(?m)^API Error: (?:429|503)\b",
            r"(?m)^API Error: 400 Tool reference '[^']+' not found in available tools$",
        ]
        .iter()
        .any(|p| search(&re(p), o).is_some()),
        "codex" => [
            r"(?mi)^Error: failed to initialize in-process app-server client: .+$",
            r"(?mi)^Error: (?:connection reset|service unavailable).*$",
        ]
        .iter()
        .any(|p| search(&re(p), e).is_some()),
        _ => false,
    }
}

pub struct Builtin {
    pub kind: &'static str,
    pub binary: String,
}

impl Builtin {
    pub fn new(kind: &'static str) -> Self {
        let binary = match kind {
            "opencode" => std::env::var("OPENCODE_BIN").unwrap_or_else(|_| "opencode".into()),
            "gemini-cli" => which_or("gemini"),
            k => which_or(k),
        };
        Builtin { kind, binary }
    }

    fn run_print(
        &self,
        argv: Vec<String>,
        prompt: &str,
        timeout: f64,
        ctx: Option<&Context>,
    ) -> Result<String, ProviderError> {
        let k = self.kind;
        let r = run_with_group_kill(argv, Some(prompt.to_string()), timeout, &workdir(), ctx).map_err(|e| match e {
            ProviderError::Infra(m) => ProviderError::Infra(format!("{k} failed: {m}")),
            other => other,
        })?;
        if r.returncode != 0 {
            let msg = format!("{k} exited {}: {}", r.returncode, process_diagnostics(&r.stdout, &r.stderr));
            return Err(if infra_exit(k, &r.stdout, &r.stderr) {
                ProviderError::Infra(msg)
            } else {
                ProviderError::Runtime(msg)
            });
        }
        if r.stdout.is_empty() {
            return Err(ProviderError::Infra(format!("{k} returned empty output")));
        }
        Ok(r.stdout)
    }
}

fn opencode_text(stdout: &str) -> String {
    let mut parts = String::new();
    for line in crate::pyre::splitlines(stdout) {
        let Ok(ev) = lope_core::pyjson::loads(line.trim()) else { continue };
        match ev.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = ev.get("part").and_then(|p| p.get("text")).and_then(Value::as_str) {
                    parts.push_str(t);
                }
            }
            Some("message.part.delta") => {
                let p = ev.get("properties");
                if p.and_then(|p| p.get("field")).and_then(Value::as_str) == Some("text") {
                    if let Some(d) = p.and_then(|p| p.get("delta")).and_then(Value::as_str) {
                        parts.push_str(d);
                    }
                }
            }
            _ => {}
        }
    }
    parts
}

impl Validator for Builtin {
    fn name(&self) -> &str {
        self.kind
    }
    fn available(&self) -> bool {
        lope_core::paths::which(&self.binary).is_some()
    }
    fn transport(&self) -> &'static str {
        match self.kind {
            "opencode" | "gemini-cli" => "argv",
            "aider" => "file",
            _ => "stdin",
        }
    }
    fn generate(&self, prompt: &str, timeout: f64, ctx: Option<&Context>) -> Result<String, ProviderError> {
        let b = self.binary.clone();
        match self.kind {
            "claude" => {
                if !self.available() {
                    return Err(ProviderError::Runtime(format!("claude binary not found: {b}")));
                }
                let mut argv = vec![
                    b,
                    "--print".into(),
                    "--safe-mode".into(),
                    "--no-session-persistence".into(),
                    "--output-format".into(),
                    "text".into(),
                ];
                argv.extend(env_args("LOPE_CLAUDE_ARGS"));
                self.run_print(argv, prompt, timeout, ctx)
            }
            "codex" => {
                if !self.available() {
                    return Err(ProviderError::Runtime(format!("codex binary not found: {b}")));
                }
                let model = std::env::var("LOPE_CODEX_MODEL").unwrap_or_else(|_| "gpt-5.6-sol".into());
                let mode = if ctx.is_some_and(|c| c.implementation) { "workspace-write" } else { "read-only" };
                let argv: Vec<String> = [
                    b.as_str(),
                    "exec",
                    "--ignore-user-config",
                    "--skip-git-repo-check",
                    "-s",
                    mode,
                    "-m",
                    &model,
                    "-c",
                    "model_reasoning_effort=\"low\"",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect();
                self.run_print(argv, prompt, timeout, ctx)
            }
            "gemini-cli" => {
                if !self.available() {
                    return Err(ProviderError::Runtime(format!("gemini binary not found: {b}")));
                }
                let argv = vec![b, "--prompt".into(), prompt.into(), "--output-format".into(), "json".into()];
                let r = run_with_group_kill(argv, None, timeout, &workdir(), ctx)
                    .map_err(|e| ProviderError::Runtime(format!("gemini-cli failed: {e}")))?;
                if r.returncode != 0 {
                    return Err(ProviderError::Runtime(format!(
                        "gemini-cli exited {}: {}",
                        r.returncode,
                        head(&r.stderr, 500)
                    )));
                }
                let v = lope_core::pyjson::loads(&r.stdout).map_err(|_| {
                    ProviderError::Runtime(format!(
                        "gemini-cli returned non-JSON output; stdout head: {}",
                        head(&r.stdout, 300)
                    ))
                })?;
                let text = v.get("response").and_then(Value::as_str).unwrap_or("").to_string();
                if text.is_empty() {
                    return Err(ProviderError::Runtime("gemini-cli returned empty response".into()));
                }
                Ok(text)
            }
            "aider" => {
                // Prompt files (FR14) arrive with the review sprint; until then
                // the pool reports aider as not native.
                Err(ProviderError::NotImplemented("aider".into()))
            }
            _ => {
                if !self.available() {
                    return Err(ProviderError::Runtime(format!("opencode binary not found at {b}")));
                }
                let mut extra = env_args("LOPE_OPENCODE_ARGS");
                if extra.is_empty() {
                    extra = vec!["--pure".into(), "--model".into(), "myprovider/ail-compound".into()];
                }
                let mut argv = vec![b, "run".into()];
                argv.extend(extra);
                argv.extend(["--format".into(), "json".into(), prompt.into()]);
                let r = run_with_group_kill(argv, None, timeout, &workdir(), ctx)?;
                if r.returncode != 0 {
                    return Err(ProviderError::Runtime(format!(
                        "opencode exited {}: {}",
                        r.returncode,
                        head(&r.stderr, 500)
                    )));
                }
                let text = opencode_text(&r.stdout);
                if text.is_empty() {
                    // The `opencode export` fallback and stream diagnosis are
                    // not ported; keep the message shape.
                    return Err(ProviderError::Runtime(
                        "opencode returned no text events — stdout was empty (no JSON events parsed)".into(),
                    ));
                }
                Ok(text)
            }
        }
    }
}
