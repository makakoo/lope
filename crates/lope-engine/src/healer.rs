//! SelfHealer — lope's adapter resilience primitive (`healer.py`, Sprint S7).

use crate::journal::append_event;
use lope_core::config::{default_path, load, save, LearnedAdapter, LopeCfg};
use lope_providers::budget::Context;
use lope_providers::generic::Validator;
use lope_providers::invocation::invoke_generate;
use regex::Regex;
use serde_json::json;
use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use lope_providers::verdict::is_flag_error;

pub const SMOKE_PROMPT: &str = "Reply with the single word OK and nothing else.";
pub const SMOKE_EXPECTED: &str = "OK";
pub const SMOKE_TIMEOUT_SECONDS: u64 = 60;
pub const HELP_TIMEOUT_SECONDS: u64 = 10;
pub const REVIEW_TIMEOUT_SECONDS: f64 = 120.0;
pub const SELF_HEAL_BUDGET_SECONDS: f64 =
    HELP_TIMEOUT_SECONDS as f64 + REVIEW_TIMEOUT_SECONDS + SMOKE_TIMEOUT_SECONDS as f64;
pub const LEARNED_ADAPTER_TTL_SECONDS: f64 = 90.0 * 24.0 * 3600.0;

#[derive(Debug, Default)]
pub struct SelfHealer {
    healed_this_session: HashSet<String>,
}

impl SelfHealer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn should_attempt(
        &self,
        cli_name: &str,
        reviewer_available: bool,
        remaining_seconds: Option<f64>,
    ) -> bool {
        if cli_name.is_empty() {
            return false;
        }
        let env = std::env::var("LOPE_SELF_HEAL")
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if !matches!(env.as_str(), "1" | "true" | "yes" | "on") {
            return false;
        }
        if self.healed_this_session.contains(cli_name) {
            return false;
        }
        if !reviewer_available {
            return false;
        }
        if let Some(rem) = remaining_seconds {
            if rem < SELF_HEAL_BUDGET_SECONDS {
                return false;
            }
        }
        true
    }

    pub fn mark_attempted(&mut self, cli_name: &str) {
        self.healed_this_session.insert(cli_name.to_string());
    }

    pub fn attempt(
        &mut self,
        cli_name: &str,
        cli_binary: &str,
        old_argv: &[String],
        stderr: &str,
        reviewer: &dyn Validator,
        ctx: Option<&Context>,
    ) -> Option<LearnedAdapter> {
        self.mark_attempted(cli_name);
        let started = now_epoch();

        let _ = append_event(
            "heal_attempt",
            vec![
                ("cli", json!(cli_name)),
                ("old_argv", json!(old_argv)),
                ("stderr_head", json!(stderr.chars().take(500).collect::<String>())),
                ("reviewer", json!(reviewer.name())),
            ],
        );

        let Some(help_text) = capture_help(cli_binary) else {
            let _ = append_event(
                "heal_failure",
                vec![
                    ("cli", json!(cli_name)),
                    ("reason", json!("help_capture_failed")),
                ],
            );
            return None;
        };

        let prompt = build_heal_prompt(cli_name, old_argv, stderr, &help_text);
        let response = match invoke_generate(
            reviewer,
            &prompt,
            REVIEW_TIMEOUT_SECONDS,
            ctx,
            Some("self_heal_reviewer"),
        ) {
            Ok(r) => r,
            Err(_) => {
                let _ = append_event(
                    "heal_failure",
                    vec![
                        ("cli", json!(cli_name)),
                        ("reason", json!("reviewer_no_proposal")),
                    ],
                );
                return None;
            }
        };

        let Some(mut proposal) = parse_heal_response(&response) else {
            let _ = append_event(
                "heal_failure",
                vec![
                    ("cli", json!(cli_name)),
                    ("reason", json!("reviewer_no_proposal")),
                ],
            );
            return None;
        };

        let (ok, smoke_output) = smoke_test(cli_binary, &proposal);
        if !ok {
            let _ = append_event(
                "heal_failure",
                vec![
                    ("cli", json!(cli_name)),
                    ("reason", json!("smoke_test_failed")),
                    ("proposed_argv", json!(proposal.argv_template)),
                    (
                        "smoke_output_head",
                        json!(smoke_output.chars().take(300).collect::<String>()),
                    ),
                ],
            );
            return None;
        }

        proposal.timestamp = now_epoch();
        proposal.source_cli = json!(reviewer.name());
        persist_adapter(cli_name, proposal.clone());

        let duration = now_epoch() - started;
        let _ = append_event(
            "heal_success",
            vec![
                ("cli", json!(cli_name)),
                ("new_argv", json!(proposal.argv_template)),
                ("confidence", json!(proposal.confidence)),
                ("duration_seconds", json!(duration)),
            ],
        );
        Some(proposal)
    }
}

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn capture_help(cli_binary: &str) -> Option<String> {
    let out = Command::new(cli_binary)
        .arg("--help")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !stdout.is_empty() {
        Some(stdout)
    } else if !stderr.is_empty() {
        Some(stderr)
    } else {
        None
    }
}

fn smoke_test(cli_binary: &str, proposal: &LearnedAdapter) -> (bool, String) {
    let argv: Vec<String> = proposal
        .argv_template
        .iter()
        .map(|t| fill_template(t, SMOKE_PROMPT, cli_binary))
        .collect();
    if argv.is_empty() {
        return (false, "empty argv_template".to_string());
    }
    let mut cmd = Command::new(&argv[0]);
    if argv.len() > 1 {
        cmd.args(&argv[1..]);
    }
    let is_pipe = proposal.stdin_mode.as_str() == Some("pipe");
    if is_pipe {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (false, format!("smoke-test failed to launch: {e}")),
    };
    if is_pipe {
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(SMOKE_PROMPT.as_bytes());
        }
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return (false, format!("smoke-test failed: {e}")),
    };
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let head: String = stderr.chars().take(300).collect();
        return (
            false,
            format!("smoke-test exit {:?}: {head}", out.status.code()),
        );
    }
    let mut output = String::from_utf8_lossy(&out.stdout).to_string();
    if let Some(parser) = proposal.stdout_parser.as_str() {
        if let Some(path) = parser.strip_prefix("json:") {
            output = extract_json_path(&output, path).unwrap_or_default();
        }
    }
    (output.to_uppercase().contains(SMOKE_EXPECTED), output)
}

fn persist_adapter(cli_name: &str, adapter: LearnedAdapter) {
    let path = default_path();
    let mut cfg = load(&path).unwrap_or_else(LopeCfg::defaults);
    cfg.learned_adapters.retain(|(n, _)| n != cli_name);
    cfg.learned_adapters.push((cli_name.to_string(), adapter));
    let _ = save(&cfg, &path);
}

pub fn build_heal_prompt(
    cli_name: &str,
    old_argv: &[String],
    stderr: &str,
    help_text: &str,
) -> String {
    let stderr_head: String = stderr.chars().take(1500).collect();
    let help_head: String = help_text.chars().take(4000).collect();
    format!(
        "You are helping lope repair a broken AI CLI adapter.\n\n\
Lope invoked the `{cli_name}` CLI with this argv and it failed:\n\n\
```\n{old_argv:?}\n```\n\n\
The stderr from the failure (truncated):\n\n\
```\n{stderr_head}\n```\n\n\
The current `{cli_name} --help` output is:\n\n\
```\n{help_head}\n```\n\n\
Lope needs this CLI to accept a single prompt string on the command line or\n\
on stdin, and respond with plaintext on stdout containing the model's answer.\n\n\
Propose a corrected invocation and return ONLY a JSON object wrapped in a\n\
triple-backtick json code fence. No prose before or after. Schema:\n\n\
```json\n{{\n  \"argv_template\": [\"<binary>\", \"<flag>\", \"{{prompt}}\"],\n  \"stdin_mode\": \"none\",\n  \"stdout_parser\": \"plaintext\",\n  \"confidence\": 0.85,\n  \"rationale\": \"one sentence\"\n}}\n```\n\n\
Rules:\n\
- `argv_template` uses `{{prompt}}` as the placeholder where lope injects\n  \
the prompt string. If the CLI reads prompts from stdin, set `stdin_mode`\n  \
to `\"pipe\"` and omit `{{prompt}}` from argv.\n\
- `stdout_parser` is `\"plaintext\"` if the CLI prints raw text, or\n  \
`\"json:path.to.field\"` if it prints JSON and the response is at that path.\n\
- `confidence` is your own estimate, 0.0 to 1.0. Be honest.\n\
- `rationale` is one sentence on what you changed and why.\n\n\
Return only the JSON. No other text."
    )
}

pub fn parse_heal_response(response: &str) -> Option<LearnedAdapter> {
    if response.is_empty() {
        return None;
    }
    static FENCE_RE: OnceLock<Regex> = OnceLock::new();
    let fence_re = FENCE_RE.get_or_init(|| Regex::new(r"(?s)```(?:json)?\s*(\{.*?\})\s*```").unwrap());
    static GREEDY_RE: OnceLock<Regex> = OnceLock::new();
    let greedy_re = GREEDY_RE.get_or_init(|| Regex::new(r"(?s)\{.*\}").unwrap());

    let mut candidates: Vec<&str> = fence_re
        .captures_iter(response)
        .filter_map(|c| c.get(1).map(|m| m.as_str()))
        .collect();
    if candidates.is_empty() {
        if let Some(m) = greedy_re.find(response) {
            candidates.push(m.as_str());
        }
    }

    for candidate in candidates {
        let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(candidate)
        else {
            continue;
        };
        let Some(serde_json::Value::Array(arr)) = map.get("argv_template") else {
            continue;
        };
        let mut argv = Vec::with_capacity(arr.len());
        let mut all_str = true;
        for item in arr {
            if let Some(s) = item.as_str() {
                argv.push(s.to_string());
            } else {
                all_str = false;
                break;
            }
        }
        if !all_str {
            continue;
        }
        return Some(LearnedAdapter {
            argv_template: argv,
            stdin_mode: map
                .get("stdin_mode")
                .cloned()
                .unwrap_or_else(|| json!("none")),
            stdout_parser: map
                .get("stdout_parser")
                .cloned()
                .unwrap_or_else(|| json!("plaintext")),
            timestamp: 0.0,
            source_cli: json!(""),
            confidence: map
                .get("confidence")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
        });
    }
    None
}

pub fn fill_template(template: &str, prompt: &str, cli_binary: &str) -> String {
    template
        .replace("{prompt}", prompt)
        .replace("{binary}", cli_binary)
}

pub fn extract_json_path(raw: &str, path: &str) -> Option<String> {
    let data: serde_json::Value = serde_json::from_str(raw).ok()?;
    let mut current = &data;
    for seg in path.split('.') {
        current = current.get(seg)?;
    }
    match current {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

pub fn is_adapter_expired(adapter: &LearnedAdapter, now: Option<f64>) -> bool {
    let now = now.unwrap_or_else(now_epoch);
    if adapter.timestamp <= 0.0 {
        return false;
    }
    (now - adapter.timestamp) > LEARNED_ADAPTER_TTL_SECONDS
}

pub fn build_healed_validator(
    cli_name: &str,
    adapter: &LearnedAdapter,
) -> Option<Arc<dyn Validator>> {
    let stdin = adapter.stdin_mode.as_str() == Some("pipe");
    lope_providers::generic::Subprocess::from_config(&json!({
        "name": cli_name,
        "type": "subprocess",
        "command": adapter.argv_template,
        "stdin": stdin,
    }))
    .ok()
    .map(|s| Arc::new(s) as Arc<dyn Validator>)
}

/// Detect flag-surface errors and coordinate one SelfHealer attempt + adapter build.
pub fn try_self_heal_for_validator(
    healer: &mut SelfHealer,
    target: &dyn Validator,
    candidates: &[Arc<dyn Validator>],
    err_msg: &str,
    ctx: Option<&Context>,
) -> Option<Arc<dyn Validator>> {
    if !is_flag_error(err_msg) {
        return None;
    }
    let reviewer = candidates
        .iter()
        .find(|v| v.name() != target.name() && v.available())?;
    let remaining = ctx.and_then(|c| c.budget.deadline.map(|_| c.budget.remaining(false)));
    if !healer.should_attempt(target.name(), true, remaining) {
        return None;
    }
    let old_argv = target.command_template();
    let cli_binary = old_argv
        .first()
        .map(String::as_str)
        .unwrap_or_else(|| target.name());
    let adapter = healer.attempt(
        target.name(),
        cli_binary,
        &old_argv,
        err_msg,
        reviewer.as_ref(),
        ctx,
    )?;
    build_healed_validator(target.name(), &adapter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lope_providers::errors::ProviderError;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn parse_heal_response_fenced() {
        let resp = "Here you go:\n```json\n{\"argv_template\":[\"mycli\",\"-p\",\"{prompt}\"],\"stdin_mode\":\"none\",\"stdout_parser\":\"json:result.text\",\"confidence\":0.9}\n```";
        let a = parse_heal_response(resp).expect("parsed");
        assert_eq!(a.argv_template, vec!["mycli", "-p", "{prompt}"]);
        assert_eq!(
            extract_json_path(r#"{"result":{"text":"OK"}}"#, "result.text"),
            Some("OK".to_string())
        );
    }

    struct MockReviewer {
        name: String,
        reply: String,
    }

    impl Validator for MockReviewer {
        fn name(&self) -> &str {
            &self.name
        }
        fn available(&self) -> bool {
            true
        }
        fn generate(
            &self,
            _prompt: &str,
            _timeout: f64,
            _ctx: Option<&Context>,
        ) -> Result<String, ProviderError> {
            Ok(self.reply.clone())
        }
        fn transport(&self) -> &'static str {
            "argv"
        }
    }

    #[test]
    fn self_heal_end_to_end_persists_learned_adapter_and_logs_journal() {
        // Test environment mutation safety: serialize tests touching LOPE_HOME/LOPE_SELF_HEAL.
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap();

        let dir = tempdir().unwrap();
        let lope_home = dir.path().join("lope-home");
        fs::create_dir_all(&lope_home).unwrap();
        fs::write(
            lope_home.join("config.json"),
            r#"{"version":1,"validators":["broken","claude"],"primary":"broken","timeout":30,"parallel":false,"providers":[]}"#,
        )
        .unwrap();

        // Mock CLI: `--help` prints new flag `--ask`; `--bad-flag` exits 2 with unrecognized option;
        // `--ask <prompt>` prints prompt-derived output (OK for smoke, PASS verdict for validate).
        let bin = dir.path().join("broken-cli");
        let script = r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  echo "Usage: broken-cli --ask <prompt>"
  exit 0
fi
if [ "$1" = "--bad-flag" ]; then
  echo "error: unrecognized argument '--bad-flag'" >&2
  exit 2
fi
if [ "$1" = "--ask" ]; then
  case "$2" in
    *"Reply with the single word OK"*) echo "OK" ;;
    *) printf -- "---VERDICT---\nstatus: PASS\nconfidence: 0.95\nrationale: verified in src/lib.rs:10 (cargo test passed)\n---END---\n" ;;
  esac
  exit 0
fi
exit 1
"#;
        fs::write(&bin, script).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();

        let prev_home = std::env::var("LOPE_HOME").ok();
        let prev_heal = std::env::var("LOPE_SELF_HEAL").ok();
        std::env::set_var("LOPE_HOME", &lope_home);
        std::env::set_var("LOPE_SELF_HEAL", "1");

        struct MockBroken {
            name: String,
            bin: PathBuf,
        }
        impl Validator for MockBroken {
            fn name(&self) -> &str {
                &self.name
            }
            fn available(&self) -> bool {
                true
            }
            fn transport(&self) -> &'static str {
                "argv"
            }
            fn command_template(&self) -> Vec<String> {
                vec![
                    self.bin.to_string_lossy().to_string(),
                    "--bad-flag".to_string(),
                    "{prompt}".to_string(),
                ]
            }
            fn generate(
                &self,
                prompt: &str,
                _timeout: f64,
                _ctx: Option<&Context>,
            ) -> Result<String, ProviderError> {
                let out = std::process::Command::new(&self.bin)
                    .args(["--bad-flag", prompt])
                    .output()
                    .map_err(|e| ProviderError::Runtime(e.to_string()))?;
                Err(ProviderError::Runtime(
                    String::from_utf8_lossy(&out.stderr).to_string(),
                ))
            }
        }

        let broken = MockBroken {
            name: "broken".into(),
            bin: bin.clone(),
        };
        let err = broken
            .generate("hello", 10.0, None)
            .expect_err("old flag should fail");
        let err_msg = err.to_string();
        assert!(is_flag_error(&err_msg), "expected flag error, got: {err_msg}");

        let reviewer: Arc<dyn Validator> = Arc::new(MockReviewer {
            name: "claude".into(),
            reply: format!(
                "```json\n{{\"argv_template\":[\"{}\",\"--ask\",\"{{prompt}}\"],\"stdin_mode\":\"none\",\"stdout_parser\":\"plaintext\",\"confidence\":0.92,\"rationale\":\"updated flag\"}}\n```",
                bin.display()
            ),
        });
        let mut healer = SelfHealer::new();
        let healed = try_self_heal_for_validator(&mut healer, &broken, &[reviewer], &err_msg, None)
            .expect("self-heal should produce healed validator");

        assert_eq!(
            healed.command_template(),
            vec![
                bin.to_string_lossy().to_string(),
                "--ask".to_string(),
                "{prompt}".to_string(),
            ]
        );
        let verify_out = std::process::Command::new(&bin)
            .args(["--ask", "verify"])
            .output()
            .unwrap();
        assert!(verify_out.status.success());
        assert!(String::from_utf8_lossy(&verify_out.stdout).contains("status: PASS"));

        let saved_cfg = load(&lope_home.join("config.json")).expect("saved config");
        assert!(
            saved_cfg.learned("broken").is_some(),
            "learned_adapters should contain 'broken'"
        );
        let events = crate::journal::read_recent(10);
        assert!(
            events.iter().any(|e| e["event"] == "heal_success"),
            "journal should record heal_success: {events:?}"
        );

        match prev_home {
            Some(v) => std::env::set_var("LOPE_HOME", v),
            None => std::env::remove_var("LOPE_HOME"),
        }
        match prev_heal {
            Some(v) => std::env::set_var("LOPE_SELF_HEAL", v),
            None => std::env::remove_var("LOPE_SELF_HEAL"),
        }
    }
}
