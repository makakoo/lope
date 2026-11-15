//! `build_validator_pool` (generate path). Resolution: learned adapter →
//! built-in → `providers` → KNOWN_CLIS generic command → Makakoo adapter.
//! Anything not native yet is reported in `unsupported` so the verb can
//! delegate to Python before any side effect.

use crate::builtin::{Builtin, MakakooAdapter};
use crate::generic::{Http, Subprocess, Validator};
use lope_core::config::LopeCfg;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct Pool {
    pub validators: Vec<Arc<dyn Validator>>,
    pub primary: String,
    pub parallel: bool,
    /// EnsemblePool (true) vs single-validator ValidatorPool.
    pub ensemble: bool,
    pub unsupported: Vec<String>,
    /// Python `log.warning` lines (`lope.validators | ...`).
    pub warnings: Vec<String>,
}

impl Pool {
    pub fn names(&self) -> Vec<String> {
        self.validators.iter().map(|v| v.name().to_string()).collect()
    }
    pub fn primary_validator(&self) -> Option<Arc<dyn Validator>> {
        self.validators.iter().find(|v| v.name() == self.primary).cloned().or_else(|| self.validators.first().cloned())
    }
    pub fn reviewers(&self) -> Vec<Arc<dyn Validator>> {
        let primary = self.primary_validator();
        self.validators
            .iter()
            .filter(|v| primary.as_ref().is_none_or(|p| !Arc::ptr_eq(v, p)))
            .cloned()
            .collect()
    }
}

const KNOWN_GENERIC: [(&str, &[&str]); 10] = [
    ("vibe", &["vibe", "run", "{prompt}"]),
    ("ollama", &["ollama", "run", "qwen3:8b", "{prompt}"]),
    ("goose", &["goose", "run", "--text", "{prompt}"]),
    ("interpreter", &["interpreter", "--fast", "-y", "{prompt}"]),
    ("llama-cpp", &["llama-cli", "-p", "{prompt}", "--no-display-prompt"]),
    ("gh-copilot", &["gh", "copilot", "suggest", "{prompt}"]),
    ("amazon-q", &["q", "chat", "{prompt}"]),
    ("pi", &["pi", "--no-session", "--offline", "--no-tools", "-p", "{prompt}"]),
    ("qwen", &["qwen", "-p", "{prompt}"]),
    ("agy", &["agy", "-p", "{prompt}"]),
];

fn makakoo_adapters() -> Vec<String> {
    crate::builtin::enumerate_registered_adapters()
}

const LEARNED_TTL: f64 = 90.0 * 24.0 * 3600.0;

pub fn build(cfg: &LopeCfg) -> Pool {
    let mut providers: Vec<(String, Value)> = Vec::new();
    for e in &cfg.providers {
        if let Some(n) = e.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            providers.retain(|(k, _)| k != n);
            providers.push((n.to_string(), e.clone()));
        }
    }
    let mut makakoo: Option<Vec<String>> = None;
    let mut pool = Pool {
        validators: vec![],
        primary: String::new(),
        parallel: cfg.parallel,
        ensemble: false,
        unsupported: vec![],
        warnings: vec![],
    };
    let mut primary: Option<String> = None;
    for raw in &cfg.validators {
        let name = lope_core::config::py_str(raw);
        let mut v: Option<Arc<dyn Validator>> = None;
        if let Some(a) = cfg.learned(&name) {
            let expired = a.timestamp > 0.0 && lope_core::time::now() - a.timestamp > LEARNED_TTL;
            if !expired {
                let stdin = a.stdin_mode.as_str() == Some("pipe");
                match Subprocess::from_config(
                    &json!({"name": name, "type": "subprocess", "command": a.argv_template, "stdin": stdin}),
                ) {
                    Ok(s) => v = Some(Arc::new(s)),
                    Err(_) => pool.unsupported.push(format!("{name}: learned adapter")),
                }
            }
        }
        let builtin = match name.as_str() {
            "claude" => Some("claude"),
            "opencode" => Some("opencode"),
            "gemini" => Some("gemini-cli"),
            "codex" => Some("codex"),
            "aider" => Some("aider"),
            _ => None,
        };
        if v.is_none() {
            if let Some(k) = builtin {
                v = Some(Arc::new(Builtin::new(k)));
            } else if let Some((_, e)) = providers.iter().find(|(k, _)| *k == name) {
                match e.get("type").and_then(Value::as_str) {
                    Some("subprocess") => match Subprocess::from_config(e) {
                        Ok(s) => {
                            if let Some(u) = s.unsupported() {
                                pool.unsupported.push(u);
                            }
                            v = Some(Arc::new(s));
                        }
                        Err(_) => {
                            pool.unsupported.push(format!("{name}: provider config error"));
                            continue;
                        }
                    },
                    Some("http") => match Http::from_config(e) {
                        Ok(h) => {
                            if let Some(u) = h.unsupported() {
                                pool.unsupported.push(u);
                            }
                            v = Some(Arc::new(h));
                        }
                        Err(_) => {
                            pool.unsupported.push(format!("{name}: provider config error"));
                            continue;
                        }
                    },
                    _ => {
                        pool.unsupported.push(format!("{name}: provider config error"));
                        continue;
                    }
                }
            } else if let Some((_, cmd)) = KNOWN_GENERIC.iter().find(|(k, _)| *k == name) {
                v = Some(Arc::new(
                    Subprocess::from_config(&json!({"name": name, "type": "subprocess", "command": cmd})).unwrap(),
                ));
            } else if makakoo.get_or_insert_with(makakoo_adapters).contains(&name) {
                v = Some(Arc::new(MakakooAdapter::new(&name)));
            } else {
                pool.warnings.push(format!("lope.validators | Unknown validator: {name}"));
                continue;
            }
        }
        let v = v.unwrap();
        if name == cfg.primary {
            primary = Some(v.name().to_string());
        }
        pool.validators.push(v);
    }
    if pool.validators.is_empty() {
        pool.unsupported.push("no valid validators (Python raises)".into());
        return pool;
    }
    let primary = primary.unwrap_or_else(|| pool.validators[0].name().to_string());
    if pool.validators.len() > 1 {
        pool.ensemble = true;
    } else {
        pool.parallel = true; // ValidatorPool has no _parallel attribute
        pool.validators.retain(|v| v.name() == primary);
    }
    pool.primary = primary;
    pool
}
