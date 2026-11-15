//! Built-in CLI validators: claude, codex, opencode, gemini-cli, aider
//! (`validators.py`), and `MakakooAdapter` (`makakoo_adapter.py`).

use crate::budget::Context;
use crate::errors::ProviderError;
use crate::generic::{num, process_diagnostics, run, run_with_group_kill, which_or, workdir, Validator};
use crate::pyre::{head, re, search};
use crate::verdict::{infra_error, parse_opencode_verdict, PhaseVerdict, ValidatorResult, VerdictStatus};
use lope_core::pyjson;
use lope_runtime::call::CallError;
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;

const PAPER_REVIEW_PREFIX: &str = "You are a senior AI researcher reviewing a research paper draft. Give honest, specific, actionable feedback. Return a VERDICT block.\n\n";

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

    fn run_aider_with_prompt_file(
        &self,
        prompt: &str,
        timeout: f64,
        ctx: Option<&Context>,
    ) -> Result<crate::generic::Run, ProviderError> {
        use std::os::unix::fs::OpenOptionsExt;
        let path = if let Some(c) = ctx {
            if let (Some(reg), Some(call_id)) = (&c.registry, &c.call_id) {
                let base = reg
                    .run_work_dir(c.run_id())
                    .unwrap_or_else(|_| std::env::temp_dir())
                    .join(call_id);
                let _ = std::fs::create_dir_all(&base);
                let hex = &uuid::Uuid::new_v4().simple().to_string()[..16];
                let p = base.join(format!("prompt-{hex}.txt"));
                let mut owned = c.owned_paths.lock().unwrap();
                let ps = p.to_string_lossy().into_owned();
                if !owned.contains(&ps) {
                    owned.push(ps);
                }
                let mut ch = serde_json::Map::new();
                ch.insert("owned_paths".into(), serde_json::json!(*owned));
                let _ = reg.update_call(c.run_id(), call_id, ch);
                p
            } else {
                let hex = &uuid::Uuid::new_v4().simple().to_string()[..16];
                std::env::temp_dir().join(format!("lope-prompt-{hex}.txt"))
            }
        } else {
            let hex = &uuid::Uuid::new_v4().simple().to_string()[..16];
            std::env::temp_dir().join(format!("lope-prompt-{hex}.txt"))
        };
        struct RmGuard(PathBuf);
        impl Drop for RmGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| ProviderError::Runtime(format!("aider failed: {e}")))?;
        let _guard = RmGuard(path.clone());
        f.write_all(prompt.as_bytes()).and_then(|_| f.flush()).and_then(|_| f.sync_all()).map_err(|e| {
            ProviderError::Runtime(format!("aider failed: {e}"))
        })?;
        drop(f);
        let argv = vec![
            self.binary.clone(),
            "--message-file".into(),
            path.to_string_lossy().into_owned(),
            "--no-git".into(),
            "--no-auto-commits".into(),
            "--yes".into(),
        ];
        run_with_group_kill(argv, None, timeout, &workdir(), ctx)
    }
}

pub fn opencode_text(stdout: &str) -> String {
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

pub fn opencode_session_id_from_stdout(stdout: &str) -> Option<String> {
    for line in crate::pyre::splitlines(stdout) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut candidates = vec![line];
        if let Some(brace) = line.find('{') {
            if brace > 0 {
                candidates.push(&line[brace..]);
                if let Some(end_brace) = line.rfind('}') {
                    if end_brace > brace {
                        candidates.push(&line[brace..=end_brace]);
                    }
                }
            }
        }
        for cand in candidates {
            let Ok(event) = pyjson::loads(cand) else { continue };
            if let Some(sid) = event
                .get("sessionID")
                .or_else(|| event.get("session_id"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                return Some(sid.to_string());
            }
            if let Some(payload) = event.get("payload").and_then(Value::as_object) {
                if let Some(sid) = payload
                    .get("sessionID")
                    .or_else(|| payload.get("session_id"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    return Some(sid.to_string());
                }
            }
        }
    }
    None
}

fn extract_text_from_opencode_export(
    binary: &str,
    stdout: &str,
    timeout: f64,
    cwd: &str,
    ctx: Option<&Context>,
) -> String {
    let Some(sid) = opencode_session_id_from_stdout(stdout) else { return String::new() };
    if timeout <= 0.0 {
        return String::new();
    }
    let cmd = vec![binary.to_string(), "export".into(), sid];
    let Ok(p) = run_with_group_kill(cmd, None, timeout.min(60.0), cwd, ctx) else {
        return String::new();
    };
    if p.returncode != 0 {
        return String::new();
    }
    let Ok(data) = pyjson::loads(&p.stdout) else { return String::new() };
    let Some(messages) = data.get("messages").and_then(Value::as_array) else {
        return String::new();
    };
    for msg in messages.iter().rev() {
        let role = msg.get("info").and_then(|i| i.get("role")).and_then(Value::as_str);
        if role != Some("assistant") {
            continue;
        }
        let mut chunks = Vec::new();
        if let Some(parts) = msg.get("parts").and_then(Value::as_array) {
            for part in parts {
                if part.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        chunks.push(t.to_string());
                    }
                }
            }
        }
        if !chunks.is_empty() {
            return chunks.join("");
        }
    }
    String::new()
}

pub fn diagnose_empty_opencode_stream(stdout: &str) -> String {
    let mut finish_reason: Option<String> = None;
    let mut rejected_tools: Vec<String> = Vec::new();
    let mut saw_any_event = false;
    let mut event_types: Vec<String> = Vec::new();
    let mut first_non_empty_line: Option<String> = None;

    for line in crate::pyre::splitlines(stdout) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if first_non_empty_line.is_none() {
            first_non_empty_line = Some(head(line, 160).to_string());
        }
        let Ok(event) = pyjson::loads(line) else { continue };
        saw_any_event = true;
        let ev_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        if !ev_type.is_empty() && !event_types.iter().any(|e| e == ev_type) {
            event_types.push(ev_type.to_string());
        }
        let part = if ev_type == "message.part.updated" {
            event.get("properties").and_then(|p| p.get("part"))
        } else if matches!(ev_type, "step_finish" | "step-finish" | "tool_use" | "tool") {
            event.get("part").or(Some(&event))
        } else {
            None
        };
        let part_type = part.and_then(|p| p.get("type")).and_then(Value::as_str).unwrap_or(ev_type);
        if matches!(part_type, "step_finish" | "step-finish") {
            if let Some(r) = part.and_then(|p| p.get("reason")).and_then(Value::as_str) {
                finish_reason = Some(r.to_string());
            }
        } else if matches!(part_type, "tool_use" | "tool") {
            if let Some(state) = part.and_then(|p| p.get("state")) {
                if state.get("status").and_then(Value::as_str) == Some("error") {
                    let tool = part.and_then(|p| p.get("tool")).and_then(Value::as_str).unwrap_or("unknown");
                    let err = state.get("error").and_then(Value::as_str).unwrap_or("");
                    rejected_tools.push(format!("{tool}: {}", head(err, 120)));
                }
            }
        }
    }
    if !saw_any_event {
        let raw = first_non_empty_line.map(|f| format!("; first raw line: {f}")).unwrap_or_default();
        return format!("stdout was empty (no JSON events parsed{raw})");
    }
    if !rejected_tools.is_empty() {
        return format!(
            "model attempted tool-use that was rejected — {} call(s); first: {}. Add 'DO NOT USE TOOLS' to the prompt.",
            rejected_tools.len(),
            rejected_tools[0]
        );
    }
    if finish_reason.as_deref() == Some("tool-calls") {
        return "session ended via `reason: tool-calls` (model wanted a tool that was unavailable / blocked). Add 'DO NOT USE TOOLS' to the prompt.".into();
    }
    if finish_reason.as_deref() == Some("error") {
        return "session ended via `reason: error` (opencode itself errored)".into();
    }
    let types_repr = format!(
        "[{}]",
        event_types.iter().map(|s| format!("'{s}'")).collect::<Vec<_>>().join(", ")
    );
    if let Some(fr) = finish_reason {
        return format!("session ended via `reason: {fr}` with no text events (types seen: {types_repr})");
    }
    format!(
        "stream had events but no text events and no step_finish (types seen: {types_repr}; first line: {})",
        first_non_empty_line.unwrap_or_else(|| "None".into())
    )
}

impl Validator for Builtin {
    fn name(&self) -> &str {
        self.kind
    }
    fn available(&self) -> bool {
        lope_core::paths::which(&self.binary).is_some()
    }
    fn command_template(&self) -> Vec<String> {
        vec![self.binary.clone()]
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
                if !self.available() {
                    return Err(ProviderError::Runtime(format!("aider binary not found: {b}")));
                }
                let r = self
                    .run_aider_with_prompt_file(prompt, timeout, ctx)
                    .map_err(|e| ProviderError::Runtime(format!("aider failed: {e}")))?;
                if r.returncode != 0 {
                    return Err(ProviderError::Runtime(format!(
                        "aider exited {}: {}",
                        r.returncode,
                        head(&r.stderr, 500)
                    )));
                }
                if r.stdout.is_empty() {
                    return Err(ProviderError::Runtime("aider returned empty output".into()));
                }
                Ok(r.stdout)
            }
            _ => {
                if !self.available() {
                    return Err(ProviderError::Runtime(format!("opencode binary not found at {b}")));
                }
                let deadline = crate::budget::monotonic() + timeout;
                let cwd = workdir();
                let mut extra = env_args("LOPE_OPENCODE_ARGS");
                if extra.is_empty() {
                    extra = vec!["--pure".into(), "--model".into(), "myprovider/ail-compound".into()];
                }
                let mut argv = vec![b.clone(), "run".into()];
                argv.extend(extra);
                argv.extend(["--format".into(), "json".into(), prompt.into()]);
                let r = run_with_group_kill(argv, None, timeout, &cwd, ctx)?;
                if r.returncode != 0 {
                    return Err(ProviderError::Runtime(format!(
                        "opencode exited {}: {}",
                        r.returncode,
                        head(&r.stderr, 500)
                    )));
                }
                let mut text = opencode_text(&r.stdout);
                if text.is_empty() {
                    let rem = deadline - crate::budget::monotonic();
                    if rem > 0.0 {
                        text = extract_text_from_opencode_export(&b, &r.stdout, rem, &cwd, ctx);
                    }
                }
                if text.is_empty() {
                    let diag = diagnose_empty_opencode_stream(&r.stdout);
                    return Err(ProviderError::Runtime(format!("opencode returned no text events — {diag}")));
                }
                Ok(text)
            }
        }
    }

    fn validate(&self, prompt: &str, timeout: f64, ctx: Option<&Context>) -> ValidatorResult {
        let b = self.binary.clone();
        let t0 = lope_core::time::now();
        let full = if self.kind == "opencode" {
            prompt.to_string()
        } else {
            format!("{PAPER_REVIEW_PREFIX}{prompt}")
        };
        match self.kind {
            "claude" => {
                if !self.available() {
                    return infra_error("claude", &format!("claude binary not found: {b}"), 0.0);
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
                let r = match run_with_group_kill(argv, Some(full), timeout, &workdir(), ctx) {
                    Ok(r) => r,
                    Err(e) => {
                        return infra_error("claude", &format!("claude failed: {e}"), (lope_core::time::now() - t0).max(0.0))
                    }
                };
                let d = (lope_core::time::now() - t0).max(0.0);
                if r.returncode != 0 {
                    return infra_error(
                        "claude",
                        &format!("claude exited with code {}; stderr: {}", r.returncode, head(&r.stderr, 500)),
                        d,
                    );
                }
                if r.stdout.is_empty() {
                    return infra_error("claude", "claude returned empty output", d);
                }
                let v = parse_opencode_verdict(&r.stdout, "claude", d);
                ValidatorResult {
                    validator_name: "claude".into(),
                    verdict: v,
                    raw_response: r.stdout,
                    error: String::new(),
                    flag_error_hint: String::new(),
                    repair_attempted: false,
                    repair_status: None,
                    repaired_response: String::new(),
                    initial_parse_status: None,
                    parse_error_category: None,
                }
            }
            "codex" => {
                if !self.available() {
                    return infra_error("codex", &format!("codex binary not found: {b}"), 0.0);
                }
                let model = std::env::var("LOPE_CODEX_MODEL").unwrap_or_else(|_| "gpt-5.6-sol".into());
                let argv: Vec<String> = [
                    b.as_str(),
                    "exec",
                    "--ignore-user-config",
                    "--skip-git-repo-check",
                    "-s",
                    "read-only",
                    "-m",
                    &model,
                    "-c",
                    "model_reasoning_effort=\"low\"",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect();
                let r = match run_with_group_kill(argv, Some(full), timeout, &workdir(), ctx) {
                    Ok(r) => r,
                    Err(e) => {
                        return infra_error("codex", &format!("codex failed: {e}"), (lope_core::time::now() - t0).max(0.0))
                    }
                };
                let d = (lope_core::time::now() - t0).max(0.0);
                if r.returncode != 0 {
                    return infra_error(
                        "codex",
                        &format!("codex exited with code {}; stderr: {}", r.returncode, head(&r.stderr, 500)),
                        d,
                    );
                }
                if r.stdout.is_empty() {
                    return infra_error("codex", "codex returned empty output", d);
                }
                let v = parse_opencode_verdict(&r.stdout, "codex", d);
                ValidatorResult {
                    validator_name: "codex".into(),
                    verdict: v,
                    raw_response: r.stdout,
                    error: String::new(),
                    flag_error_hint: String::new(),
                    repair_attempted: false,
                    repair_status: None,
                    repaired_response: String::new(),
                    initial_parse_status: None,
                    parse_error_category: None,
                }
            }
            "gemini-cli" => {
                let argv = vec![b, "--prompt".into(), full, "--output-format".into(), "json".into()];
                let r = match run_with_group_kill(argv, None, timeout, &workdir(), ctx) {
                    Ok(r) => r,
                    Err(e) => {
                        return infra_error(
                            "gemini-cli",
                            &format!("gemini-cli failed: {e}"),
                            (lope_core::time::now() - t0).max(0.0),
                        )
                    }
                };
                let d = (lope_core::time::now() - t0).max(0.0);
                if r.returncode != 0 {
                    return infra_error(
                        "gemini-cli",
                        &format!("gemini-cli exited with code {}; stderr: {}", r.returncode, head(&r.stderr, 300)),
                        d,
                    );
                }
                let Ok(v) = lope_core::pyjson::loads(&r.stdout) else {
                    return infra_error(
                        "gemini-cli",
                        &format!("gemini-cli returned non-JSON output; stdout head: {}", head(&r.stdout, 300)),
                        d,
                    );
                };
                let text = v.get("response").and_then(Value::as_str).unwrap_or("").to_string();
                if text.is_empty() {
                    return infra_error("gemini-cli", "gemini-cli returned empty response", d);
                }
                let verd = parse_opencode_verdict(&text, "gemini-cli", d);
                ValidatorResult {
                    validator_name: "gemini-cli".into(),
                    verdict: verd,
                    raw_response: text,
                    error: String::new(),
                    flag_error_hint: String::new(),
                    repair_attempted: false,
                    repair_status: None,
                    repaired_response: String::new(),
                    initial_parse_status: None,
                    parse_error_category: None,
                }
            }
            "aider" => {
                if !self.available() {
                    return infra_error("aider", &format!("aider binary not found: {b}"), 0.0);
                }
                let r = match self.run_aider_with_prompt_file(&full, timeout, ctx) {
                    Ok(r) => r,
                    Err(e) => {
                        return infra_error("aider", &format!("aider failed: {e}"), (lope_core::time::now() - t0).max(0.0))
                    }
                };
                let d = (lope_core::time::now() - t0).max(0.0);
                if r.returncode != 0 {
                    return infra_error(
                        "aider",
                        &format!("aider exited with code {}; stderr: {}", r.returncode, head(&r.stderr, 500)),
                        d,
                    );
                }
                if r.stdout.is_empty() {
                    return infra_error("aider", "aider returned empty output", d);
                }
                let v = parse_opencode_verdict(&r.stdout, "aider", d);
                ValidatorResult {
                    validator_name: "aider".into(),
                    verdict: v,
                    raw_response: r.stdout,
                    error: String::new(),
                    flag_error_hint: String::new(),
                    repair_attempted: false,
                    repair_status: None,
                    repaired_response: String::new(),
                    initial_parse_status: None,
                    parse_error_category: None,
                }
            }
            _ => {
                if !self.available() {
                    return infra_error("opencode", &format!("opencode binary not found at {b}"), 0.0);
                }
                let deadline = crate::budget::monotonic() + timeout;
                let cwd = workdir();
                let mut extra = env_args("LOPE_OPENCODE_ARGS");
                if extra.is_empty() {
                    extra = vec!["--pure".into(), "--model".into(), "myprovider/ail-compound".into()];
                }
                let mut argv = vec![b.clone(), "run".into()];
                argv.extend(extra);
                argv.extend(["--format".into(), "json".into(), full]);
                let r = match run_with_group_kill(argv, None, timeout, &cwd, ctx) {
                    Ok(r) => r,
                    Err(e) => return infra_error("opencode", &e.to_string(), (lope_core::time::now() - t0).max(0.0)),
                };
                let d = (lope_core::time::now() - t0).max(0.0);
                if r.returncode != 0 {
                    return infra_error(
                        "opencode",
                        &format!(
                            "opencode exited with code {}; stderr head: {}",
                            r.returncode,
                            head(&r.stderr, 500)
                        ),
                        d,
                    );
                }
                let mut text = opencode_text(&r.stdout);
                if text.is_empty() {
                    let rem = deadline - crate::budget::monotonic();
                    if rem > 0.0 {
                        text = extract_text_from_opencode_export(&b, &r.stdout, rem, &cwd, ctx);
                    }
                }
                if text.is_empty() {
                    let diag = diagnose_empty_opencode_stream(&r.stdout);
                    return infra_error("opencode", &format!("opencode returned no text events — {diag}"), d);
                }
                let v = parse_opencode_verdict(&text, "opencode", d);
                ValidatorResult {
                    validator_name: "opencode".into(),
                    verdict: v,
                    raw_response: text,
                    error: String::new(),
                    flag_error_hint: String::new(),
                    repair_attempted: false,
                    repair_status: None,
                    repaired_response: String::new(),
                    initial_parse_status: None,
                    parse_error_category: None,
                }
            }
        }
    }
}

// ─── MakakooAdapterValidator ──────────────────────────────────────────────────

pub fn resolve_makakoo_bin() -> Option<String> {
    if let Ok(exp) = std::env::var("MAKAKOO_BIN") {
        if !exp.is_empty() && std::path::Path::new(&exp).is_file() {
            return Some(exp);
        }
    }
    lope_core::paths::which("makakoo").map(|p| p.display().to_string())
}

pub fn adapters_root() -> PathBuf {
    if let Ok(env) = std::env::var("MAKAKOO_ADAPTERS_HOME") {
        if !env.is_empty() {
            return PathBuf::from(env);
        }
    }
    lope_core::paths::home_dir().join(".makakoo").join("adapters")
}

pub fn enumerate_registered_adapters() -> Vec<String> {
    let enabled = std::env::var("LOPE_MAKAKOO_ADAPTERS").as_deref() != Ok("0");
    enumerate_registered_adapters_in(None, enabled)
}

pub fn enumerate_registered_adapters_in(
    adapters_dir: Option<&std::path::Path>,
    enabled: bool,
) -> Vec<String> {
    if !enabled {
        return Vec::new();
    }
    let root = adapters_dir
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(adapters_root)
        .join("registered");
    let Ok(rd) = std::fs::read_dir(&root) else { return Vec::new() };
    let mut names = Vec::new();
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("toml") {
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    names
}

pub struct MakakooAdapter {
    pub adapter_name: String,
    pub bin: Option<String>,
}

impl MakakooAdapter {
    pub fn new(adapter_name: &str) -> Self {
        MakakooAdapter {
            adapter_name: adapter_name.to_string(),
            bin: resolve_makakoo_bin(),
        }
    }
}

pub fn hydrate_makakoo_result(stdout: &str, fallback_name: &str, fallback_duration: f64) -> ValidatorResult {
    let payload = match pyjson::loads(stdout) {
        Ok(Value::Object(m)) => m,
        Ok(_) => {
            return ValidatorResult {
                validator_name: fallback_name.into(),
                verdict: PhaseVerdict {
                    status: VerdictStatus::InfraError,
                    confidence: 0.0,
                    rationale: "invalid JSON from makakoo adapter call: expected object".into(),
                    required_fixes: vec![],
                    nice_to_have: vec![],
                    duration_seconds: fallback_duration,
                    validator_name: fallback_name.into(),
                    stage: None,
                    evidence_gate_triggered: false,
                },
                raw_response: head(stdout, 2000).into(),
                error: "invalid JSON: expected object".into(),
                flag_error_hint: String::new(),
                repair_attempted: false,
                repair_status: None,
                repaired_response: String::new(),
                initial_parse_status: None,
                parse_error_category: None,
            };
        }
        Err(e) => {
            return ValidatorResult {
                validator_name: fallback_name.into(),
                verdict: PhaseVerdict {
                    status: VerdictStatus::InfraError,
                    confidence: 0.0,
                    rationale: format!("invalid JSON from makakoo adapter call: {e}"),
                    required_fixes: vec![],
                    nice_to_have: vec![],
                    duration_seconds: fallback_duration,
                    validator_name: fallback_name.into(),
                    stage: None,
                    evidence_gate_triggered: false,
                },
                raw_response: head(stdout, 2000).into(),
                error: format!("invalid JSON: {e}"),
                flag_error_hint: String::new(),
                repair_attempted: false,
                repair_status: None,
                repaired_response: String::new(),
                initial_parse_status: None,
                parse_error_category: None,
            };
        }
    };
    let vdict = payload.get("verdict").and_then(Value::as_object);
    let status_raw = vdict.and_then(|v| v.get("status")).and_then(Value::as_str).unwrap_or("");
    let status = match status_raw.trim().to_ascii_uppercase().as_str() {
        "PASS" => VerdictStatus::Pass,
        "NEEDS_FIX" => VerdictStatus::NeedsFix,
        "FAIL" => VerdictStatus::Fail,
        "INCONCLUSIVE" => VerdictStatus::Inconclusive,
        _ => VerdictStatus::InfraError,
    };
    let confidence = vdict
        .and_then(|v| v.get("confidence"))
        .and_then(lope_core::config::py_float)
        .unwrap_or(0.5);
    let rationale = vdict
        .and_then(|v| v.get("rationale"))
        .map(lope_core::config::py_str)
        .unwrap_or_default();
    let required_fixes = vdict
        .and_then(|v| v.get("required_fixes"))
        .and_then(Value::as_array)
        .map(|a| a.iter().map(lope_core::config::py_str).collect())
        .unwrap_or_default();
    let nice_to_have = vdict
        .and_then(|v| v.get("nice_to_have"))
        .and_then(Value::as_array)
        .map(|a| a.iter().map(lope_core::config::py_str).collect())
        .unwrap_or_default();
    let duration_seconds = vdict
        .and_then(|v| v.get("duration_seconds"))
        .and_then(lope_core::config::py_float)
        .unwrap_or(fallback_duration);
    let validator_name = vdict
        .and_then(|v| v.get("validator_name"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_name)
        .to_string();
    let stage = vdict.and_then(|v| v.get("stage")).and_then(Value::as_str).map(String::from);
    let evidence_gate_triggered = vdict
        .and_then(|v| v.get("evidence_gate_triggered"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let top_validator_name = payload
        .get("validator_name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_name)
        .to_string();
    let raw_response = payload
        .get("raw_response")
        .map(|v| head(&lope_core::config::py_str(v), 20000).to_string())
        .unwrap_or_default();
    let error = payload.get("error").map(lope_core::config::py_str).unwrap_or_default();
    let flag_error_hint = payload.get("flag_error_hint").map(lope_core::config::py_str).unwrap_or_default();

    ValidatorResult {
        validator_name: top_validator_name,
        verdict: PhaseVerdict {
            status,
            confidence,
            rationale,
            required_fixes,
            nice_to_have,
            duration_seconds,
            validator_name,
            stage,
            evidence_gate_triggered,
        },
        raw_response,
        error,
        flag_error_hint,
        repair_attempted: false,
        repair_status: None,
        repaired_response: String::new(),
        initial_parse_status: None,
        parse_error_category: None,
    }
}

impl Validator for MakakooAdapter {
    fn name(&self) -> &str {
        &self.adapter_name
    }
    fn available(&self) -> bool {
        if std::env::var("LOPE_MAKAKOO_ADAPTERS").as_deref() == Ok("0") {
            return false;
        }
        self.bin.is_some()
    }
    fn transport(&self) -> &'static str {
        "stdin"
    }

    fn validate(&self, prompt: &str, timeout: f64, ctx: Option<&Context>) -> ValidatorResult {
        let t0 = lope_core::time::now();
        let Some(ref bin) = self.bin else {
            return infra_error(&self.adapter_name, "makakoo binary not on PATH", (lope_core::time::now() - t0).max(0.0));
        };
        let cmd = vec![
            bin.clone(),
            "adapter".into(),
            "call".into(),
            self.adapter_name.clone(),
            "--timeout".into(),
            num(timeout),
        ];
        let res = run(cmd, Some(prompt.to_string()), timeout, &workdir(), vec![], ctx);
        let d = (lope_core::time::now() - t0).max(0.0);
        match res {
            Ok(p) => {
                if p.returncode != 0 {
                    let msg = head(p.stderr.trim(), 500);
                    return infra_error(
                        &self.adapter_name,
                        &format!("makakoo adapter call exit {}: {msg}", p.returncode),
                        d,
                    );
                }
                hydrate_makakoo_result(&p.stdout, &self.adapter_name, d)
            }
            Err(CallError::Timeout { .. }) => {
                infra_error(&self.adapter_name, &format!("makakoo adapter call timeout after {}s", num(timeout)), d)
            }
            Err(CallError::FileNotFound(_)) => infra_error(&self.adapter_name, "makakoo binary vanished mid-call", d),
            Err(e) => infra_error(&self.adapter_name, &format!("subprocess error: {e}"), d),
        }
    }

    fn generate(&self, prompt: &str, timeout: f64, ctx: Option<&Context>) -> Result<String, ProviderError> {
        if self.bin.is_none() {
            return Err(ProviderError::NotImplemented(
                "MakakooAdapterValidator.generate requires the makakoo binary on PATH; install Makakoo OS or set MAKAKOO_BIN"
                    .into(),
            ));
        }
        let result = self.validate(prompt, timeout, ctx);
        if !result.error.is_empty() && result.raw_response.is_empty() {
            return Err(ProviderError::NotImplemented(format!(
                "MakakooAdapter '{}' did not return plain-text generation output: {}",
                self.adapter_name,
                head(&result.error, 200)
            )));
        }
        Ok(result.raw_response)
    }
}
