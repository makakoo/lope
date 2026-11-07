//! `runtime.py`: command-wide budget, call leases, invocation context.

use crate::progress::Progress;
use lope_state::registry::RunRegistry;
use serde_json::{json, Map, Value};
use std::sync::{Arc, Condvar, Mutex};

pub const DEFAULT_CLEANUP_RESERVE_SECONDS: f64 = 5.0;

pub fn mode_run_timeout(mode: &str) -> f64 {
    match mode {
        "negotiate" | "deliberate" => 3600.0,
        "execute" | "implement" | "flow" => 7200.0,
        _ => 1800.0,
    }
}

/// `time.monotonic()`.
pub fn monotonic() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9
}

fn f(x: f64) -> Value {
    serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputProfile {
    pub utf8_bytes: usize,
    pub lines: usize,
    pub estimated_tokens: usize,
}

impl InputProfile {
    pub fn from_text(t: &str) -> Self {
        let b = t.len();
        InputProfile {
            utf8_bytes: b,
            lines: t.matches('\n').count() + usize::from(!t.is_empty()),
            estimated_tokens: b.div_ceil(3),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CallRecord {
    pub run_id: String,
    pub call_id: String,
    pub mode: String,
    pub stage: String,
    pub validator: String,
    pub queued_at: f64,
    pub started_at: f64,
    pub ended_at: f64,
    pub prompt_bytes: usize,
    pub prompt_lines: usize,
    pub estimated_tokens: usize,
    pub requested_timeout: f64,
    pub effective_timeout: f64,
    pub transport: String,
    pub outcome: String,
    pub output_bytes: usize,
    pub cleanup_result: String,
    pub reason: String,
}

impl CallRecord {
    pub fn duration(&self) -> f64 {
        let end = if self.ended_at != 0.0 { self.ended_at } else { self.started_at };
        (end - self.started_at).max(0.0)
    }
    pub fn to_json(&self) -> Value {
        json!({
            "run_id": self.run_id, "call_id": self.call_id, "mode": self.mode, "stage": self.stage,
            "validator": self.validator, "queued_at": f(self.queued_at), "started_at": f(self.started_at),
            "ended_at": f(self.ended_at), "prompt_bytes": self.prompt_bytes, "prompt_lines": self.prompt_lines,
            "estimated_tokens": self.estimated_tokens, "requested_timeout": f(self.requested_timeout),
            "effective_timeout": f(self.effective_timeout), "transport": self.transport, "outcome": self.outcome,
            "output_bytes": self.output_bytes, "cleanup_result": self.cleanup_result, "reason": self.reason,
            "duration_seconds": f(self.duration()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Exhausted {
    pub reason: String,
    pub message: String,
}

#[derive(Default)]
struct Inner {
    calls_reserved: usize,
    input_reserved: usize,
    output_reserved: usize,
    output_actual: usize,
    records: Vec<CallRecord>,
    partial_reason: String,
    request_plans: Vec<Value>,
    artifacts: Vec<Value>,
    events: Vec<Value>,
    circuit_failures: Vec<(String, i64)>,
    circuit_open: Vec<(String, String)>,
    cancelled: Option<String>,
}

pub struct RunBudget {
    pub mode: String,
    pub run_id: String,
    pub started_at: f64,
    pub deadline: Option<f64>,
    pub cleanup_reserve: f64,
    pub max_external_calls: usize,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    inner: Mutex<Inner>,
    cancel_cv: Condvar,
}

impl RunBudget {
    pub fn new(
        mode: &str,
        run_timeout: Option<f64>,
        allow_unbounded: bool,
        max_calls: usize,
        max_in: usize,
        max_out: usize,
    ) -> Self {
        let rt = match run_timeout {
            Some(t) => Some(t),
            None if !allow_unbounded => Some(mode_run_timeout(mode)),
            None => None,
        };
        let now = monotonic();
        RunBudget {
            mode: mode.into(),
            run_id: uuid::Uuid::new_v4().simple().to_string(),
            started_at: now,
            deadline: rt.map(|t| now + t),
            cleanup_reserve: DEFAULT_CLEANUP_RESERVE_SECONDS,
            max_external_calls: max_calls,
            max_input_bytes: max_in,
            max_output_bytes: max_out,
            inner: Mutex::new(Inner::default()),
            cancel_cv: Condvar::new(),
        }
    }

    pub fn elapsed(&self) -> f64 {
        (monotonic() - self.started_at).max(0.0)
    }

    pub fn remaining(&self, include_cleanup: bool) -> f64 {
        match self.deadline {
            None => f64::INFINITY,
            Some(d) => (d - monotonic() - if include_cleanup { 0.0 } else { self.cleanup_reserve }).max(0.0),
        }
    }

    pub fn cancel(&self, reason: &str) {
        let mut g = self.inner.lock().unwrap();
        if g.cancelled.is_none() {
            g.cancelled = Some(reason.into());
            self.cancel_cv.notify_all();
        }
    }

    pub fn cancelled(&self) -> Option<String> {
        self.inner.lock().unwrap().cancelled.clone()
    }

    /// `cancellation.wait(seconds)`: true if cancelled within the window.
    pub fn wait_cancel(&self, seconds: f64) -> bool {
        let g = self.inner.lock().unwrap();
        if g.cancelled.is_some() {
            return true;
        }
        let (g, _) = self.cancel_cv.wait_timeout(g, std::time::Duration::from_secs_f64(seconds.max(0.0))).unwrap();
        g.cancelled.is_some()
    }

    fn effective_timeout(&self, requested: Option<f64>) -> Result<f64, Exhausted> {
        let mut v = self.remaining(false);
        if let Some(r) = requested {
            v = v.min(r);
        }
        if v <= 0.0 {
            return Err(Exhausted {
                reason: "run_budget_exhausted".into(),
                message: "run deadline leaves no time for another external call".into(),
            });
        }
        Ok(v)
    }

    pub fn reserve_call(
        &self,
        stage: &str,
        validator: &str,
        prompt: &str,
        requested: Option<f64>,
        transport: &str,
    ) -> Result<CallRecord, Exhausted> {
        let prof = InputProfile::from_text(prompt);
        let out_limit = 2 * 1024 * 1024;
        let now = monotonic();
        let mut g = self.inner.lock().unwrap();
        if let Some((_, r)) = g.circuit_open.iter().find(|(v, _)| v == validator) {
            return Err(Exhausted {
                reason: "circuit_open".into(),
                message: format!("validator {validator} circuit open: {r}"),
            });
        }
        if let Some(r) = &g.cancelled {
            return Err(Exhausted {
                reason: "cancelled".into(),
                message: if r.is_empty() { "run cancelled".into() } else { r.clone() },
            });
        }
        let eff = self.effective_timeout(requested)?;
        if g.calls_reserved + 1 > self.max_external_calls {
            return Err(Exhausted {
                reason: "run_budget_exhausted".into(),
                message: format!("external-call limit {} exhausted", self.max_external_calls),
            });
        }
        if g.input_reserved + prof.utf8_bytes > self.max_input_bytes {
            return Err(Exhausted {
                reason: "input_limit".into(),
                message: format!("run input limit {} bytes exceeded", self.max_input_bytes),
            });
        }
        if g.output_reserved + out_limit > self.max_output_bytes {
            return Err(Exhausted {
                reason: "output_limit".into(),
                message: format!("run output reservation limit {} bytes exceeded", self.max_output_bytes),
            });
        }
        g.calls_reserved += 1;
        g.input_reserved += prof.utf8_bytes;
        g.output_reserved += out_limit;
        let rec = CallRecord {
            run_id: self.run_id.clone(),
            call_id: uuid::Uuid::new_v4().simple().to_string(),
            mode: self.mode.clone(),
            stage: stage.into(),
            validator: validator.into(),
            queued_at: now,
            started_at: now,
            ended_at: 0.0,
            prompt_bytes: prof.utf8_bytes,
            prompt_lines: prof.lines,
            estimated_tokens: prof.estimated_tokens,
            requested_timeout: requested.filter(|r| *r != 0.0).unwrap_or(eff),
            effective_timeout: eff,
            transport: transport.into(),
            outcome: String::new(),
            output_bytes: 0,
            cleanup_result: "pending".into(),
            reason: String::new(),
        };
        g.records.push(rec.clone());
        Ok(rec)
    }

    /// `CallLease.finish` → `_finish_call`; returns the updated record.
    pub fn finish_call(
        &self,
        call_id: &str,
        outcome: &str,
        output_bytes: usize,
        cleanup: &str,
        reason: &str,
    ) -> CallRecord {
        let reserved = 2 * 1024 * 1024;
        let rec = {
            let mut g = self.inner.lock().unwrap();
            g.output_reserved = g.output_reserved.saturating_sub(reserved);
            g.output_actual += output_bytes;
            let over = output_bytes > reserved || g.output_actual > self.max_output_bytes;
            let r = g.records.iter_mut().find(|r| r.call_id == call_id).expect("lease record");
            r.ended_at = monotonic();
            r.outcome = outcome.into();
            r.output_bytes = output_bytes;
            r.cleanup_result = cleanup.into();
            r.reason = reason.into();
            if over {
                r.outcome = "output_limit".into();
                if r.reason.is_empty() {
                    r.reason = "actual output exceeded reserved run allowance".into();
                }
            }
            let rec = r.clone();
            Self::circuit_locked(&mut g, &rec.validator, &rec.outcome, &rec.reason, self.elapsed());
            rec
        };
        crate::latency::record(&rec.validator, rec.duration(), &rec.outcome);
        rec
    }

    fn circuit_locked(g: &mut Inner, validator: &str, outcome: &str, reason: &str, elapsed: f64) {
        if validator.is_empty() {
            return;
        }
        if outcome == "ok" {
            g.circuit_failures.retain(|(v, _)| v != validator);
            return;
        }
        if !matches!(outcome, "provider_timeout" | "rate_limited") {
            return;
        }
        let n = match g.circuit_failures.iter_mut().find(|(v, _)| v == validator) {
            Some((_, n)) => {
                *n += 1;
                *n
            }
            None => {
                g.circuit_failures.push((validator.into(), 1));
                1
            }
        };
        if n >= 2 && !g.circuit_open.iter().any(|(v, _)| v == validator) {
            let r = format!(
                "{n} consecutive {outcome} failures{}",
                if reason.is_empty() { String::new() } else { format!(": {}", crate::pyre::head(reason, 160)) }
            );
            g.circuit_open.push((validator.into(), r.clone()));
            g.events.push(
                json!({"kind": "circuit_opened", "elapsed_seconds": f(elapsed), "validator": validator, "reason": r}),
            );
        }
    }

    pub fn mark_partial(&self, reason: &str) {
        let mut g = self.inner.lock().unwrap();
        if g.partial_reason.is_empty() {
            g.partial_reason = reason.into();
        }
    }

    pub fn add_event(&self, kind: &str, fields: Vec<(&str, Value)>) {
        let mut ev = Map::new();
        ev.insert("kind".into(), json!(kind));
        ev.insert("elapsed_seconds".into(), f(self.elapsed()));
        for (k, v) in fields {
            if !v.is_null() {
                ev.insert(k.into(), v);
            }
        }
        self.inner.lock().unwrap().events.push(Value::Object(ev));
    }

    pub fn can_fund(&self, seconds: f64, calls: usize, input_bytes: usize) -> bool {
        let g = self.inner.lock().unwrap();
        self.remaining(false) >= seconds.max(0.0)
            && g.calls_reserved + calls <= self.max_external_calls
            && g.input_reserved + input_bytes <= self.max_input_bytes
            && g.cancelled.is_none()
    }

    pub fn add_request_plan(&self, plan: Value) {
        self.inner.lock().unwrap().request_plans.push(plan);
    }

    pub fn records(&self) -> Vec<CallRecord> {
        self.inner.lock().unwrap().records.clone()
    }

    pub fn snapshot(&self) -> Value {
        let g = self.inner.lock().unwrap();
        let reason = g.cancelled.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| g.partial_reason.clone());
        let partial =
            !g.partial_reason.is_empty() || g.records.iter().any(|r| !r.outcome.is_empty() && r.outcome != "ok");
        let fc = g
            .request_plans
            .iter()
            .map(|p| p.get("planned_calls").and_then(Value::as_i64).unwrap_or(0))
            .max()
            .unwrap_or(0)
            .max(0);
        let fw = g
            .request_plans
            .iter()
            .map(|p| p.get("nominal_wall_ceiling_seconds").and_then(Value::as_f64).unwrap_or(0.0))
            .fold(0.0f64, f64::max);
        let mut per: Map<String, Value> = Map::new();
        for r in &g.records {
            let row = per.entry(r.validator.clone()).or_insert_with(|| {
                json!({"calls": 0, "completed": 0, "latency_seconds": 0.0, "prompt_bytes": 0, "output_bytes": 0, "outcomes": {}})
            });
            row["calls"] = json!(row["calls"].as_i64().unwrap() + 1);
            row["completed"] = json!(row["completed"].as_i64().unwrap() + i64::from(r.ended_at > 0.0));
            row["latency_seconds"] = f(row["latency_seconds"].as_f64().unwrap() + r.duration());
            row["prompt_bytes"] = json!(row["prompt_bytes"].as_i64().unwrap() + r.prompt_bytes as i64);
            row["output_bytes"] = json!(row["output_bytes"].as_i64().unwrap() + r.output_bytes as i64);
            if !r.outcome.is_empty() {
                let o = row["outcomes"].as_object_mut().unwrap();
                let n = o.get(&r.outcome).and_then(Value::as_i64).unwrap_or(0) + 1;
                o.insert(r.outcome.clone(), json!(n));
            }
        }
        let completed = g.records.iter().filter(|r| r.ended_at > 0.0).count();
        let remaining = self.remaining(false);
        json!({
            "schema_version": 1,
            "run_id": self.run_id,
            "plan": {"mode": self.mode, "requests": g.request_plans},
            "timing": {
                "elapsed_seconds": f(self.elapsed()),
                "remaining_seconds": if remaining.is_infinite() { json!("Infinity") } else { f(remaining) },
                "deadline_monotonic": self.deadline.map(f).unwrap_or(Value::Null),
            },
            "limits": {
                "max_external_calls": self.max_external_calls, "calls_reserved": g.calls_reserved,
                "max_input_bytes": self.max_input_bytes, "input_bytes": g.input_reserved,
                "max_output_bytes": self.max_output_bytes, "output_bytes": g.output_actual,
            },
            "partial": partial,
            "reason": if reason.is_empty() { Value::Null } else { json!(reason) },
            "artifacts": g.artifacts,
            "forecast": {"calls": fc, "wall_seconds": f(fw)},
            "actual": {
                "calls": g.calls_reserved, "completed_calls": completed, "wall_seconds": f(self.elapsed()),
                "input_bytes": g.input_reserved, "output_bytes": g.output_actual, "per_validator": per,
            },
            "circuits": {
                "failures": g.circuit_failures.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<_, _>>(),
                "open": g.circuit_open.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<Map<_, _>>(),
            },
            "events": g.events,
            "calls": g.records.iter().map(CallRecord::to_json).collect::<Vec<_>>(),
        })
    }
}

/// `InvocationContext`: shared per command; `child()` clones are cheap.
#[derive(Clone)]
pub struct Context {
    pub budget: Arc<RunBudget>,
    pub mode: String,
    pub stage: String,
    pub validator: String,
    pub registry: Option<Arc<RunRegistry>>,
    pub progress: Option<Arc<Progress>>,
    pub call_id: Option<String>,
    pub implementation: bool,
    pub cleanup_result: Arc<Mutex<Option<String>>>,
    pub owned_paths: Arc<Mutex<Vec<String>>>,
}

impl Context {
    pub fn run_id(&self) -> &str {
        &self.budget.run_id
    }

    pub fn child(&self, stage: Option<&str>, validator: Option<&str>) -> Context {
        let mut c = self.clone();
        if let Some(s) = stage {
            c.stage = s.into();
        }
        if let Some(v) = validator {
            c.validator = v.into();
        }
        c
    }

    /// Per-call context: fresh cleanup-result slot and owned-path list.
    pub fn for_call(&self, stage: Option<&str>, validator: &str, call_id: &str) -> Context {
        let mut c = self.child(stage, Some(validator));
        c.call_id = Some(call_id.into());
        c.cleanup_result = Arc::new(Mutex::new(None));
        c.owned_paths = Arc::new(Mutex::new(Vec::new()));
        c
    }

    pub fn register_lease(&self, rec: &CallRecord, transport: &str) {
        if let Some(reg) = &self.registry {
            let now = lope_core::time::now();
            let call = json!({
                "call_id": rec.call_id, "validator": rec.validator, "stage": rec.stage, "state": "active",
                "started_at": f(now), "heartbeat_at": f(now), "deadline_at": f(now + rec.effective_timeout),
                "transport": transport, "owned_paths": [], "cleanup_result": "pending",
            });
            let _ = reg.register_call(self.run_id(), call.as_object().unwrap().clone());
        }
        if let Some(p) = &self.progress {
            p.call_started(&rec.validator, &rec.stage);
        }
    }

    pub fn add_request_plan(&self, plan: Value) {
        let planned = plan.get("planned_calls").and_then(Value::as_i64).unwrap_or(0);
        self.budget.add_request_plan(plan);
        if let Some(reg) = &self.registry {
            let _ = reg.set_planned_calls(self.run_id(), planned);
        }
    }

    pub fn finish_lease(&self, rec: &CallRecord) {
        if let Some(reg) = &self.registry {
            let ch = json!({
                "state": "finished", "outcome": rec.outcome, "cleanup_result": rec.cleanup_result,
                "ended_at": f(lope_core::time::now()), "reason": rec.reason,
            });
            let _ = reg.update_call(self.run_id(), &rec.call_id, ch.as_object().unwrap().clone());
        }
        if let Some(p) = &self.progress {
            p.call_finished(&rec.validator, &rec.outcome);
        }
    }

    /// The runtime CallContext for one supervised subprocess call.
    pub fn call_context(&self) -> lope_runtime::call::CallContext {
        lope_runtime::call::CallContext {
            registry_root: self.registry.as_ref().map(|r| r.root.clone()),
            run_id: self.run_id().to_string(),
            call_id: self.call_id.clone().unwrap_or_default(),
            owned_paths: self.owned_paths.lock().unwrap().clone(),
            implementation: self.implementation,
            cleanup_result: None,
        }
    }
}
