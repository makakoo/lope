//! `_fanout_generate`, `_render_fanout`, `_fanout_quorum`.

use lope_providers::budget::Context;
use lope_providers::errors::ProviderError;
use lope_providers::generic::Validator;
use lope_providers::invocation::invoke_generate;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub type Row = (String, String, Option<ProviderError>);

pub fn fanout_generate(
    validators: &[Arc<dyn Validator>],
    parallel: bool,
    prompt: &str,
    timeout: f64,
    ctx: &Context,
) -> Vec<Row> {
    let available: Vec<Arc<dyn Validator>> = validators.iter().filter(|v| v.available()).cloned().collect();
    if available.is_empty() {
        return vec![];
    }
    let deadline = Instant::now() + Duration::from_secs_f64(ctx.budget.remaining(false).min(1e9));
    let (tx, rx) = channel::<(String, Result<String, ProviderError>)>();
    let mut pending = available.clone();
    let mut active: Vec<String> = vec![];
    let mut out = vec![];
    let max_workers = if parallel { available.len().min(5) } else { 1 };
    loop {
        while !pending.is_empty() && active.len() < max_workers && Instant::now() < deadline {
            let v = pending.remove(0);
            active.push(v.name().to_string());
            let (tx, c, p) = (tx.clone(), ctx.clone(), prompt.to_string());
            std::thread::spawn(move || {
                let r = invoke_generate(v.as_ref(), &p, timeout, Some(&c), Some(&c.stage.clone()));
                let _ = tx.send((v.name().to_string(), r));
            });
        }
        if pending.is_empty() && active.is_empty() {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match rx.recv_timeout(deadline - now) {
            Ok((name, r)) => {
                active.retain(|n| *n != name);
                match r {
                    Ok(a) => out.push((name, a, None)),
                    Err(e) => out.push((name, String::new(), Some(e))),
                }
            }
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    for v in pending {
        out.push((
            v.name().to_string(),
            String::new(),
            Some(ProviderError::Runtime("run_budget_exhausted: validator was never scheduled".into())),
        ));
    }
    let to = lope_providers::generic::num(timeout);
    for n in active {
        out.push((n.clone(), String::new(), Some(ProviderError::Runtime(format!("{n} fanout timed out after {to}s")))));
    }
    if ctx.budget.remaining(false) <= 0.0 {
        ctx.budget.mark_partial("run_budget_exhausted: fan-out reached the command deadline");
    }
    out
}

/// The text Python prints for an error row (`str(error)`; BudgetExhausted
/// rows are pre-formatted as "reason: message").
pub fn err_text(e: &ProviderError) -> String {
    match e {
        ProviderError::Budget { reason, message } => format!("{reason}: {message}"),
        other => other.to_string(),
    }
}

pub fn render(rows: &[Row], out: &mut super::Out) {
    use lope_core::redaction::redact_text;
    for (name, answer, err) in rows {
        out.line(format!("\n━━━ {name} ━━━"));
        match err {
            Some(e) => out.line(format!("[ERROR] {}", redact_text(&err_text(e)))),
            None if !answer.trim().is_empty() => out.line(redact_text(answer).trim_end()),
            None => out.line("[empty response]"),
        }
    }
    out.blank();
}

pub fn payload(label: &str, rows: &[Row]) -> serde_json::Value {
    use lope_core::redaction::redact_text;
    let v: Vec<serde_json::Value> = rows
        .iter()
        .map(|(name, answer, err)| {
            let mut m = serde_json::Map::new();
            m.insert("validator".into(), name.clone().into());
            m.insert(label.into(), redact_text(answer).into());
            m.insert("error".into(), err.as_ref().map(|e| serde_json::Value::from(redact_text(&err_text(e)))).unwrap_or(serde_json::Value::Null));
            if let Some(ProviderError::Unverified { cleanup, stdout_complete, stdout_truncated, returncode, stdout, .. }) = err {
                m.insert(
                    "completion".into(),
                    serde_json::json!({"status": "UNVERIFIED", "cleanup": cleanup, "stdout_complete": stdout_complete, "stdout_truncated": stdout_truncated, "returncode": returncode}),
                );
                if std::env::var("LOPE_RECOVER_UNVERIFIED_OUTPUT").as_deref() == Ok("1") {
                    m.insert("unverified_output".into(), redact_text(&lope_providers::diagnostics::redact_diagnostic(stdout)).into());
                }
            }
            serde_json::Value::Object(m)
        })
        .collect();
    serde_json::Value::Array(v)
}

/// `_fanout_quorum`.
pub fn quorum(rows: &[Row], expected: &[String]) -> (bool, String) {
    let mut names: Vec<&str> = expected.iter().map(String::as_str).collect();
    names.sort();
    names.dedup();
    let n = if expected.is_empty() {
        let mut g: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
        g.sort();
        g.dedup();
        g.len()
    } else {
        names.len()
    };
    let required = n / 2 + 1;
    let mut groups: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    groups.sort();
    groups.dedup();
    let usable = groups
        .iter()
        .filter(|g| {
            rows.iter().any(|(nm, a, e)| {
                nm == *g && e.is_none() && !a.trim().is_empty() && lope_providers::validity::substantive(a)
            })
        })
        .count();
    let ok = usable >= required;
    (
        ok,
        if ok {
            String::new()
        } else {
            format!("inconclusive: substantive-result quorum {usable}/{required} not met from {n} validators")
        },
    )
}
