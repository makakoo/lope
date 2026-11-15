//! `lope team [list|enable|disable|remove|add|test|health]` (`cli.py:_cmd_team`
//! and helpers).

use super::{discover, fanout, scope, Native, Out};
use clap::ArgMatches;
use lope_core::config::{self, LopeCfg};
use lope_core::output::{json_text, ljust_str};
use lope_core::pyjson;
use lope_providers::curl::{parse_curl_command, shlex_split_strict};
use lope_providers::generic::{num, validate_provider_config};
use lope_providers::invocation::invoke_generate;
use lope_providers::pool;
use lope_providers::request_plan::{plan_direct, Limits};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const BUILTINS: [&str; 5] = ["aider", "claude", "codex", "gemini", "opencode"];
const AUTO: [&str; 10] = [
    "vibe",
    "ollama",
    "goose",
    "interpreter",
    "llama-cpp",
    "gh-copilot",
    "amazon-q",
    "pi",
    "qwen",
    "agy",
];

fn names(cfg: &LopeCfg) -> Vec<String> {
    cfg.validators
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn pname(p: &Value) -> Option<&str> {
    p.get("name").and_then(Value::as_str)
}

fn source(name: &str, cfg: &LopeCfg) -> String {
    if BUILTINS.contains(&name) {
        return "(built-in)".into();
    }
    if let Some(p) = cfg.providers.iter().find(|p| pname(p) == Some(name)) {
        return format!(
            "(custom {})",
            p.get("type").map(config::py_str).unwrap_or_else(|| "?".into())
        );
    }
    if AUTO.contains(&name) {
        return "(auto)".into();
    }
    "(?)".into()
}

fn known(name: &str, cfg: &LopeCfg) -> bool {
    BUILTINS.contains(&name)
        || cfg.providers.iter().any(|p| pname(p) == Some(name))
        || AUTO.contains(&name)
}

fn repr(s: &str) -> String {
    lope_state::registry::py_repr_str(s)
}

fn active_lines(cfg: &LopeCfg, out: &mut Out) {
    let n = names(cfg);
    out.line(format!("     Active validators ({}): {}", n.len(), n.join(", ")));
    if !cfg.primary.is_empty() {
        out.line(format!("     Primary: {}", cfg.primary));
    }
}

pub fn run(sub: Option<&str>, m: &ArgMatches) -> Native {
    let path = config::default_path();
    let loaded = config::load(&path);
    if let Some(c) = &loaded {
        if c.validators.iter().any(|v| !v.is_string())
            || c.providers.iter().any(|p| !p.is_object())
        {
            return Native::Delegate("unusual config shape".into());
        }
    }
    let mut cfg = loaded.unwrap_or_else(|| {
        let mut c = LopeCfg::defaults();
        c.timeout = Value::from(480);
        c
    });
    let args: Vec<String> = m
        .try_get_many::<std::ffi::OsString>("names")
        .ok()
        .flatten()
        .map(|v| v.map(|s| s.to_string_lossy().into_owned()).collect())
        .or_else(|| {
            m.try_get_one::<std::ffi::OsString>("name")
                .ok()
                .flatten()
                .map(|s| vec![s.to_string_lossy().into_owned()])
        })
        .unwrap_or_default();
    let sub_name = sub.unwrap_or("list");
    if !matches!(sub_name, "add" | "test" | "health")
        && args
            .iter()
            .any(|a| a.chars().any(|c| c.is_control() || c == '\\' || c == '\'' || c == '"'))
    {
        return Native::Delegate("name needs Python repr".into());
    }
    let mut out = Out::new();
    let save = |cfg: &LopeCfg| config::save(cfg, &path).map_err(|e| e.to_string());
    match sub_name {
        "enable" => {
            let mut changed = vec![];
            for raw in &args {
                let name = raw.trim();
                if name.is_empty() {
                    continue;
                }
                if !known(name, &cfg) {
                    drop(out);
                    eprintln!(
                        "ERROR: cannot enable unknown teammate {}.\n       Built-ins: {}.\n       Custom teammates must be added first with `lope team add {name} --cmd ...`.",
                        repr(name),
                        BUILTINS.join(", ")
                    );
                    return Native::Exit(2);
                }
                if !names(&cfg).iter().any(|n| n == name) {
                    cfg.validators.push(Value::from(name));
                    changed.push(name.to_string());
                }
            }
            if cfg.validators.is_empty() {
                drop(out);
                eprintln!("ERROR: no teammate names supplied");
                return Native::Exit(2);
            }
            let primary = m
                .try_get_one::<std::ffi::OsString>("--primary")
                .ok()
                .flatten()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty());
            let n = names(&cfg);
            if let Some(p) = primary {
                if !n.contains(&p) {
                    drop(out);
                    eprintln!(
                        "ERROR: --primary {} must be one of the enabled validators.",
                        repr(&p)
                    );
                    return Native::Exit(2);
                }
                cfg.primary = p;
            } else if cfg.primary.is_empty() || !n.contains(&cfg.primary) {
                cfg.primary = n[0].clone();
            }
            if let Err(e) = save(&cfg) {
                return Native::Delegate(format!("save failed: {e}"));
            }
            if changed.is_empty() {
                out.line("[OK] already enabled.");
            } else {
                out.line(format!("[OK] enabled {}.", changed.join(", ")));
            }
            active_lines(&cfg, &mut out);
            Native::Exit(0)
        }
        "disable" => {
            let requested: Vec<String> = args
                .iter()
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect();
            if requested.is_empty() {
                drop(out);
                eprintln!("ERROR: no teammate names supplied");
                return Native::Exit(2);
            }
            let active = names(&cfg);
            let missing: Vec<&String> =
                requested.iter().filter(|n| !active.contains(n)).collect();
            if !missing.is_empty() {
                drop(out);
                let a = if active.is_empty() {
                    "(none)".to_string()
                } else {
                    active.join(", ")
                };
                eprintln!(
                    "ERROR: not active: {}.\n       Active: {a}",
                    missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                );
                return Native::Exit(1);
            }
            cfg.validators
                .retain(|v| !requested.iter().any(|r| v.as_str() == Some(r)));
            if requested.contains(&cfg.primary) {
                cfg.primary = names(&cfg).first().cloned().unwrap_or_default();
            }
            if let Err(e) = save(&cfg) {
                return Native::Delegate(format!("save failed: {e}"));
            }
            out.line(format!("[OK] disabled {}.", requested.join(", ")));
            if cfg.validators.is_empty() {
                out.line(
                    "     No active validators. Enable one with `lope team enable claude` or add a custom teammate.",
                );
            } else {
                active_lines(&cfg, &mut out);
            }
            Native::Exit(0)
        }
        "remove" => {
            let name = args.first().map(|s| s.trim().to_string()).unwrap_or_default();
            if name.is_empty() {
                drop(out);
                eprintln!("ERROR: name required");
                return Native::Exit(2);
            }
            let before = cfg.providers.len();
            cfg.providers.retain(|p| pname(p) != Some(&name));
            let removed_provider = cfg.providers.len() < before;
            let was_validator = names(&cfg).contains(&name);
            if was_validator {
                cfg.validators.retain(|v| v.as_str() != Some(&name));
            }
            let was_primary = cfg.primary == name;
            if was_primary {
                cfg.primary = names(&cfg).first().cloned().unwrap_or_default();
            }
            if !removed_provider && !was_validator {
                drop(out);
                eprintln!(
                    "ERROR: no teammate named {} found in providers or validators.\n       Run `lope team list` to see who's on the team.",
                    repr(&name)
                );
                return Native::Exit(1);
            }
            if let Err(e) = save(&cfg) {
                return Native::Delegate(format!("save failed: {e}"));
            }
            let mut bits = vec![];
            if removed_provider {
                bits.push("custom provider config");
            }
            if was_validator {
                bits.push("active validators");
            }
            if was_primary {
                bits.push("primary role");
            }
            out.line(format!("[OK] removed {name} from {}.", bits.join(", ")));
            if cfg.validators.is_empty() {
                out.line("     No validators remaining — add one with `lope team add`.");
            } else {
                active_lines(&cfg, &mut out);
            }
            Native::Exit(0)
        }
        "add" => team_add(m, &mut cfg, &path, &mut out),
        "test" => team_test(m, &cfg, &mut out),
        "health" => team_health(m, &cfg, &mut out),
        _ => {
            out.blank();
            out.line(lope_core::logo::boxed(""));
            out.line("\nLope — Your Validator Team");
            out.line("-".repeat(40));
            let active = names(&cfg);
            if active.is_empty() {
                out.line(
                    "\n  (no active validators yet — add one with `lope team add <name> ...`)",
                );
            } else {
                out.line(format!("\nActive ({}):", active.len()));
                for n in &active {
                    let marker = if *n == cfg.primary { " ★ primary" } else { "" };
                    out.line(format!(
                        "  {} {}{marker}",
                        ljust_str(n, 22),
                        source(n, &cfg)
                    ));
                }
            }
            let disabled: Vec<&Value> = cfg
                .providers
                .iter()
                .filter(|p| !active.iter().any(|a| Some(a.as_str()) == pname(p)))
                .collect();
            if !disabled.is_empty() {
                out.line(format!("\nDisabled providers ({}):", disabled.len()));
                for e in disabled {
                    let nm = e
                        .get("name")
                        .map(config::py_str)
                        .unwrap_or_else(|| "?".into());
                    let ty = e
                        .get("type")
                        .map(config::py_str)
                        .unwrap_or_else(|| "?".into());
                    out.line(format!("  {} ({ty})", ljust_str(&nm, 22)));
                }
            }
            let avail: Vec<&str> = BUILTINS
                .iter()
                .copied()
                .filter(|b| !active.iter().any(|a| a == b) && lope_core::paths::which(b).is_some())
                .collect();
            let _ = discover::KNOWN_CLIS.len();
            if !avail.is_empty() {
                out.line(format!("\nAvailable built-ins ({}):", avail.len()));
                for b in avail {
                    out.line(format!("  {} enable: lope team enable {b}", ljust_str(b, 22)));
                }
            }
            out.blank();
            out.line("Add a teammate:");
            out.line("  lope team add <name> --cmd \"binary --flag {prompt}\"");
            out.line("  lope team add <name> --url URL --model MODEL --key-env OPENAI_API_KEY");
            out.line("Enable a built-in:");
            out.line("  lope team enable codex opencode");
            out.blank();
            Native::Exit(0)
        }
    }
}

fn team_add(
    m: &ArgMatches,
    cfg: &mut LopeCfg,
    path: &std::path::Path,
    out: &mut Out,
) -> Native {
    let raw_name = scope::os_str(m, "name").unwrap_or_default();
    let name = raw_name.trim().to_string();
    if name.is_empty() {
        eprintln!("ERROR: name cannot be empty");
        return Native::Exit(2);
    }
    if BUILTINS.contains(&name.as_str()) {
        let extra = if lope_core::paths::which(&name).is_some() {
            format!("Enable it with: lope team enable {name}")
        } else {
            format!("Install the {name} CLI first, then run: lope team enable {name}")
        };
        eprintln!(
            "ERROR: {} is a built-in validator, not a custom provider name. {extra}. For one-off use: lope ask \"...\" --validators {name}",
            repr(&name)
        );
        return Native::Exit(2);
    }
    if name
        .chars()
        .any(|ch| matches!(ch, ' ' | '\t' | '\n' | ',' | ';' | '|'))
    {
        eprintln!("ERROR: name must not contain whitespace or separators");
        return Native::Exit(2);
    }

    let force = scope::flag(m, "--force");
    let existing = cfg.providers.iter().any(|p| pname(p) == Some(name.as_str()));
    if existing && !force {
        eprintln!(
            "ERROR: provider {} already exists — use --force to overwrite or `lope team remove {name}` first.",
            repr(&name)
        );
        return Native::Exit(2);
    }

    let from_curl = scope::os_str(m, "--from-curl").filter(|s| !s.is_empty());
    let url = scope::os_str(m, "--url").filter(|s| !s.is_empty());
    let cmd = scope::os_str(m, "--cmd").filter(|s| !s.is_empty());
    let body_json = scope::os_str(m, "--body-json").filter(|s| !s.is_empty());
    let model = scope::os_str(m, "--model").filter(|s| !s.is_empty());
    let key_env = scope::os_str(m, "--key-env").filter(|s| !s.is_empty());
    let key_header = scope::os_str(m, "--key-header").unwrap_or_else(|| "Authorization".into());
    let key_prefix = scope::os_str(m, "--key-prefix").unwrap_or_else(|| "Bearer ".into());
    let response_path = scope::os_str(m, "--response-path").filter(|s| !s.is_empty());
    let wrap = scope::os_str(m, "--wrap").filter(|s| !s.is_empty());
    let timeout = m.try_get_one::<i64>("--timeout").ok().flatten().copied();
    let max_tokens = m.try_get_one::<i64>("--max-tokens").ok().flatten().copied();
    let stdin = scope::flag(m, "--stdin");
    let disabled = scope::flag(m, "--disabled");
    let primary = scope::flag(m, "--primary");

    let mode_count =
        usize::from(from_curl.is_some()) + usize::from(url.is_some()) + usize::from(cmd.is_some());
    if mode_count > 1 {
        eprintln!("ERROR: --from-curl, --url, and --cmd are mutually exclusive — pick one.");
        return Native::Exit(2);
    }
    if from_curl.is_some() && body_json.is_some() {
        eprintln!(
            "ERROR: --from-curl already sets the body from the pasted curl; --body-json would override it. Drop one."
        );
        return Native::Exit(2);
    }

    let entry = if let Some(fc) = from_curl {
        match parse_curl_command(
            &fc,
            &name,
            key_env.as_deref(),
            response_path.as_deref(),
            wrap.as_deref(),
            timeout,
            max_tokens,
        ) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("ERROR: {e}");
                return Native::Exit(2);
            }
        }
    } else if let Some(u) = url {
        if !u.starts_with("http://") && !u.starts_with("https://") {
            eprintln!("ERROR: --url must start with http:// or https://");
            return Native::Exit(2);
        }
        let mut headers = Map::new();
        headers.insert("Content-Type".into(), Value::String("application/json".into()));
        if let Some(ke) = key_env {
            let env_var = ke.trim();
            if env_var.is_empty() {
                eprintln!("ERROR: --key-env cannot be empty");
                return Native::Exit(2);
            }
            let stripped = env_var.replace('_', "");
            if stripped.is_empty() || !stripped.chars().all(|c| c.is_alphanumeric()) {
                eprintln!(
                    "ERROR: --key-env {} must be a valid env var name (A-Z, 0-9, _)",
                    repr(env_var)
                );
                return Native::Exit(2);
            }
            headers.insert(
                key_header,
                Value::String(format!("{key_prefix}${{{env_var}}}")),
            );
        }
        let mut body = if let Some(bj) = body_json {
            match pyjson::loads(&bj) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("ERROR: --body-json is not valid JSON: {e}");
                    return Native::Exit(2);
                }
            }
        } else {
            let Some(mod_name) = model else {
                eprintln!(
                    "ERROR: HTTP provider needs --model (for OpenAI-compatible body), or use --body-json to supply a custom shape."
                );
                return Native::Exit(2);
            };
            json!({
                "model": mod_name,
                "messages": [{"role": "user", "content": "{prompt}"}]
            })
        };
        if let Some(mt) = max_tokens {
            let serialized = pyjson::dumps(&body, &pyjson::Opts::default());
            if !serialized.contains("{max_tokens}") {
                if let Value::Object(ref mut bm) = body {
                    bm.insert("max_tokens".into(), Value::from(mt));
                }
            }
        }
        let mut em = Map::new();
        em.insert("name".into(), Value::String(name.clone()));
        em.insert("type".into(), Value::String("http".into()));
        em.insert("url".into(), Value::String(u));
        em.insert("headers".into(), Value::Object(headers));
        em.insert("body".into(), body);
        em.insert(
            "response_path".into(),
            Value::String(
                response_path.unwrap_or_else(|| "choices.0.message.content".into()),
            ),
        );
        if let Some(w) = wrap {
            em.insert("prompt_wrapper".into(), Value::String(w));
        }
        if let Some(t) = timeout.filter(|&t| t != 0) {
            em.insert("timeout".into(), Value::from(t));
        }
        if let Some(mt) = max_tokens {
            em.insert("max_tokens".into(), Value::from(mt));
        }
        Value::Object(em)
    } else if let Some(c) = cmd {
        let mut tokens = match shlex_split_strict(&c) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("ERROR: could not parse --cmd (unclosed quote?): {e}");
                return Native::Exit(2);
            }
        };
        if tokens.is_empty() {
            eprintln!("ERROR: --cmd must have at least one token (the binary name)");
            return Native::Exit(2);
        }
        if !stdin && !tokens.join(" ").contains("{prompt}") {
            tokens.push("{prompt}".into());
        }
        let mut em = Map::new();
        em.insert("name".into(), Value::String(name.clone()));
        em.insert("type".into(), Value::String("subprocess".into()));
        em.insert(
            "command".into(),
            Value::Array(tokens.into_iter().map(Value::String).collect()),
        );
        if stdin {
            em.insert("stdin".into(), Value::Bool(true));
        }
        if let Some(w) = wrap {
            em.insert("prompt_wrapper".into(), Value::String(w));
        }
        if let Some(t) = timeout.filter(|&t| t != 0) {
            em.insert("timeout".into(), Value::from(t));
        }
        Value::Object(em)
    } else {
        eprintln!(
            "ERROR: provide one of --cmd (subprocess), --url (HTTP), or --from-curl (paste a curl command).\nExamples:\n  lope team add my-ollama --cmd \"ollama run qwen3:8b {{prompt}}\"\n  lope team add openclaw --url http://10.42.42.1:18080/v1/chat/completions --model openclaw --key-env OPENAI_API_KEY\n  lope team add openai --from-curl \"curl https://api.openai.com/v1/chat/completions -H 'Authorization: Bearer \\${{OPENAI_API_KEY}}' -H 'Content-Type: application/json' -d '{{\\\"model\\\":\\\"gpt-4o-mini\\\",\\\"messages\\\":[{{\\\"role\\\":\\\"user\\\",\\\"content\\\":\\\"hi\\\"}}]}}'\""
        );
        return Native::Exit(2);
    };

    if let Err(e) = validate_provider_config(&entry) {
        eprintln!("ERROR: invalid config: {e}");
        return Native::Exit(2);
    }

    cfg.providers.retain(|p| pname(p) != Some(name.as_str()));
    cfg.providers.push(entry.clone());

    if !disabled {
        if !names(cfg).contains(&name) {
            cfg.validators.push(Value::String(name.clone()));
        }
        if cfg.primary.is_empty() && !cfg.validators.is_empty() {
            cfg.primary = name.clone();
        }
    }

    if primary {
        if disabled {
            eprintln!(
                "ERROR: cannot set --primary and --disabled together (disabled validators can't be primary)."
            );
            return Native::Exit(2);
        }
        cfg.primary = name.clone();
    }

    if let Err(e) = config::save(cfg, path) {
        return Native::Delegate(format!("save failed: {e}"));
    }

    let status = if !disabled {
        "added to team"
    } else {
        "saved (disabled — enable with `lope team add` again without --disabled)"
    };
    let kind = entry
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("subprocess");
    out.line(format!("[OK] {name} {status} ({kind})."));
    let n = names(cfg);
    if !n.is_empty() {
        out.line(format!("     Active validators ({}): {}", n.len(), n.join(", ")));
    }
    if !cfg.primary.is_empty() {
        out.line(format!("     Primary: {}", cfg.primary));
    }
    out.line(format!("     Smoke-test now: lope team test {name}"));
    Native::Exit(0)
}

fn team_test(m: &ArgMatches, cfg: &LopeCfg, out: &mut Out) -> Native {
    let pool = pool::build(cfg);
    if !pool.unsupported.is_empty() {
        return Native::Delegate(pool.unsupported.join("; "));
    }
    for w in &pool.warnings {
        eprintln!("{w}");
    }
    let scope = match scope::start("team", cfg, false) {
        Ok(s) => s,
        Err(e) => return Native::Delegate(format!("runtime start failed: {e}")),
    };

    let name = scope::os_str(m, "name")
        .unwrap_or_default()
        .trim()
        .to_string();
    if name.is_empty() {
        out.flush();
        eprintln!("ERROR: name required");
        return Native::Exit(scope::finish(scope, 2));
    }

    let validator = pool.validators.iter().find(|v| v.name() == name).cloned();
    let Some(validator) = validator else {
        let active = {
            let n = names(cfg);
            if n.is_empty() {
                "(none)".to_string()
            } else {
                n.join(", ")
            }
        };
        eprintln!(
            "ERROR: {} is not on the team.\n       Active: {active}.\n       Run `lope team list` to see the full roster.",
            repr(&name)
        );
        return Native::Exit(scope::finish(scope, 1));
    };

    let prompt = scope::os_str(m, "prompt")
        .unwrap_or_else(|| "Say hello in one word.".into());
    let timeout = m
        .try_get_one::<i64>("--timeout")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(60) as f64;

    out.line(format!("[test] {name} ← {}", repr(&prompt)));
    out.flush();

    match invoke_generate(
        validator.as_ref(),
        &prompt,
        timeout,
        Some(&scope.ctx),
        Some("team_test"),
    ) {
        Err(e) => {
            eprintln!("ERROR: {} failed: {}", repr(&name), fanout::err_text(&e));
            Native::Exit(scope::finish(scope, 1))
        }
        Ok(raw) => {
            let text = lope_providers::pyre::strip(&raw);
            if text.is_empty() {
                eprintln!("[warn] {name} returned empty output");
                Native::Exit(scope::finish(scope, 1))
            } else {
                out.line("-".repeat(60));
                out.line(text);
                out.line("-".repeat(60));
                out.line(format!(
                    "[OK] {name} responded ({} chars).",
                    text.chars().count()
                ));
                out.flush();
                Native::Exit(scope::finish(scope, 0))
            }
        }
    }
}

fn team_health(m: &ArgMatches, cfg: &LopeCfg, out: &mut Out) -> Native {
    let pool = pool::build(cfg);
    if !pool.unsupported.is_empty() {
        return Native::Delegate(pool.unsupported.join("; "));
    }
    for w in &pool.warnings {
        eprintln!("{w}");
    }
    let json_mode = scope::flag(m, "--json");
    let scope = match scope::start("team", cfg, json_mode) {
        Ok(s) => s,
        Err(e) => return Native::Delegate(format!("runtime start failed: {e}")),
    };

    let timeout_int = m
        .try_get_one::<i64>("--timeout")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(30);
    let timeout = timeout_int as f64;
    let prompt = "Say OK only.";

    let rows = fanout::fanout_generate(
        &pool.validators,
        pool.parallel,
        prompt,
        timeout,
        &scope.ctx,
    );
    let mut by_name: HashMap<String, (String, String)> = HashMap::new();
    for (n, ans, err) in rows {
        let err_s = err.as_ref().map(fanout::err_text).unwrap_or_default();
        by_name.insert(n, (ans, err_s));
    }

    let snap = scope.ctx.budget.snapshot();
    let mut durations: HashMap<String, f64> = HashMap::new();
    if let Some(recs) = snap.get("records").and_then(Value::as_array) {
        for r in recs {
            if let (Some(v), Some(d)) = (
                r.get("validator").and_then(Value::as_str),
                r.get("duration_seconds").and_then(Value::as_f64),
            ) {
                *durations.entry(v.to_string()).or_insert(0.0) += d;
            }
        }
    }

    let default_lim = Limits {
        max_chunks: 32,
        max_calls: 96,
        max_input_bytes: 16 * 1024 * 1024,
    };

    let mut results: Vec<Value> = Vec::new();
    for v in &pool.validators {
        let vname = v.name().to_string();
        if !v.available() {
            results.push(json!({
                "name": vname,
                "status": "FAIL",
                "reason": "binary not found",
                "elapsed": 0.0,
                "chars": 0,
                "probe": "tiny",
            }));
            continue;
        }
        let (answer, error) = by_name
            .get(&vname)
            .cloned()
            .unwrap_or_else(|| (String::new(), "not scheduled".into()));
        let elapsed = durations.get(&vname).copied().unwrap_or(0.0);
        let text = lope_providers::pyre::strip(&answer);
        let chars = text.chars().count();
        let admission = plan_direct(
            prompt,
            "team_health",
            std::slice::from_ref(v),
            "auto",
            &default_lim,
            timeout,
            true,
            0,
        )
        .json;

        if !error.is_empty() {
            let lowered = error.to_ascii_lowercase();
            let reason = if lowered.contains("timed out") || lowered.contains("timeout") {
                format!("timeout after {}s", num(timeout))
            } else {
                format!("error: {}", lope_providers::pyre::head(&error, 120))
            };
            results.push(json!({
                "name": vname,
                "status": "FAIL",
                "reason": reason,
                "elapsed": elapsed,
                "chars": 0,
                "probe": "tiny",
                "admission": admission,
            }));
        } else if text.is_empty() {
            results.push(json!({
                "name": vname,
                "status": "FAIL",
                "reason": "returned empty output",
                "elapsed": elapsed,
                "chars": 0,
                "probe": "tiny",
                "admission": admission,
            }));
        } else {
            results.push(json!({
                "name": vname,
                "status": "PASS",
                "reason": format!("{elapsed:.1}s, {chars} chars"),
                "elapsed": elapsed,
                "chars": chars,
                "probe": "tiny",
                "admission": admission,
            }));
        }
    }

    let passed_count = results.iter().filter(|r| r["status"] == "PASS").count();
    let failed_count = results.iter().filter(|r| r["status"] == "FAIL").count();
    let skipped_count = results.iter().filter(|r| r["status"] == "SKIP").count();

    if json_mode {
        let payload = json!({
            "schema_version": 1,
            "probe": "tiny_prompt_health",
            "results": results,
            "summary": {
                "passed": passed_count,
                "failed": failed_count,
                "skipped": skipped_count,
            },
            "runtime": scope.ctx.budget.snapshot(),
        });
        out.line(json_text(&payload));
        out.flush();
        return Native::Exit(scope::finish(scope, 0));
    }

    for r in &results {
        let status = r["status"].as_str().unwrap_or("FAIL");
        let nm = r["name"].as_str().unwrap_or("unknown");
        let reason = r["reason"].as_str().unwrap_or("");
        out.line(format!(
            "[{}] {}  {reason}",
            ljust_str(status, 4),
            ljust_str(nm, 20)
        ));
    }
    out.line(format!(
        "\n{passed_count} passed, {failed_count} failed, {} skipped",
        results.len() - passed_count - failed_count
    ));

    if failed_count > 0 {
        out.line("\nActionable fixes:");
        for f in results.iter().filter(|r| r["status"] == "FAIL") {
            let nm = f["name"].as_str().unwrap_or("unknown");
            let reason = f["reason"].as_str().unwrap_or("");
            if reason.contains("not found") {
                out.line(format!(
                    "  - {nm}: install the CLI binary or add a custom provider"
                ));
            } else if reason.contains("timeout") {
                out.line(format!(
                    "  - {nm}: tiny prompt timed out. Verify provider health first; for real workloads inspect request plan and prefer stdin/file transport or bounded chunking before increasing --timeout."
                ));
            } else {
                out.line(format!("  - {nm}: {reason}"));
            }
        }
        out.flush();
        return Native::Exit(scope::finish(scope, 1));
    }

    out.flush();
    Native::Exit(scope::finish(scope, 0))
}
