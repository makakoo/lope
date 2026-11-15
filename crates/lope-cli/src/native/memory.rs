//! `lope memory {stats|search|file|hotspots|forget|gates}` (`cli.py:_cmd_memory`).

use super::{scope, Native, Out};
use clap::ArgMatches;
use lope_core::output::{json_text, ljust_str};
use lope_memory::open_memory;
use lope_state::registry::py_repr_str;
use serde_json::Value;

pub fn run(sub: Option<&str>, m: &ArgMatches) -> Native {
    let store = match open_memory() {
        Ok(Some(s)) => s,
        Ok(None) => {
            eprintln!(
                "Lope memory is disabled (LOPE_MEMORY=off). Unset the variable or set LOPE_MEMORY= to re-enable."
            );
            return Native::Exit(1);
        }
        Err(e) => return Native::Delegate(e),
    };

    let json_mode = scope::flag(m, "--json");
    let mut out = Out::new();
    match sub.unwrap_or("stats") {
        "search" => {
            let query = scope::os_str(m, "query").unwrap_or_default();
            let min_score = m
                .try_get_one::<f64>("--min-score")
                .ok()
                .flatten()
                .copied()
                .unwrap_or(0.0);
            let limit = m
                .try_get_one::<i64>("--limit")
                .ok()
                .flatten()
                .copied()
                .unwrap_or(20);
            let rows = match store.search_findings(&query, min_score, limit) {
                Ok(r) => r,
                Err(e) => return Native::Delegate(e),
            };
            if json_mode {
                let arr = Value::Array(rows.iter().map(|r| r.to_dict()).collect());
                out.line(json_text(&arr));
                return Native::Exit(0);
            }
            if rows.is_empty() {
                out.line(format!(
                    "No stored findings matching {}.",
                    py_repr_str(&query)
                ));
                return Native::Exit(0);
            }
            out.line(format!(
                "\nLope memory: {} match(es) for {}",
                rows.len(),
                py_repr_str(&query)
            ));
            out.line("-".repeat(60));
            for r in &rows {
                let mut location = r.file.clone().unwrap_or_default();
                if let Some(ln) = r.line {
                    location.push_str(&format!(":{ln}"));
                }
                out.line(format!(
                    "  [{}] {} score {:.2}  seen {}x  {location}",
                    ljust_str(&r.severity.to_ascii_uppercase(), 8),
                    ljust_str(&r.consensus_level, 11),
                    r.consensus_score,
                    r.seen_count
                ));
                out.line(format!(
                    "    {}",
                    lope_providers::pyre::head(&r.message, 120)
                ));
            }
            out.blank();
            Native::Exit(0)
        }
        "file" => {
            let path = scope::os_str(m, "path").unwrap_or_default();
            let limit = m
                .try_get_one::<i64>("--limit")
                .ok()
                .flatten()
                .copied()
                .unwrap_or(50);
            let rows = match store.findings_for_file(&path, limit) {
                Ok(r) => r,
                Err(e) => return Native::Delegate(e),
            };
            if json_mode {
                let arr = Value::Array(rows.iter().map(|r| r.to_dict()).collect());
                out.line(json_text(&arr));
                return Native::Exit(0);
            }
            if rows.is_empty() {
                out.line(format!("No stored findings for {path}."));
                return Native::Exit(0);
            }
            out.line(format!("\nLope memory for {path}: {} finding(s)", rows.len()));
            out.line("-".repeat(60));
            for r in &rows {
                let line = r.line.map(|ln| format!(":{ln}")).unwrap_or_default();
                let fstr = r.file.as_deref().unwrap_or("None");
                out.line(format!(
                    "  [{}] score {:.2}  seen {}x  {fstr}{line}",
                    ljust_str(&r.severity.to_ascii_uppercase(), 8),
                    r.consensus_score,
                    r.seen_count
                ));
                out.line(format!("    hash: {}  level: {}", r.hash, r.consensus_level));
                out.line(format!(
                    "    {}",
                    lope_providers::pyre::head(&r.message, 120)
                ));
            }
            out.blank();
            Native::Exit(0)
        }
        "hotspots" => {
            let days = m
                .try_get_one::<i64>("--days")
                .ok()
                .flatten()
                .copied()
                .unwrap_or(30);
            let limit = m
                .try_get_one::<i64>("--limit")
                .ok()
                .flatten()
                .copied()
                .unwrap_or(10);
            let rows = match store.hotspots(days, limit) {
                Ok(r) => r,
                Err(e) => return Native::Delegate(e),
            };
            if json_mode {
                out.line(json_text(&Value::Array(rows)));
                return Native::Exit(0);
            }
            if rows.is_empty() {
                out.line(format!("No hotspots in the last {days} day(s)."));
                return Native::Exit(0);
            }
            out.line(format!("\nLope hotspots (last {days} days)"));
            out.line("-".repeat(60));
            for row in &rows {
                let fc = row["finding_count"].as_i64().unwrap_or(0);
                let dc = row["detection_count"].as_i64().unwrap_or(0);
                let avg = row["avg_score"].as_f64().unwrap_or(0.0);
                let f = row["file"].as_str().unwrap_or("");
                out.line(format!(
                    "  {fc:>3} findings  {dc:>3} detections  avg {avg:.2}   {f}"
                ));
            }
            out.blank();
            Native::Exit(0)
        }
        "gates" => {
            let limit = m
                .try_get_one::<i64>("--limit")
                .ok()
                .flatten()
                .copied()
                .unwrap_or(20);
            let rows = match store.gate_sessions(limit) {
                Ok(r) => r,
                Err(e) => return Native::Delegate(e),
            };
            if json_mode {
                out.line(json_text(&Value::Array(rows)));
                return Native::Exit(0);
            }
            if rows.is_empty() {
                out.line("No stored gate sessions.");
                return Native::Exit(0);
            }
            out.line(format!("\nLope gate sessions: {}", rows.len()));
            out.line("-".repeat(60));
            for r in &rows {
                let status = if r["passed"].as_bool().unwrap_or(false) {
                    "PASS"
                } else {
                    "FAIL"
                };
                let id = r["id"].as_i64().unwrap_or(0);
                let mode = r["mode"].as_str().unwrap_or("");
                let gc = r["gate_count"].as_i64().unwrap_or(0);
                let fc = r["failed_count"].as_i64().unwrap_or(0);
                let ca = r["created_at"].as_str().unwrap_or("");
                out.line(format!(
                    "  #{id} {} {} gates={gc} failed={fc} {ca}",
                    ljust_str(status, 4),
                    ljust_str(mode, 6)
                ));
            }
            out.blank();
            Native::Exit(0)
        }
        "forget" => {
            let hash = scope::os_str(m, "--hash").filter(|s| !s.is_empty());
            let file = scope::os_str(m, "--file").filter(|s| !s.is_empty());
            if hash.is_none() && file.is_none() {
                eprintln!("memory forget: pass --hash or --file");
                return Native::Exit(2);
            }
            let removed = match store.forget(hash.as_deref(), file.as_deref()) {
                Ok(r) => r,
                Err(e) => return Native::Delegate(e),
            };
            let target = if let Some(h) = &hash {
                format!("hash={h}")
            } else {
                format!("file={}", file.unwrap())
            };
            out.line(format!("Removed {removed} finding(s) for {target}"));
            Native::Exit(0)
        }
        _ => {
            let payload = match store.stats() {
                Ok(p) => p,
                Err(e) => return Native::Delegate(e),
            };
            if json_mode {
                out.line(json_text(&payload));
                return Native::Exit(0);
            }
            out.line(format!(
                "\nLope memory at {}",
                payload["db_path"].as_str().unwrap_or("")
            ));
            out.line("-".repeat(40));
            out.line(format!(
                "  Findings (total)      {:>5}",
                payload["total_findings"].as_i64().unwrap_or(0)
            ));
            out.line(format!(
                "  Findings (recurring)  {:>5}",
                payload["recurring_findings"].as_i64().unwrap_or(0)
            ));
            out.line(format!(
                "  Findings (confirmed)  {:>5}",
                payload["confirmed_findings"].as_i64().unwrap_or(0)
            ));
            out.line(format!(
                "  Files flagged         {:>5}",
                payload["flagged_files"].as_i64().unwrap_or(0)
            ));
            out.line(format!(
                "  Sessions stored       {:>5}",
                payload["total_sessions"].as_i64().unwrap_or(0)
            ));
            if let Some(gs) = payload.get("total_gate_sessions").and_then(Value::as_i64) {
                out.line(format!("  Gate sessions         {gs:>5}"));
            }
            if let Some(last) = payload
                .get("last_session_at")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                out.line(format!("  Last session          {last}"));
            }
            out.blank();
            Native::Exit(0)
        }
    }
}
