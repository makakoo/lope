//! `lope review` (`cli.py:_cmd_review` and `cli.py:_cmd_review_consensus`).

use super::fanout::{self, Row};
use super::scope::{self, flag, os_str};
use super::{Native, Out};
use clap::ArgMatches;
use lope_core::config::LopeCfg;
use lope_core::pyjson::{self, Opts};
use lope_core::redaction::redact_text;
use lope_engine::divide::{
    assign_roles, build_role_prompt, pack_file_chunks, pack_hunk_chunks, parse_roles,
    split_diff_hunks, split_files, DEFAULT_MAX_CHARS, DEFAULT_MAX_FILE_BYTES,
};
use lope_engine::findings::{merge_findings, score_consensus};
use lope_engine::minimality::resolve_review_focus;
use lope_engine::review::{
    build_consensus_report, build_review_prompt, parse_responses, render_report, FanoutRow,
    RawReviewResult, ReviewInput, ReviewReport, DEFAULT_FOCUS, REVIEW_SCOPE,
};
use lope_engine::synthesis::{
    build_synthesis_prompt, format_synthesis, DEFAULT_SOURCE_BYTE_LIMIT,
    DEFAULT_TOTAL_PROMPT_BYTE_LIMIT,
};
use lope_memory::{open_memory, ConsensusFindingInput, FindingRecord};
use lope_providers::budget::Context;
use lope_providers::errors::ProviderError;
use lope_providers::pool::Pool;
use lope_providers::request_plan::{plan_request_full, Limits, Plan, RequestChunk, SemanticUnit};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub fn run(m: &ArgMatches) -> Native {
    let file_path = os_str(m, "file").unwrap_or_default();
    let divide_mode = m.try_get_one::<String>("--divide").ok().flatten().map(String::as_str);
    let roles_spec = os_str(m, "--roles").filter(|s| !s.is_empty());
    let raw_focus = os_str(m, "--focus").unwrap_or_default();
    let focus_arg = resolve_review_focus(Some(&raw_focus));

    let content = if divide_mode == Some("files") {
        if !Path::new(&file_path).exists() {
            eprintln!("Path not found: {file_path}");
            return Native::Exit(1);
        }
        String::new()
    } else {
        let p = Path::new(&file_path);
        if !p.is_file() {
            eprintln!("File not found: {file_path}");
            return Native::Exit(1);
        }
        match fs::read(p) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                eprintln!("Cannot read {file_path}: {e}");
                return Native::Exit(1);
            }
        }
    };

    let output_format = m
        .try_get_one::<String>("--format")
        .ok()
        .flatten()
        .map(String::as_str)
        .unwrap_or("text");
    let structured_mode = flag(m, "--consensus")
        || flag(m, "--structured")
        || output_format != "text"
        || divide_mode.is_some()
        || roles_spec.is_some();

    if structured_mode {
        return run_consensus(
            m,
            &file_path,
            &content,
            &focus_arg,
            output_format,
            divide_mode,
            roles_spec.as_deref(),
        );
    }

    let focus = if focus_arg.trim().is_empty() {
        DEFAULT_FOCUS.to_string()
    } else {
        focus_arg.trim().to_string()
    };
    let prompt = format!(
        "{REVIEW_SCOPE}\n\n{focus}\n\nFile: {file_path}\n```\n{content}\n```\n\nReturn your review as plain prose. No VERDICT block needed."
    );

    let (cfg, pool) = match scope::resolve(m) {
        Ok(x) => x,
        Err(reason) => return Native::Delegate(reason),
    };
    for w in &pool.warnings {
        eprintln!("{w}");
    }
    let json_mode = flag(m, "--json");
    let scope = match scope::start("review", &cfg, json_mode) {
        Ok(s) => s,
        Err(e) => return Native::Delegate(format!("runtime start failed: {e}")),
    };
    let validator_names = pool.names();

    let plan = match plan_external_request(
        m,
        &cfg,
        &pool,
        &scope.ctx,
        &prompt,
        true,
        &file_path,
        "auto",
        2,
        None,
        None,
    ) {
        Ok(p) => p,
        Err(code) => return Native::Exit(scope::finish(scope, code)),
    };

    let mut out = Out::new();
    if !json_mode {
        let content_chars = content.chars().count();
        let focus_preview = lope_providers::pyre::head(&focus, 80);
        let dots = if focus.chars().count() > 80 { "..." } else { "" };
        out.line(format!("\nLope review: {file_path}  ({content_chars} chars)"));
        out.line(format!("Validators: {}", validator_names.join(", ")));
        out.line(format!("Focus: {focus_preview}{dots}"));
        out.line(format!(
            "Timeout: {}s per validator\n",
            lope_core::config::py_str(&cfg.timeout)
        ));
        out.flush();
    }

    let timeout = cfg.timeout_secs() as f64;
    if plan.action == "chunk" {
        let report = build_report_via_semantic_chunks(
            m,
            &plan.chunks,
            &file_path,
            &file_path,
            &validator_names,
            &pool,
            &cfg,
            &scope.ctx,
            &focus_arg,
            None,
        );
        let rows: Vec<Row> = report
            .raw_results
            .iter()
            .map(|r| {
                (
                    r.validator.clone(),
                    r.answer.clone(),
                    r.error.as_ref().map(|e| ProviderError::Runtime(e.clone())),
                )
            })
            .collect();
        let (quorum_ok, quorum_reason) = fanout::quorum(&rows, &validator_names);
        if !quorum_ok {
            scope.ctx.budget.mark_partial(&quorum_reason);
        }
        let synth_task = format!("Review of {file_path} — focus: {focus}");
        let synth = if quorum_ok {
            fanout::maybe_synthesize(
                m,
                &cfg,
                &pool,
                &scope.ctx,
                &rows,
                &synth_task,
                Some(&report.scored),
                flag(m, "--include-raw"),
            )
        } else {
            None
        };
        if json_mode {
            let mut payload = scope.ctx.budget.snapshot();
            payload["review"] = report.to_dict();
            if let Some(s) = &synth {
                payload["synthesis"] = fanout::synth_json_value(s);
            }
            if !quorum_ok {
                payload["partial"] = json!(true);
                payload["reason"] = json!(quorum_reason);
            }
            out.line(pyjson::dumps(&payload, &Opts::pretty_sorted()));
        } else {
            if let Ok(rendered) = render_report(&report, "text", false) {
                out.buf.extend_from_slice(rendered.as_bytes());
            }
            if let Some(s) = &synth {
                out.line(format_synthesis(s, false));
            }
        }
        out.flush();
        if !quorum_ok {
            if !json_mode {
                eprintln!("{quorum_reason}");
            }
            return Native::Exit(scope::finish(scope, 1));
        }
        return Native::Exit(scope::finish(scope, 0));
    }

    let rows = fanout::fanout_generate(&pool.validators, pool.parallel, &prompt, timeout, &scope.ctx);
    if rows.is_empty() {
        eprintln!("No validators available. Run: lope status");
        drop(out);
        return Native::Exit(scope::finish(scope, 1));
    }
    let (quorum_ok, quorum_reason) = fanout::quorum(&rows, &validator_names);
    if !quorum_ok {
        scope.ctx.budget.mark_partial(&quorum_reason);
    }
    let synth_task = format!("Review of {file_path} — focus: {focus}");
    let synth = if quorum_ok {
        fanout::maybe_synthesize(m, &cfg, &pool, &scope.ctx, &rows, &synth_task, None, false)
    } else {
        None
    };

    if json_mode {
        let mut payload = scope.ctx.budget.snapshot();
        payload["responses"] = fanout::payload("review", &rows);
        if let Some(s) = &synth {
            payload["synthesis"] = fanout::synth_json_value(s);
        }
        if !quorum_ok {
            payload["partial"] = json!(true);
            payload["reason"] = json!(quorum_reason);
        }
        out.line(pyjson::dumps(&payload, &Opts::pretty_sorted()));
    } else {
        fanout::render(&rows, &mut out);
        if let Some(s) = &synth {
            out.line(format_synthesis(s, false));
        }
    }
    out.flush();
    if !quorum_ok {
        if !json_mode {
            eprintln!("{quorum_reason}");
        }
        return Native::Exit(scope::finish(scope, 1));
    }
    Native::Exit(scope::finish(scope, 0))
}

fn run_consensus(
    m: &ArgMatches,
    file_path: &str,
    content: &str,
    focus_arg: &str,
    output_format: &str,
    divide_mode: Option<&str>,
    roles_spec: Option<&str>,
) -> Native {
    let (cfg, pool) = match scope::resolve(m) {
        Ok(x) => x,
        Err(reason) => return Native::Delegate(reason),
    };
    for w in &pool.warnings {
        eprintln!("{w}");
    }
    let json_flag = flag(m, "--json");
    let fmt = if output_format == "text" && json_flag {
        "json"
    } else {
        output_format
    };
    let is_machine = matches!(fmt, "json" | "sarif");
    let scope = match scope::start("review", &cfg, json_flag || is_machine) {
        Ok(s) => s,
        Err(e) => return Native::Delegate(format!("runtime start failed: {e}")),
    };
    let validator_names = pool.names();

    if divide_mode.is_some() && roles_spec.is_some() {
        eprintln!(
            "lope review: --divide and --roles cannot be combined; pick one (combination behavior is reserved for a future phase)."
        );
        return Native::Exit(scope::finish(scope, 2));
    }

    let mut out = Out::new();
    if !is_machine {
        let focus_preview = focus_arg.trim();
        let focus_label = if !focus_preview.is_empty() {
            let head = lope_providers::pyre::head(focus_preview, 80);
            let dots = if focus_preview.chars().count() > 80 { "..." } else { "" };
            format!("{head}{dots}")
        } else {
            "(default)".to_string()
        };
        let mode_label = if let Some(d) = divide_mode {
            format!("divide={d}")
        } else if let Some(r) = roles_spec {
            format!("roles={r}")
        } else {
            "single".to_string()
        };
        let val_str = if validator_names.is_empty() {
            "—".to_string()
        } else {
            validator_names.join(", ")
        };
        out.line(format!(
            "\nLope consensus review: {file_path}  ({} chars)",
            content.chars().count()
        ));
        out.line(format!("Validators: {val_str}"));
        out.line(format!("Focus: {focus_label}"));
        out.line(format!("Format: {fmt}  ·  Mode: {mode_label}"));
        out.line(format!(
            "Timeout: {}s per validator\n",
            lope_core::config::py_str(&cfg.timeout)
        ));
        out.flush();
    }

    let request_plan = if divide_mode.is_none() {
        match plan_external_request(
            m,
            &cfg,
            &pool,
            &scope.ctx,
            content,
            roles_spec.is_none(),
            file_path,
            "auto",
            2,
            None,
            None,
        ) {
            Ok(p) => Some(p),
            Err(code) => return Native::Exit(scope::finish(scope, code)),
        }
    } else {
        None
    };

    let report = if divide_mode == Some("files") {
        match build_report_via_divided_files(
            m,
            file_path,
            &validator_names,
            &pool,
            &cfg,
            &scope.ctx,
            focus_arg,
            &mut out,
        ) {
            Ok(r) => r,
            Err(code) => {
                drop(out);
                return Native::Exit(scope::finish(scope, code));
            }
        }
    } else if divide_mode == Some("hunks") {
        match build_report_via_divided_hunks(
            m,
            file_path,
            content,
            &validator_names,
            &pool,
            &cfg,
            &scope.ctx,
            focus_arg,
        ) {
            Ok(r) => r,
            Err(code) => {
                drop(out);
                return Native::Exit(scope::finish(scope, code));
            }
        }
    } else if let Some(roles) = roles_spec {
        match build_report_via_roles(
            m,
            file_path,
            content,
            &validator_names,
            &pool,
            &cfg,
            &scope.ctx,
            focus_arg,
            roles,
        ) {
            Ok(r) => r,
            Err(code) => {
                drop(out);
                return Native::Exit(scope::finish(scope, code));
            }
        }
    } else if let Some(plan) = &request_plan {
        if plan.action == "chunk" {
            build_report_via_semantic_chunks(
                m,
                &plan.chunks,
                file_path,
                file_path,
                &validator_names,
                &pool,
                &cfg,
                &scope.ctx,
                focus_arg,
                None,
            )
        } else {
            run_single_consensus_review(
                m,
                file_path,
                content,
                &validator_names,
                &pool,
                &cfg,
                &scope.ctx,
                focus_arg,
                None,
                None,
            )
        }
    } else {
        run_single_consensus_review(
            m,
            file_path,
            content,
            &validator_names,
            &pool,
            &cfg,
            &scope.ctx,
            focus_arg,
            None,
            None,
        )
    };

    if report.raw_results.is_empty() && report.errors.is_empty() {
        eprintln!("No validators available. Run: lope status");
        drop(out);
        return Native::Exit(scope::finish(scope, 1));
    }

    let consensus_rows: Vec<Row> = report
        .raw_results
        .iter()
        .map(|r| {
            (
                r.validator.clone(),
                r.answer.clone(),
                r.error.as_ref().map(|e| ProviderError::Runtime(e.clone())),
            )
        })
        .collect();
    let (quorum_ok, quorum_reason) = fanout::quorum(&consensus_rows, &validator_names);
    if !quorum_ok {
        scope.ctx.budget.mark_partial(&quorum_reason);
    }

    let mut memory_message: Option<String> = None;
    let mut memory_summary: Option<Value> = None;
    if flag(m, "--remember") && quorum_ok {
        match open_memory() {
            Ok(None) => {
                memory_message =
                    Some("Memory disabled (LOPE_MEMORY=off); --remember was a no-op.".to_string());
            }
            Ok(Some(store)) => {
                let inputs: Vec<ConsensusFindingInput> = report
                    .scored
                    .iter()
                    .map(|f| {
                        let mut evidence = serde_json::Map::new();
                        for (k, v) in &f.evidence {
                            evidence.insert(k.clone(), v.clone());
                        }
                        ConsensusFindingInput {
                            hash: f.hash().to_string(),
                            message: f.message.clone(),
                            file: f.file.clone(),
                            line: f.line.map(|x| x as i64),
                            end_line: f.end_line.map(|x| x as i64),
                            severity: f.severity.clone(),
                            category: f.category.clone(),
                            confidence_max: f.confidence_max,
                            consensus_score: f.consensus_score,
                            consensus_level: f.consensus_level.as_str().to_string(),
                            agreement_count: f.agreement_count as i64,
                            total_validators: f.total_validators as i64,
                            detected_by: f.detected_by.clone(),
                            evidence,
                        }
                    })
                    .collect();
                if let Ok((session_id, stored)) = store.store_review_session(
                    &format!("lope review {file_path}"),
                    focus_arg,
                    file_path,
                    &validator_names,
                    &inputs,
                    0,
                ) {
                    let recurring: Vec<&FindingRecord> =
                        stored.iter().filter(|r| r.seen_count > 1).collect();
                    let recurring_hashes: Vec<String> =
                        recurring.iter().map(|r| r.hash.clone()).collect();
                    let db_str = store.db_path.display().to_string();
                    memory_summary = Some(json!({
                        "session_id": session_id,
                        "stored_count": stored.len(),
                        "recurring_count": recurring.len(),
                        "recurring_hashes": recurring_hashes,
                        "db_path": db_str,
                    }));
                    memory_message = Some(format!(
                        "Memory: stored {} finding(s) ({} recurring) → {}",
                        stored.len(),
                        recurring.len(),
                        store.db_path.display()
                    ));
                }
            }
            Err(_) => {}
        }
    }

    let include_raw = flag(m, "--include-raw");
    let synth = if flag(m, "--synth") && quorum_ok {
        let responses_for_synth: Vec<(String, String, Option<String>)> = report
            .raw_results
            .iter()
            .map(|r| {
                let ans = if include_raw { r.answer.clone() } else { String::new() };
                (r.validator.clone(), ans, r.error.clone())
            })
            .collect();
        let synth_task = format!(
            "Consensus review of {file_path} — {} merged findings from {} validators.",
            report.merged_count,
            report.validators.len()
        );
        let mut truncations = Vec::new();
        let synth_prompt = build_synthesis_prompt(
            &synth_task,
            &responses_for_synth,
            Some(&report.scored),
            flag(m, "--anonymous"),
            DEFAULT_SOURCE_BYTE_LIMIT,
            DEFAULT_TOTAL_PROMPT_BYTE_LIMIT,
            &mut truncations,
        );
        fanout::execute_bounded_synthesis(&cfg, &pool, &scope.ctx, &synth_prompt, truncations)
    } else {
        None
    };

    let mut rendered = render_report(&report, fmt, include_raw).unwrap_or_default();

    if fmt == "sarif" {
        if let Ok(mut sarif_payload) = serde_json::from_str::<Value>(&rendered) {
            let runtime_payload = scope.ctx.budget.snapshot();
            if let Some(runs) = sarif_payload.get_mut("runs").and_then(Value::as_array_mut) {
                if let Some(run0) = runs.first_mut().and_then(Value::as_object_mut) {
                    let props = run0
                        .entry("properties")
                        .or_insert_with(|| json!({}));
                    if let Some(pm) = props.as_object_mut() {
                        pm.insert("lope_runtime".into(), runtime_payload);
                    }
                }
            }
            rendered = format!("{}\n", pyjson::dumps(&sarif_payload, &Opts::pretty_sorted()));
        }
        out.buf.extend_from_slice(rendered.as_bytes());
        if !rendered.ends_with('\n') {
            out.buf.push(b'\n');
        }
        out.flush();
        if let Some(s) = &synth {
            eprintln!("{}", format_synthesis(s, false));
        }
        if let Some(msg) = &memory_message {
            eprintln!("{msg}");
        }
        if !quorum_ok {
            eprintln!("{quorum_reason}");
            return Native::Exit(scope::finish(scope, 1));
        }
        return Native::Exit(scope::finish(scope, 0));
    }

    if fmt == "json" {
        let mut payload = serde_json::from_str::<Value>(&rendered)
            .unwrap_or_else(|_| json!({ "raw": rendered }));
        let runtime_payload = scope.ctx.budget.snapshot();
        if let (Some(pm), Some(rm)) = (payload.as_object_mut(), runtime_payload.as_object()) {
            for key in [
                "schema_version",
                "run_id",
                "plan",
                "timing",
                "limits",
                "actual",
                "events",
                "circuits",
                "partial",
                "reason",
                "artifacts",
                "calls",
            ] {
                if let Some(v) = rm.get(key) {
                    pm.insert(key.into(), v.clone());
                }
            }
            if let Some(s) = &synth {
                pm.insert("synthesis".into(), fanout::synth_json_value(s));
            }
            if let Some(ms) = memory_summary {
                pm.insert("memory".into(), ms);
            } else if let Some(msg) = &memory_message {
                pm.insert("memory".into(), json!({ "disabled": true, "note": msg }));
            }
            if !quorum_ok {
                pm.insert("schema_version".into(), json!(1));
                pm.insert("partial".into(), json!(true));
                pm.insert("reason".into(), json!(quorum_reason));
            }
        }
        let out_text = pyjson::dumps(&payload, &Opts::pretty_sorted());
        if let Some(out_p) = os_str(m, "--output").filter(|s| !s.is_empty()) {
            let _ = write_output_atomic(Path::new(&out_p), &format!("{out_text}\n"));
        }
        out.line(out_text);
        out.flush();
        if !quorum_ok {
            return Native::Exit(scope::finish(scope, 1));
        }
        return Native::Exit(scope::finish(scope, 0));
    }

    if let Some(out_p) = os_str(m, "--output").filter(|s| !s.is_empty()) {
        let body = if rendered.ends_with('\n') {
            rendered.clone()
        } else {
            format!("{rendered}\n")
        };
        let _ = write_output_atomic(Path::new(&out_p), &body);
    }

    out.buf.extend_from_slice(rendered.as_bytes());
    if !rendered.ends_with('\n') {
        out.buf.push(b'\n');
    }
    if let Some(s) = &synth {
        out.line(format_synthesis(s, false));
    }
    if let Some(msg) = &memory_message {
        out.line(msg);
    }
    out.flush();
    if !quorum_ok {
        eprintln!("{quorum_reason}");
        return Native::Exit(scope::finish(scope, 1));
    }
    Native::Exit(scope::finish(scope, 0))
}

#[allow(clippy::too_many_arguments)]
fn plan_external_request(
    m: &ArgMatches,
    cfg: &LopeCfg,
    pool: &Pool,
    ctx: &Context,
    prompt: &str,
    allow_chunk: bool,
    source_label: &str,
    kind: &str,
    overlap_lines: usize,
    units: Option<Vec<SemanticUnit>>,
    policy_override: Option<&str>,
) -> Result<Plan, i32> {
    let lim = Limits {
        max_chunks: LopeCfg::int_field(&cfg.max_chunks),
        max_calls: LopeCfg::int_field(&cfg.max_calls),
        max_input_bytes: LopeCfg::int_field(&cfg.max_input_bytes),
    };
    let timeout = cfg.timeout_secs() as f64;
    let extra_calls = usize::from(flag(m, "--synth"));
    let policy = policy_override.unwrap_or(&cfg.request_policy);
    let plan = plan_request_full(
        prompt,
        "review",
        &pool.validators,
        policy,
        &lim,
        timeout,
        pool.parallel,
        allow_chunk,
        source_label,
        kind,
        overlap_lines,
        extra_calls,
        Some(extra_calls),
        units.as_deref(),
    );
    let tv: Vec<(String, Option<i64>)> = pool
        .validators
        .iter()
        .map(|v| (v.name().to_string(), v.timeout_override()))
        .collect();
    let advice = lope_providers::latency::fit_warnings(&tv, Some(cfg.timeout_secs()));
    let mut exec_profiles = Vec::new();
    for v in &pool.validators {
        if let Some(ep) = v.execution_profile("review") {
            exec_profiles.push(ep);
        }
    }
    let mut plan_json = plan.json.clone();
    plan_json["budget_advice"] = json!(advice);
    plan_json["execution_profiles"] = json!(exec_profiles);
    ctx.add_request_plan(plan_json.clone());

    let json_flag = flag(m, "--json");
    if plan.action == "reject" {
        if json_flag {
            let mut payload = ctx.budget.snapshot();
            payload["partial"] = json!(false);
            payload["reason"] = json!("input_limit");
            payload["request_plan"] = plan_json;
            payload["error"] = json!(plan.reason);
            let mut out = Out::new();
            out.line(pyjson::dumps(&payload, &Opts::pretty_sorted()));
            out.flush();
        } else {
            eprintln!("Request rejected before launch: {}", plan.reason);
            if !plan.mitigation.is_empty() {
                eprintln!("  {}", plan.mitigation);
            }
        }
        return Err(2);
    }

    if !json_flag {
        eprintln!(
            "Request plan: {} · {} bytes · {} chunk(s) · {} call(s) · concurrency {} · nominal ceiling {}s",
            plan.action,
            plan.utf8_bytes,
            plan.required_chunks,
            plan.planned_calls,
            plan.maximum_concurrency,
            lope_providers::progress::g(plan.nominal)
        );
    }
    for l in &advice {
        eprintln!("{l}");
    }
    for ep in &exec_profiles {
        if let Some(ws) = ep.get("warnings").and_then(Value::as_array) {
            for w in ws {
                if let Some(s) = w.as_str() {
                    eprintln!("{s}");
                }
            }
        }
    }
    Ok(plan)
}

#[allow(clippy::too_many_arguments)]
fn run_single_consensus_review(
    m: &ArgMatches,
    target: &str,
    content: &str,
    validators: &[String],
    pool: &Pool,
    cfg: &LopeCfg,
    ctx: &Context,
    focus: &str,
    brain_context_block: Option<&str>,
    source_label: Option<&str>,
) -> ReviewReport {
    let similarity = m
        .try_get_one::<f64>("--similarity")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0.85);
    let min_consensus = m
        .try_get_one::<f64>("--min-consensus")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0.0);
    let review_input = ReviewInput {
        target: target.to_string(),
        content: content.to_string(),
        focus: focus.to_string(),
        source_label: Some(source_label.unwrap_or(target).to_string()),
    };
    let prompt = build_review_prompt(&review_input, brain_context_block);
    let timeout = cfg.timeout_secs() as f64;
    let rows = fanout::fanout_generate(&pool.validators, pool.parallel, &prompt, timeout, ctx);
    let fanout_rows: Vec<FanoutRow> = rows
        .into_iter()
        .map(|(n, a, e)| (n, a, e.as_ref().map(fanout::err_text)))
        .collect();
    build_consensus_report(
        target,
        focus,
        validators,
        &fanout_rows,
        similarity,
        min_consensus,
        source_label,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_report_via_semantic_chunks(
    m: &ArgMatches,
    chunks: &[RequestChunk],
    target_label: &str,
    fallback_source: &str,
    validator_names: &[String],
    pool: &Pool,
    cfg: &LopeCfg,
    ctx: &Context,
    focus: &str,
    brain_context_block: Option<&str>,
) -> ReviewReport {
    let similarity = m
        .try_get_one::<f64>("--similarity")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0.85);
    let min_consensus = m
        .try_get_one::<f64>("--min-consensus")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0.0);
    let mut aggregated_findings = Vec::new();
    let mut aggregated_errors = Vec::new();
    let mut raw_results_combined = Vec::new();
    let mut parse_methods_combined = BTreeMap::new();
    let mut evidence = Vec::new();

    for chunk in chunks {
        let label = chunk.label();
        eprintln!("  → reviewing {label}");
        let chunk_report = run_single_consensus_review(
            m,
            &label,
            &chunk.content,
            validator_names,
            pool,
            cfg,
            ctx,
            focus,
            brain_context_block,
            Some(fallback_source),
        );
        aggregated_findings.extend(chunk_report.findings);
        aggregated_errors.extend(chunk_report.errors);
        for row in &chunk_report.raw_results {
            evidence.push(json!({
                "chunk": chunk.index + 1,
                "chunk_label": label,
                "validator": row.validator,
                "answer": row.answer,
                "error": row.error,
            }));
        }
        raw_results_combined.extend(chunk_report.raw_results);
        for (name, method) in chunk_report.parse_methods {
            parse_methods_combined.entry(name).or_insert(method);
        }
    }

    let merged = merge_findings(&aggregated_findings, similarity);
    let mut scored = score_consensus(&merged, validator_names);
    if min_consensus > 0.0 {
        scored.retain(|item| item.consensus_score >= min_consensus);
    }
    if let Some(ev_path) = write_chunk_evidence(ctx, "review", &evidence) {
        if !flag(m, "--json") {
            eprintln!("Chunk evidence: {ev_path}");
        }
    }
    let raw_count = aggregated_findings.len();
    let merged_count = merged.len();
    let fallback = aggregated_findings.is_empty();
    ReviewReport {
        target: format!("{target_label} ({} chunk(s))", chunks.len()),
        focus: if focus.is_empty() {
            "(default)".to_string()
        } else {
            focus.to_string()
        },
        validators: validator_names.to_vec(),
        raw_results: raw_results_combined,
        parse_methods: parse_methods_combined,
        findings: aggregated_findings,
        merged,
        scored,
        errors: aggregated_errors,
        raw_count,
        merged_count,
        fallback,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_report_via_divided_files(
    m: &ArgMatches,
    target_path: &str,
    validator_names: &[String],
    pool: &Pool,
    cfg: &LopeCfg,
    ctx: &Context,
    focus: &str,
    out: &mut Out,
) -> Result<ReviewReport, i32> {
    let (chunks, skipped) = match split_files(Path::new(target_path), DEFAULT_MAX_CHARS, DEFAULT_MAX_FILE_BYTES) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Cannot read {target_path}: {e}");
            return Err(1);
        }
    };
    if !skipped.is_empty() && !flag(m, "--json") {
        out.line(format!("Skipped {} non-reviewable file(s):", skipped.len()));
        for entry in skipped.iter().take(10) {
            out.line(format!("  - {}: {}", entry.path, entry.reason));
        }
        if skipped.len() > 10 {
            out.line(format!("  ... and {} more", skipped.len() - 10));
        }
        out.blank();
        out.flush();
    }
    if chunks.is_empty() {
        eprintln!("No reviewable files found.");
        return Err(1);
    }
    let packed = pack_file_chunks(&chunks, DEFAULT_MAX_CHARS);
    let combined_content: String = packed.iter().map(|i| i.content.as_str()).collect();
    let units: Vec<SemanticUnit> = packed
        .into_iter()
        .map(|item| {
            let label = item.label();
            SemanticUnit::new(label, item.content, 1, 1, "file")
        })
        .collect();
    let plan = plan_external_request(
        m,
        cfg,
        pool,
        ctx,
        &combined_content,
        true,
        target_path,
        "auto",
        0,
        Some(units),
        Some("chunk"),
    )?;
    Ok(build_report_via_semantic_chunks(
        m,
        &plan.chunks,
        target_path,
        "(packed files)",
        validator_names,
        pool,
        cfg,
        ctx,
        focus,
        None,
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_report_via_divided_hunks(
    m: &ArgMatches,
    target_path: &str,
    content: &str,
    validator_names: &[String],
    pool: &Pool,
    cfg: &LopeCfg,
    ctx: &Context,
    focus: &str,
) -> Result<ReviewReport, i32> {
    let hunks = split_diff_hunks(content);
    if hunks.is_empty() {
        eprintln!("--divide hunks: no diff hunks parsed from {target_path}.");
        return Err(2);
    }
    let packed = pack_hunk_chunks(&hunks, DEFAULT_MAX_CHARS);
    let combined_content: String = packed.iter().map(|i| i.content.as_str()).collect();
    let units: Vec<SemanticUnit> = packed
        .into_iter()
        .map(|item| {
            let label = item.label();
            SemanticUnit::new(label, item.content, 1, 1, "diff")
        })
        .collect();
    let plan = plan_external_request(
        m,
        cfg,
        pool,
        ctx,
        &combined_content,
        true,
        target_path,
        "diff",
        0,
        Some(units),
        Some("chunk"),
    )?;
    Ok(build_report_via_semantic_chunks(
        m,
        &plan.chunks,
        target_path,
        "(packed diff hunks)",
        validator_names,
        pool,
        cfg,
        ctx,
        focus,
        None,
    ))
}

#[allow(clippy::too_many_arguments)]
fn build_report_via_roles(
    m: &ArgMatches,
    target_path: &str,
    content: &str,
    validator_names: &[String],
    pool: &Pool,
    cfg: &LopeCfg,
    ctx: &Context,
    focus: &str,
    roles_spec: &str,
) -> Result<ReviewReport, i32> {
    let roles = match parse_roles(roles_spec) {
        Ok(r) if !r.is_empty() => r,
        Ok(_) => {
            eprintln!("lope review --roles: no usable role names in '{roles_spec}'");
            return Err(2);
        }
        Err(msg) => {
            eprintln!("{msg}");
            return Err(1);
        }
    };
    let role_assignments = match assign_roles(validator_names, &roles) {
        Ok(ra) => ra,
        Err(msg) => {
            eprintln!("{msg}");
            return Err(1);
        }
    };
    let base_prompt = build_review_prompt(
        &ReviewInput {
            target: target_path.to_string(),
            content: content.to_string(),
            focus: focus.to_string(),
            source_label: Some(target_path.to_string()),
        },
        None,
    );
    let similarity = m
        .try_get_one::<f64>("--similarity")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0.85);
    let min_consensus = m
        .try_get_one::<f64>("--min-consensus")
        .ok()
        .flatten()
        .copied()
        .unwrap_or(0.0);

    let mut role_prompts = BTreeMap::new();
    let mut missing: Vec<FanoutRow> = Vec::new();
    for v_name in validator_names {
        let has_validator = pool.validators.iter().any(|v| v.name() == v_name);
        let role = role_assignments.get(v_name);
        if !has_validator || role.is_none() {
            missing.push((
                v_name.clone(),
                String::new(),
                Some("validator unavailable".to_string()),
            ));
            continue;
        }
        role_prompts.insert(
            v_name.clone(),
            build_role_prompt(role.unwrap(), &base_prompt),
        );
    }
    let child_ctx = ctx.child(Some("role_review"), None);
    let timeout = cfg.timeout_secs() as f64;
    let raw_rows = fanout::fanout_generate_with_prompts(
        &pool.validators,
        pool.parallel,
        &base_prompt,
        Some(&role_prompts),
        timeout,
        &child_ctx,
    );
    let mut raw: Vec<FanoutRow> = raw_rows
        .into_iter()
        .map(|(n, a, e)| (n, a, e.as_ref().map(fanout::err_text)))
        .collect();
    raw.extend(missing);

    let raw_results: Vec<RawReviewResult> = raw
        .iter()
        .map(|(name, answer, error)| RawReviewResult {
            validator: name.clone(),
            answer: redact_text(answer).trim_end().to_string(),
            error: error
                .as_ref()
                .filter(|e| !e.is_empty())
                .map(|e| redact_text(e).trim().to_string()),
            role: role_assignments.get(name).map(|r| r.name.to_string()),
        })
        .collect();

    let (mut findings, parse_results, errors) = parse_responses(&raw, Some(target_path));
    for f in &mut findings {
        if f.category.is_none() {
            if let Some(role) = role_assignments.get(&f.validator) {
                f.category = Some(role.name.to_string());
            }
        }
    }

    let merged = merge_findings(&findings, similarity);
    let mut scored = score_consensus(&merged, validator_names);
    if min_consensus > 0.0 {
        scored.retain(|s| s.consensus_score >= min_consensus);
    }
    let parse_methods: BTreeMap<String, String> = parse_results
        .into_iter()
        .map(|(k, v)| (k, v.method))
        .collect();
    let role_names: Vec<&str> = roles.iter().map(|r| r.name).collect();
    let raw_count = findings.len();
    let merged_count = merged.len();
    let fallback = findings.is_empty();

    Ok(ReviewReport {
        target: format!("{target_path} (roles: {})", role_names.join(", ")),
        focus: if focus.is_empty() {
            "(default)".to_string()
        } else {
            focus.to_string()
        },
        validators: validator_names.to_vec(),
        raw_results,
        parse_methods,
        findings,
        merged,
        scored,
        errors,
        raw_count,
        merged_count,
        fallback,
    })
}

fn write_chunk_evidence(ctx: &Context, mode: &str, rows: &[Value]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let run_id = ctx.run_id();
    let root = std::env::var("LOPE_RUN_OUTPUT_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join("lope-runs"));
    let destination_dir = root.join(run_id);
    let mut db = fs::DirBuilder::new();
    db.recursive(true).mode(0o700);
    let _ = db.create(&destination_dir);
    let _ = fs::set_permissions(&destination_dir, fs::Permissions::from_mode(0o700));
    let destination = destination_dir.join(format!("{mode}-chunks.json"));
    let mut safe_rows = Vec::new();
    for row in rows {
        let mut answer = redact_text(row.get("answer").and_then(Value::as_str).unwrap_or(""));
        let raw_bytes = answer.as_bytes();
        let omitted = raw_bytes.len().saturating_sub(64 * 1024);
        if omitted > 0 {
            let mut cut = 64 * 1024;
            while cut > 0 && !answer.is_char_boundary(cut) {
                cut -= 1;
            }
            answer.truncate(cut);
        }
        let err_str = redact_text(row.get("error").and_then(Value::as_str).unwrap_or(""));
        safe_rows.push(json!({
            "chunk": row.get("chunk").and_then(Value::as_i64).unwrap_or(0),
            "chunk_label": redact_text(row.get("chunk_label").and_then(Value::as_str).unwrap_or("")),
            "validator": redact_text(row.get("validator").and_then(Value::as_str).unwrap_or("")),
            "answer": answer,
            "error": if err_str.is_empty() { Value::Null } else { Value::String(err_str) },
            "omitted_answer_bytes": omitted,
        }));
    }
    let payload = json!({
        "schema_version": 1,
        "run_id": run_id,
        "mode": mode,
        "chunks": safe_rows,
    });
    let text = format!("{}\n", pyjson::dumps(&payload, &Opts::pretty_sorted()));
    if write_output_atomic_mode(&destination, &text, 0o600).is_err() {
        return None;
    }
    let dest_str = destination.display().to_string();
    ctx.budget.add_artifact(
        &format!("{mode}_chunk_evidence"),
        &dest_str,
        vec![("chunks", json!(safe_rows.len()))],
    );
    Some(dest_str)
}

fn write_output_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    write_output_atomic_mode(path, content, 0o644)
}

fn write_output_atomic_mode(path: &Path, content: &str, mode: u32) -> std::io::Result<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("output"),
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)?;
    f.write_all(content.as_bytes())?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)
}
