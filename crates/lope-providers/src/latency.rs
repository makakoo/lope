//! `latency.py`: per-validator duration ledger at `$LOPE_HOME/latency.json`
//! and the pre-launch budget fit check. Never fails a run.

use lope_core::pyjson::{self, Opts};
use serde_json::{json, Value};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

pub const MAX_SAMPLES: usize = 50;
pub const MIN_SAMPLES_FOR_ADVICE: usize = 3;
pub const FIT_SAFETY_FACTOR: f64 = 1.5;

pub fn enabled() -> bool {
    !matches!(
        std::env::var("LOPE_LATENCY").unwrap_or_default().trim().to_lowercase().as_str(),
        "off" | "0" | "false" | "no"
    )
}

pub fn ledger_path() -> PathBuf {
    lope_core::paths::lope_home_get_default().join("latency.json")
}

fn load() -> Value {
    let empty = json!({"schema_version": 1, "validators": {}});
    let Ok(t) = std::fs::read_to_string(ledger_path()) else { return empty };
    match pyjson::loads(&t) {
        Ok(v) if v.get("validators").is_some_and(Value::is_object) => v,
        _ => empty,
    }
}

/// Python `round(x, 3)`.
pub fn round3(x: f64) -> f64 {
    format!("{x:.3}").parse().unwrap_or(x)
}

pub fn record(validator: &str, duration: f64, outcome: &str) {
    if !enabled() {
        return;
    }
    let name = validator.trim();
    let outcome = outcome.trim();
    if name.is_empty() || !matches!(outcome, "ok" | "provider_timeout") {
        return;
    }
    let d = round3(duration);
    if d <= 0.0 {
        return;
    }
    let _ = (|| -> std::io::Result<()> {
        let mut data = load();
        let vals = data.as_object_mut().unwrap().entry("validators").or_insert_with(|| json!({}));
        let row = vals.as_object_mut().unwrap().entry(name.to_string()).or_insert_with(|| json!({"samples": []}));
        if !row.is_object() {
            *row = json!({"samples": []});
        }
        let samples = row.get("samples").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut s = samples;
        s.push(json!({"d": d, "t": outcome == "provider_timeout"}));
        let start = s.len().saturating_sub(MAX_SAMPLES);
        row["samples"] = Value::Array(s[start..].to_vec());
        data["schema_version"] = json!(1);
        let path = ledger_path();
        let dir = path.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!(".latency-{}.tmp", &uuid::Uuid::new_v4().simple().to_string()[..8]));
        let mut fh = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
        fh.write_all(pyjson::dumps(&data, &Opts::compact_sorted()).as_bytes())?;
        fh.sync_all()?;
        std::fs::rename(&tmp, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    })();
}

pub struct Stats {
    pub count: usize,
    pub p50: f64,
    pub p90: f64,
}

pub fn stats(validator: &str) -> Option<Stats> {
    if !enabled() {
        return None;
    }
    let data = load();
    let raw = data.get("validators")?.get(validator)?.get("samples")?.as_array()?.clone();
    let mut ds: Vec<f64> = raw
        .iter()
        .filter_map(|s| s.as_object())
        .filter_map(|s| s.get("d").and_then(lope_core::config::py_float))
        .filter(|d| *d > 0.0)
        .collect();
    if ds.is_empty() {
        return None;
    }
    ds.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |f: f64| ds[((f * ds.len() as f64).ceil() as usize).clamp(1, ds.len()) - 1];
    Some(Stats { count: ds.len(), p50: pct(0.5), p90: pct(0.9) })
}

/// `fit_warnings(validators, call_timeout)`; `provider_timeouts` gives each
/// validator's configured `_timeout_override` (ints only, as in Python).
pub fn fit_warnings(validators: &[(String, Option<i64>)], call_timeout: Option<i64>) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(call) = call_timeout else { return lines };
    if !enabled() {
        return lines;
    }
    let respect = crate::generic::respect_provider_timeout();
    for (name, pt) in validators {
        if name.is_empty() {
            continue;
        }
        let enforced = match pt {
            None => call,
            Some(p) if respect => *p,
            Some(p) => (*p).min(call),
        };
        let clamped = pt.is_some_and(|p| p > enforced);
        let st = stats(name);
        let misfit = st
            .as_ref()
            .is_some_and(|s| s.count >= MIN_SAMPLES_FOR_ADVICE && (enforced as f64) < s.p90 * FIT_SAFETY_FACTOR);
        if !clamped && !misfit {
            continue;
        }
        let mut detail = vec![format!("budget advice: {name} ceiling {enforced}s")];
        if clamped {
            detail.push(format!(
                "clamps its configured {}s (pass --respect-provider-timeout to honour the config)",
                pt.unwrap()
            ));
        }
        if let (true, Some(s)) = (misfit, &st) {
            detail.push(format!(
                "is below observed p90 {:.0}s over {} call(s) — predicted timeout; raise --timeout to >= {}s",
                s.p90,
                s.count,
                (s.p90 * FIT_SAFETY_FACTOR) as i64 + 1
            ));
        }
        lines.push(format!("  {}", detail.join("; ")));
    }
    lines
}
