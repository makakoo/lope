//! `_fanout_generate`, `_render_fanout`, `_fanout_quorum`, `_maybe_synthesize`.

use super::scope::flag;
use clap::ArgMatches;
use lope_engine::findings::ConsensusFinding;
use lope_engine::synthesis::{build_synthesis_prompt, format_synthesis, run_synthesis, SynthesisResult};
use lope_providers::budget::Context;
use lope_providers::errors::ProviderError;
use lope_providers::generic::Validator;
use lope_providers::invocation::invoke_generate;
use lope_providers::request_plan::{plan_request_full, Limits};
use serde_json::{json, Value};
use std::collections::BTreeMap;
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
    fanout_generate_with_prompts(validators, parallel, prompt, None, timeout, ctx)
}

pub fn fanout_generate_with_prompts(
    validators: &[Arc<dyn Validator>],
    parallel: bool,
    prompt: &str,
    prompts_by_validator: Option<&BTreeMap<String, String>>,
    timeout: f64,
    ctx: &Context,
) -> Vec<Row> {
    let available: Vec<Arc<dyn Validator>> = validators
        .iter()
        .filter(|v| v.available())
        .filter(|v| prompts_by_validator.is_none_or(|m| m.contains_key(v.name())))
        .cloned()
        .collect();
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
            let selected_prompt = prompts_by_validator
                .and_then(|m| m.get(v.name()))
                .cloned()
                .unwrap_or_else(|| prompt.to_string());
            let (tx, c) = (tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let r = invoke_generate(v.as_ref(), &selected_prompt, timeout, Some(&c), Some(&c.stage.clone()));
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

pub fn payload(label: &str, rows: &[Row]) -> Value {
    use lope_core::redaction::redact_text;
    let v: Vec<Value> = rows
        .iter()
        .map(|(name, answer, err)| {
            let mut m = serde_json::Map::new();
            m.insert("validator".into(), name.clone().into());
            m.insert(label.into(), redact_text(answer).into());
            m.insert(
                "error".into(),
                err.as_ref()
                    .map(|e| Value::from(redact_text(&err_text(e))))
                    .unwrap_or(Value::Null),
            );
            if let Some(ProviderError::Unverified {
                cleanup,
                stdout_complete,
                stdout_truncated,
                returncode,
                stdout,
                ..
            }) = err
            {
                m.insert(
                    "completion".into(),
                    json!({
                        "status": "UNVERIFIED",
                        "cleanup": cleanup,
                        "stdout_complete": stdout_complete,
                        "stdout_truncated": stdout_truncated,
                        "returncode": returncode
                    }),
                );
                if std::env::var("LOPE_RECOVER_UNVERIFIED_OUTPUT").as_deref() == Ok("1") {
                    m.insert(
                        "unverified_output".into(),
                        redact_text(&lope_providers::diagnostics::redact_diagnostic(stdout)).into(),
                    );
                }
            }
            Value::Object(m)
        })
        .collect();
    Value::Array(v)
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

/// `_maybe_synthesize` + `_execute_bounded_synthesis` (`cli.py:3658-3757`).
#[allow(clippy::too_many_arguments)]
pub fn maybe_synthesize(
    m: &ArgMatches,
    cfg: &lope_core::config::LopeCfg,
    pool: &lope_providers::pool::Pool,
    ctx: &Context,
    rows: &[Row],
    task: &str,
    structured_findings: Option<&[ConsensusFinding]>,
    include_raw_in_structured: bool,
) -> Option<SynthesisResult> {
    if !flag(m, "--synth") {
        return None;
    }
    let primary_rows: Vec<(String, String, Option<String>)> = rows
        .iter()
        .map(|(n, a, e)| {
            let ans = if structured_findings.is_some() && !include_raw_in_structured {
                String::new()
            } else {
                a.clone()
            };
            (n.clone(), ans, e.as_ref().map(err_text))
        })
        .collect();
    let mut truncations = Vec::new();
    let prompt = build_synthesis_prompt(
        task,
        &primary_rows,
        structured_findings,
        flag(m, "--anonymous"),
        lope_engine::synthesis::DEFAULT_SOURCE_BYTE_LIMIT,
        lope_engine::synthesis::DEFAULT_TOTAL_PROMPT_BYTE_LIMIT,
        &mut truncations,
    );
    execute_bounded_synthesis(cfg, pool, ctx, &prompt, truncations)
}

pub fn execute_bounded_synthesis(
    cfg: &lope_core::config::LopeCfg,
    pool: &lope_providers::pool::Pool,
    ctx: &Context,
    prompt: &str,
    truncations: Vec<Value>,
) -> Option<SynthesisResult> {
    let primary = pool.primary_validator();
    let primary_slice: Vec<Arc<dyn Validator>> = primary.iter().cloned().collect();
    let lim = Limits {
        max_chunks: lope_core::config::LopeCfg::int_field(&cfg.max_chunks),
        max_calls: lope_core::config::LopeCfg::int_field(&cfg.max_calls),
        max_input_bytes: lope_core::config::LopeCfg::int_field(&cfg.max_input_bytes),
    };
    let timeout = cfg.timeout_secs() as f64;
    let plan = plan_request_full(
        prompt,
        "synthesis",
        &primary_slice,
        &cfg.request_policy,
        &lim,
        timeout,
        false,
        false,
        "bounded synthesis input",
        "markdown",
        0,
        0,
        None,
        None,
    );
    ctx.add_request_plan(plan.json.clone());
    if plan.action == "reject" {
        ctx.budget
            .mark_partial(&format!("budget_exhausted_optional: {}", plan.reason));
        return Some(SynthesisResult {
            ok: false,
            text: String::new(),
            primary: primary.as_ref().map(|p| p.name().to_string()).unwrap_or_default(),
            error: format!("input_limit: {}", plan.reason),
            truncations,
        });
    }
    if !ctx.budget.can_fund(timeout, 1, prompt.len()) {
        let reason = "budget_exhausted_optional: synthesis skipped; full call cannot fit";
        ctx.budget.mark_partial(reason);
        ctx.budget.add_event(
            "optional_stage_skipped",
            vec![("stage", json!("synthesis")), ("reason", json!(reason))],
        );
        return Some(SynthesisResult {
            ok: false,
            text: String::new(),
            primary: primary.as_ref().map(|p| p.name().to_string()).unwrap_or_default(),
            error: reason.into(),
            truncations,
        });
    }
    let child_ctx = ctx.child(Some("synthesis"), None);
    Some(run_synthesis(
        primary.as_deref(),
        prompt,
        timeout,
        truncations,
        Some(&child_ctx),
    ))
}

pub fn synth_json_value(synth: &SynthesisResult) -> Value {
    json!({
        "ok": synth.ok,
        "primary": synth.primary,
        "text": if synth.ok { format_synthesis(synth, true) } else { String::new() },
        "error": if !synth.ok { synth.error.clone() } else { String::new() },
        "truncations": synth.truncations,
    })
}
