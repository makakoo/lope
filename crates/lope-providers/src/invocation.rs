//! `invocation.py:invoke_generate`: budget lease, call, outcome, one typed
//! transient retry.

use crate::budget::Context;
use crate::errors::ProviderError;
use crate::generic::Validator;
use crate::retry::{classify, decide, ErrView};
use serde_json::json;

fn outcome_for(e: &ProviderError) -> &'static str {
    match classify(ErrView::from(e)).kind {
        "cleanup_failed" => "cleanup_failed",
        "rate_limited" => "rate_limited",
        "provider_timeout" => "provider_timeout",
        "output_limit" => "output_limit",
        "input_limit" => "input_limit",
        "parse_error" => "parse_error",
        "tool_permission_denied" => "tool_permission_denied",
        "auth_error" => "auth_error",
        "empty_output" => "empty_output",
        "service_unavailable" | "connection_reset" => "transient_failure",
        _ => "launch_error",
    }
}

pub fn invoke_generate(
    v: &dyn Validator,
    prompt: &str,
    timeout: f64,
    ctx: Option<&Context>,
    stage: Option<&str>,
) -> Result<String, ProviderError> {
    let Some(ctx) = ctx else { return v.generate(prompt, timeout, None) };
    let root = ctx.child(Some(stage.unwrap_or(&ctx.stage)), Some(v.name()));
    let mut attempt = 0;
    loop {
        if root.budget.remaining(false) < timeout {
            let msg = format!("remaining run budget cannot fund complete {}s call", crate::progress::g(timeout));
            root.budget.mark_partial(&format!("run_budget_exhausted: {msg}"));
            return Err(ProviderError::Budget { reason: "run_budget_exhausted".into(), message: msg });
        }
        let rec = match root.budget.reserve_call(&root.stage, v.name(), prompt, Some(timeout), "adapter") {
            Ok(r) => r,
            Err(x) => {
                if x.reason == "run_budget_exhausted" {
                    root.budget.mark_partial(&format!("{}: {}", x.reason, x.message));
                }
                return Err(ProviderError::Budget { reason: x.reason, message: x.message });
            }
        };
        root.register_lease(&rec, "adapter");
        let call = root.for_call(stage, v.name(), &rec.call_id);
        let res = v.generate(prompt, rec.effective_timeout, Some(&call));
        let cleanup_slot = call.cleanup_result.lock().unwrap().clone();
        match res {
            Ok(answer) => {
                let fin = root.budget.finish_call(
                    &rec.call_id,
                    "ok",
                    answer.len(),
                    cleanup_slot.as_deref().unwrap_or("clean"),
                    "",
                );
                root.finish_lease(&fin);
                return Ok(answer);
            }
            Err(e) => {
                let outcome = outcome_for(&e);
                let default_cleanup = if matches!(
                    outcome,
                    "provider_timeout" | "rate_limited" | "transient_failure" | "input_limit" | "output_limit"
                ) {
                    "clean"
                } else {
                    "unknown"
                };
                let msg = e.to_string();
                let fin = root.budget.finish_call(
                    &rec.call_id,
                    outcome,
                    0,
                    cleanup_slot.as_deref().unwrap_or(default_cleanup),
                    crate::pyre::head(&msg, 300),
                );
                root.finish_lease(&fin);
                let d = decide(
                    ErrView::from(&e),
                    attempt,
                    root.budget.remaining(false),
                    timeout,
                    &format!("{}:{}:{attempt}", root.run_id(), v.name()),
                    1,
                );
                root.budget.add_event(
                    if d.retry { "retry_scheduled" } else { "retry_skipped" },
                    vec![
                        ("validator", json!(v.name())),
                        ("stage", json!(root.stage)),
                        ("classification", json!(d.classification)),
                        ("reason", json!(d.reason)),
                        ("delay_seconds", json!(d.delay)),
                        ("attempt", json!(attempt + 1)),
                    ],
                );
                if !d.retry {
                    return Err(e);
                }
                if let Some(p) = &root.progress {
                    p.retry(v.name(), d.delay, d.classification);
                }
                if root.budget.wait_cancel(d.delay) {
                    let r = root.budget.cancelled().unwrap_or_default();
                    return Err(ProviderError::Budget {
                        reason: "cancelled".into(),
                        message: if r.is_empty() { "run cancelled during retry delay".into() } else { r },
                    });
                }
                attempt += 1;
            }
        }
    }
}
