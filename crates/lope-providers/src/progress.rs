//! `progress.py`: stderr progress lines, owner heartbeats, runtime summary.

use crate::budget::RunBudget;
use lope_state::registry::RunRegistry;
use serde_json::Value;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const DEFAULT_HEARTBEAT_SECONDS: f64 = 15.0;

pub struct Progress {
    budget: Arc<RunBudget>,
    registry: Option<Arc<RunRegistry>>,
    run_id: String,
    interval: f64,
    emit: AtomicBool,
    stop: Arc<(Mutex<bool>, std::sync::Condvar)>,
    state: Mutex<(String, std::collections::BTreeSet<String>)>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Progress {
    pub fn new(budget: Arc<RunBudget>, registry: Option<Arc<RunRegistry>>) -> Arc<Self> {
        let interval = std::env::var("LOPE_HEARTBEAT_INTERVAL")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_HEARTBEAT_SECONDS)
            .max(0.05);
        let emit = !matches!(
            std::env::var("LOPE_PROGRESS").unwrap_or_default().trim().to_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        );
        Arc::new(Progress {
            run_id: budget.run_id.clone(),
            budget,
            registry,
            interval,
            emit: AtomicBool::new(emit),
            stop: Arc::new((Mutex::new(false), std::sync::Condvar::new())),
            state: Mutex::new(("provider".into(), Default::default())),
            thread: Mutex::new(None),
        })
    }

    pub fn start(self: &Arc<Self>) {
        self.heartbeat_owner();
        let me = Arc::clone(self);
        let h = std::thread::spawn(move || loop {
            let (lock, cv) = &*me.stop;
            let g = lock.lock().unwrap();
            let (g, _) = cv.wait_timeout(g, std::time::Duration::from_secs_f64(me.interval)).unwrap();
            if *g {
                return;
            }
            drop(g);
            me.heartbeat();
        });
        *self.thread.lock().unwrap() = Some(h);
    }

    pub fn stop(&self) {
        let (lock, cv) = &*self.stop;
        *lock.lock().unwrap() = true;
        cv.notify_all();
        if let Some(h) = self.thread.lock().unwrap().take() {
            let _ = h.join();
        }
    }

    pub fn call_started(&self, validator: &str, stage: &str) {
        let mut s = self.state.lock().unwrap();
        s.1.insert(validator.into());
        if !stage.is_empty() {
            s.0 = stage.into();
        }
    }

    pub fn call_finished(&self, validator: &str, outcome: &str) {
        let stage = {
            let mut s = self.state.lock().unwrap();
            s.1.remove(validator);
            s.0.clone()
        };
        if self.emit.load(Ordering::SeqCst) {
            let snap = self.budget.snapshot();
            let actual = &snap["actual"];
            let fc = snap["forecast"]["calls"].as_i64().unwrap_or(0);
            let forecast = if fc != 0 { fc } else { actual["calls"].as_i64().unwrap_or(0) };
            self.write(&format!(
                "progress: {stage} · {validator} {} · {}/{forecast} calls · {:.1}s elapsed",
                if outcome.is_empty() { "finished" } else { outcome },
                actual["completed_calls"],
                snap["timing"]["elapsed_seconds"].as_f64().unwrap_or(0.0)
            ));
        }
    }

    pub fn retry(&self, validator: &str, delay: f64, reason: &str) {
        if self.emit.load(Ordering::SeqCst) {
            self.write(&format!("progress: retry {validator} in {delay:.2}s · {reason}"));
        }
    }

    fn heartbeat(&self) {
        self.heartbeat_owner();
        if !self.emit.load(Ordering::SeqCst) {
            return;
        }
        let snap = self.budget.snapshot();
        let actual = &snap["actual"];
        let fc = snap["forecast"]["calls"].as_i64().unwrap_or(0);
        let forecast = if fc != 0 { fc } else { actual["calls"].as_i64().unwrap_or(0) };
        let remaining = match &snap["timing"]["remaining_seconds"] {
            Value::Number(n) => format!("{:.1}s", n.as_f64().unwrap_or(0.0)),
            _ => "unbounded".into(),
        };
        let (stage, active) = {
            let s = self.state.lock().unwrap();
            (
                s.0.clone(),
                if s.1.is_empty() { "none".to_string() } else { s.1.iter().cloned().collect::<Vec<_>>().join(",") },
            )
        };
        self.write(&format!(
            "heartbeat: {stage} · {:.1}s elapsed · {remaining} remaining · {}/{forecast} calls · active={active}",
            snap["timing"]["elapsed_seconds"].as_f64().unwrap_or(0.0),
            actual["completed_calls"]
        ));
    }

    fn write(&self, msg: &str) {
        let mut e = std::io::stderr().lock();
        if writeln!(e, "{msg}").and_then(|_| e.flush()).is_err() {
            self.emit.store(false, Ordering::SeqCst);
        }
    }

    fn heartbeat_owner(&self) {
        if let Some(r) = &self.registry {
            let _ = r.heartbeat(&self.run_id, "owner");
        }
    }
}

/// Python `f"{x:g}"`.
pub fn g(x: f64) -> String {
    lope_runtime::supervisor::py_g(x)
}

/// `format_runtime_summary(snapshot)`.
pub fn format_runtime_summary(s: &Value) -> String {
    let actual = &s["actual"];
    let fc = s["forecast"]["calls"].as_i64().unwrap_or(0);
    let calls_forecast = if fc != 0 { fc } else { actual["calls"].as_i64().unwrap_or(0) };
    let wall_forecast = s["forecast"]["wall_seconds"].as_f64().unwrap_or(0.0);
    let wall = actual["wall_seconds"].as_f64().unwrap_or(0.0);
    let mut line =
        format!("Runtime: calls {}/{calls_forecast} · wall {wall:.1}s", actual["calls"].as_i64().unwrap_or(0));
    if wall_forecast != 0.0 {
        line.push_str(&format!("/{}s nominal", g(wall_forecast)));
    }
    line.push_str(&format!(
        " · input {}B · output {}B",
        actual["input_bytes"].as_i64().unwrap_or(0),
        actual["output_bytes"].as_i64().unwrap_or(0)
    ));
    if let Some(a) = s["artifacts"].as_array().filter(|a| !a.is_empty()) {
        line.push_str(&format!(
            " · artifacts {}",
            a.iter().map(|i| lope_core::config::py_str(&i["path"])).collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some(o) = s["circuits"]["open"].as_object().filter(|o| !o.is_empty()) {
        let mut k: Vec<&String> = o.keys().collect();
        k.sort();
        line.push_str(&format!(" · circuits open={}", k.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")));
    }
    if let Some(per) = actual["per_validator"].as_object().filter(|o| !o.is_empty()) {
        let mut names: Vec<&String> = per.keys().collect();
        names.sort();
        line.push_str(&format!(
            "\n  Latency: {}",
            names
                .iter()
                .map(|n| format!("{n}={:.1}s", per[*n]["latency_seconds"].as_f64().unwrap_or(0.0)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let reasons: Vec<String> = s["calls"]
        .as_array()
        .map(|cs| {
            cs.iter()
                .filter(|c| {
                    !matches!(c["outcome"].as_str(), None | Some("") | Some("ok"))
                        && c["reason"].as_str().is_some_and(|r| !r.is_empty())
                })
                .map(|c| {
                    format!(
                        "{}:{}",
                        lope_core::config::py_str(&c["validator"]),
                        lope_core::config::py_str(&c["reason"])
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if !reasons.is_empty() {
        line.push_str(&format!("\n  Reasons: {}", reasons.join("; ")));
    }
    line
}
