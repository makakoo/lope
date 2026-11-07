//! `lope jobs [list|reap|prune|kill]` (`cli.py:_cmd_jobs`).

use super::Out;
use clap::ArgMatches;
use lope_core::output::{get_str, json_text, ljust};
use lope_state::registry::RunRegistry;
use serde_json::{json, Value};
use std::ffi::OsString;

fn flag(m: &ArgMatches, id: &str) -> bool {
    m.try_get_one::<bool>(id).ok().flatten().copied().unwrap_or(false)
}

pub fn run(sub: Option<&str>, m: &ArgMatches) -> i32 {
    let sub = sub.unwrap_or("list");
    let json_mode = flag(m, "--json");
    let reg = match RunRegistry::open_default() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("lope jobs: {e}");
            return 1;
        }
    };
    let mut out = Out::new();
    match sub {
        "reap" | "prune" => {
            let dry_run = flag(m, "--dry-run");
            let rows = match reg.reap_all(dry_run) {
                Ok(r) => r,
                Err(e) => {
                    drop(out);
                    eprintln!("lope_state.RegistryError: {e}");
                    return 1;
                }
            };
            let verb = if sub == "prune" { "prune" } else { "reap" };
            if json_mode {
                let payload = json!({"schema_version": 1, "dry_run": dry_run, "actions": rows, "count": rows.len()});
                out.line(json_text(&payload));
            } else if rows.is_empty() {
                out.line(format!("No active or stale Lope-owned jobs to {verb}."));
            } else {
                out.line(format!("Lope jobs {verb}{}", if dry_run { " (dry-run):" } else { ":" }));
                for row in &rows {
                    out.line(format!(
                        "  {}  {} {} {}",
                        get_str(row, "run_id", "?"),
                        ljust(row.get("classification").unwrap_or(&json!("?")), 20),
                        ljust(row.get("action").unwrap_or(&json!("?")), 16),
                        get_str(row, "reason", "")
                    ));
                }
            }
            let failed = rows
                .iter()
                .any(|r| matches!(r.get("action").and_then(Value::as_str), Some("refused" | "cleanup_failed")));
            if !dry_run && failed {
                return 1;
            }
            0
        }
        "kill" => {
            let run_id = m.get_one::<OsString>("run_id").map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let result = reg.kill_run(&run_id).unwrap_or_else(|e| {
                json!({"run_id": run_id, "classification": "ownership_unverified", "action": "refused", "reason": e.0, "calls": []})
            });
            if json_mode {
                out.line(json_text(&json!({"schema_version": 1, "result": result})));
            } else {
                out.line(format!(
                    "{}  {}  {}",
                    get_str(&result, "run_id", "?"),
                    get_str(&result, "action", "?"),
                    get_str(&result, "reason", "")
                ));
            }
            if matches!(result.get("action").and_then(Value::as_str), Some("killed" | "reaped")) {
                0
            } else {
                1
            }
        }
        _ => {
            let rows = reg.list_active(true);
            if json_mode {
                out.line(json_text(&json!({"schema_version": 1, "jobs": rows, "count": rows.len()})));
                return 0;
            }
            if rows.is_empty() {
                out.line("No active or stale Lope-owned jobs.");
                return 0;
            }
            out.line("Lope jobs:");
            for row in &rows {
                let owner = row.get("owner").filter(|o| o.is_object()).cloned().unwrap_or(json!({}));
                let res = row.get("resources").filter(|o| o.is_object()).cloned().unwrap_or(json!({}));
                out.line(format!(
                    "  {}  {} {} owner={} procs={} cpu={}% rss={}B",
                    get_str(row, "run_id", "?"),
                    ljust(row.get("mode").unwrap_or(&json!("?")), 12),
                    ljust(row.get("classification").unwrap_or(&json!("ownership_unverified")), 20),
                    get_str(&owner, "pid", "?"),
                    get_str(&res, "process_count", "?"),
                    get_str(&res, "cpu_percent", "?"),
                    get_str(&res, "rss_bytes", "?")
                ));
            }
            0
        }
    }
}
