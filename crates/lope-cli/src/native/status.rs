//! `lope status` (`cli.py:_cmd_status`), output byte-identical.

use super::{discover::discover, Out};
use lope_core::config::{self, py_str};
use lope_core::output::{get_str, ljust, ljust_str};
use lope_core::{journal, logo};
use lope_state::registry::RunRegistry;
use serde_json::Value;

const LEARNED_ADAPTER_TTL_SECONDS: f64 = 90.0 * 24.0 * 3600.0;

pub fn run() -> i32 {
    let mut out = Out::new();
    let available = discover();
    let cfg_path = config::default_path();
    let cfg = config::load(&cfg_path);
    out.blank();
    out.line(logo::boxed(""));
    out.line("\nLope — Validator Status");
    out.line("-".repeat(40));
    out.line("\nDetected built-in CLIs:");
    for cli in &available {
        let marker = if cli.is_default { " * DEFAULT" } else { "" };
        out.line(format!("  {} ({}){marker}", ljust_str(cli.display_name, 20), cli.name));
    }
    if available.is_empty() {
        out.line("  (none detected)");
    }
    if let Some(cfg) = &cfg {
        if !cfg.providers.is_empty() {
            out.line(format!("\nCustom providers ({}):", cfg.providers.len()));
            for p in &cfg.providers {
                let name = p.get("name").cloned().unwrap_or(Value::from("?"));
                let ptype = get_str(p, "type", "?");
                out.line(format!("  {} ({ptype})", ljust(&name, 20)));
            }
        }
        out.line(format!("\nConfig: {}", cfg_path.display()));
        out.line(format!("  Validators: {}", cfg.validator_names().join(", ")));
        out.line(format!("  Primary: {}", cfg.primary));
        out.line(format!("  Parallel: {}", if cfg.parallel { "True" } else { "False" }));
        out.line(format!("  Timeout: {}s", py_str(&cfg.timeout)));
        let run_timeout = match &cfg.run_timeout {
            Some(v) if config::py_int(v).is_some_and(|i| i != 0) => py_str(v),
            _ => (if cfg.allow_unbounded_run { "unbounded" } else { "mode default" }).to_string(),
        };
        out.line(format!("  Run timeout: {run_timeout}"));
        out.line(format!("  Max calls/chunks: {}/{}", py_str(&cfg.max_calls), py_str(&cfg.max_chunks)));
        if !cfg.learned_adapters.is_empty() {
            out.line(format!("\nLearned adapters ({}):", cfg.learned_adapters.len()));
            let now = lope_core::time::now();
            for (name, a) in &cfg.learned_adapters {
                let age_days: i64 = if a.timestamp > 0.0 { ((now - a.timestamp) / 86400.0).trunc() as i64 } else { -1 };
                let expired = a.timestamp > 0.0 && (now - a.timestamp) > LEARNED_ADAPTER_TTL_SECONDS;
                let warn = if expired {
                    " [EXPIRED — will re-verify on next run]"
                } else if age_days >= 60 {
                    " [aging — re-verify soon]"
                } else {
                    ""
                };
                let src = a.source_cli_str();
                let src = if src.is_empty() { "?".to_string() } else { src };
                let conf = if a.confidence > 0.0 { format!("{:.2}", a.confidence) } else { "?".into() };
                out.line(format!("  {} from {src}, {age_days}d ago, conf={conf}{warn}", ljust_str(name, 20)));
            }
        }
        let recent = journal::read_recent(5);
        let heal: Vec<&Value> =
            recent.iter().filter(|e| e.get("event").map(py_str).unwrap_or_default().starts_with("heal_")).collect();
        if !heal.is_empty() {
            out.line(format!("\nRecent heal events ({}):", heal.len()));
            let start = heal.len().saturating_sub(5);
            for evt in &heal[start..] {
                let ts = evt.get("timestamp").and_then(Value::as_f64).unwrap_or(0.0);
                let age: i64 = if ts != 0.0 { ((lope_core::time::now() - ts) / 60.0).trunc() as i64 } else { -1 };
                out.line(format!(
                    "  {} {} {age}m ago",
                    ljust(evt.get("event").unwrap_or(&Value::from("?")), 16),
                    ljust(evt.get("cli").unwrap_or(&Value::from("?")), 16)
                ));
            }
        }
    } else {
        out.line("\nNo config found. Run: lope configure");
    }
    match RunRegistry::open_default() {
        Ok(reg) => {
            let jobs = reg.list_active(true);
            let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
            for j in &jobs {
                *counts.entry(get_str(j, "classification", "ownership_unverified")).or_default() += 1;
            }
            out.line("\nJobs:");
            if jobs.is_empty() {
                out.line("  0 active or stale Lope-owned runs");
            } else {
                let summary: Vec<String> = counts.iter().map(|(k, v)| format!("{k}={v}")).collect();
                out.line(format!("  {} manifest(s): {}", jobs.len(), summary.join(", ")));
                for job in &jobs {
                    let run_id = get_str(job, "run_id", "?");
                    let res = job
                        .get("resources")
                        .filter(|r| r.is_object())
                        .cloned()
                        .unwrap_or(Value::Object(Default::default()));
                    let deadline_text = match job.get("deadline_remaining_seconds") {
                        None | Some(Value::Null) => "none".to_string(),
                        Some(d) => format!("{:.0}s", d.as_f64().unwrap_or(0.0)),
                    };
                    let class = get_str(job, "classification", "?");
                    out.line(format!(
                        "  {run_id}  {class}  deadline={deadline_text}  procs={}  cpu={}%  rss={}B",
                        get_str(&res, "process_count", "?"),
                        get_str(&res, "cpu_percent", "?"),
                        get_str(&res, "rss_bytes", "?")
                    ));
                    match job.get("classification").and_then(Value::as_str) {
                        Some("active" | "unresponsive") => {
                            out.line(format!("    cancel safely: lope jobs kill {run_id}"))
                        }
                        Some("abandoned") => out.line("    reconcile safely: lope jobs reap --dry-run"),
                        Some("cleanup_failed") => {
                            out.line(format!("    inspect cleanup failure: lope jobs list  # {run_id}"))
                        }
                        _ => {}
                    }
                }
            }
        }
        Err(e) => out.line(format!(
            "\nJobs: registry unavailable ({})",
            lope_state::registry::py_os_error(&e, &lope_core::paths::lope_home().join("runs"))
        )),
    }
    out.blank();
    let g = logo::maybe_gimmick(0.15);
    if !g.is_empty() {
        out.line(g);
        out.blank();
    }
    0
}
