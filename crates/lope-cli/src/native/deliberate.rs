//! `lope deliberate` (`cli.py:_cmd_deliberate`, Sprint S6, `FR31`, `A14`).

use super::scope::{self, flag, os_str};
use super::{Native, Out};
use clap::ArgMatches;
use lope_core::config::LopeCfg;
use lope_core::pyjson::{self, Opts};
use lope_engine::deliberate::{
    default_output_dir, get_template, run_deliberation, DeliberationError, DeliberationOptions,
    DEFAULT_DEPTH,
};
use lope_makakoo::{cli_brain_context_block, cli_emit_brain_log};
use lope_providers::errors::ProviderError;
use lope_providers::invocation::invoke_generate;
use lope_providers::request_plan::{plan_request_full, Limits};
use serde_json::json;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

fn brain_budget(m: &ArgMatches) -> Option<usize> {
    m.try_get_one::<i64>("--brain-budget")
        .ok()
        .flatten()
        .map(|n| (*n).max(1) as usize)
}

pub fn run(m: &ArgMatches) -> Native {
    let template_arg = m
        .try_get_one::<String>("template")
        .ok()
        .flatten()
        .cloned()
        .unwrap_or_default();
    let spec = match get_template(&template_arg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lope deliberate: {e}");
            return Native::Exit(2);
        }
    };

    let scenario_arg = os_str(m, "scenario").unwrap_or_default();
    let mut stdin_buf = Vec::new();
    let mut scenario = if scenario_arg == "-" {
        if unsafe { libc::isatty(0) } == 1 {
            eprintln!("lope deliberate: '-' requires piped stdin");
            return Native::Exit(2);
        }
        let _ = std::io::stdin().read_to_end(&mut stdin_buf);
        String::from_utf8_lossy(&stdin_buf).into_owned()
    } else {
        let p = Path::new(&scenario_arg);
        if !p.is_file() {
            eprintln!("Scenario file not found: {scenario_arg}");
            return Native::Exit(1);
        }
        match fs::read(p) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                eprintln!("Cannot read {scenario_arg}: {e}");
                return Native::Exit(1);
            }
        }
    };

    if scenario.trim().is_empty() {
        eprintln!("lope deliberate: scenario is empty");
        return Native::Exit(2);
    }

    let bc = os_str(m, "--brain-context").filter(|s| !s.is_empty());
    match cli_brain_context_block(bc.as_deref(), brain_budget(m)) {
        Ok(Some(block)) => {
            scenario = format!("{block}\n{scenario}");
        }
        Ok(None) => {}
        Err(code) => return Native::Exit(code),
    }

    let (cfg, pool) = match scope::resolve(m) {
        Ok(v) => v,
        Err(reason) => {
            let n = Native::Delegate(reason);
            return if scenario_arg == "-" {
                n.with_stdin(stdin_buf)
            } else {
                n
            };
        }
    };

    let validator_names = pool.names();
    if validator_names.is_empty() {
        eprintln!("No validators available. Run: lope status");
        return Native::Exit(1);
    }

    let json_mode = flag(m, "--json");
    let scope = match scope::start("deliberate", &cfg, json_mode) {
        Ok(s) => s,
        Err(reason) => {
            let n = Native::Delegate(format!("runtime start failed: {reason}"));
            return if scenario_arg == "-" {
                n.with_stdin(stdin_buf)
            } else {
                n
            };
        }
    };

    let lim = Limits {
        max_chunks: LopeCfg::int_field(&cfg.max_chunks),
        max_calls: LopeCfg::int_field(&cfg.max_calls),
        max_input_bytes: LopeCfg::int_field(&cfg.max_input_bytes),
    };
    let timeout_secs = cfg.timeout_secs();
    let plan = plan_request_full(
        &scenario,
        "deliberate",
        &pool.validators,
        &cfg.request_policy,
        &lim,
        timeout_secs as f64,
        pool.parallel,
        false,
        "scenario",
        "markdown",
        0,
        0,
        None,
        None,
    );
    let tv: Vec<(String, Option<i64>)> = pool
        .validators
        .iter()
        .map(|v| (v.name().to_string(), v.timeout_override()))
        .collect();
    let advice = lope_providers::latency::fit_warnings(&tv, Some(timeout_secs));
    let mut plan_json = plan.json.clone();
    plan_json["budget_advice"] = json!(advice);
    plan_json["execution_profiles"] = json!([]);
    scope.ctx.add_request_plan(plan_json.clone());

    if plan.action == "reject" {
        if json_mode {
            let mut payload = scope.ctx.budget.snapshot();
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
        return Native::Exit(scope::finish(scope, 2));
    }

    for w in &pool.warnings {
        eprintln!("{w}");
    }
    if !json_mode {
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

    let primary_name = pool.primary.clone();
    let depth = m
        .try_get_one::<String>("--depth")
        .ok()
        .flatten()
        .map(String::as_str)
        .unwrap_or(DEFAULT_DEPTH);
    let anonymize = flag(m, "--no-anonymize");
    let out_dir = os_str(m, "--out")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_output_dir(&spec, None));

    let mut out = Out::new();
    if !json_mode {
        out.line(format!("\nLope deliberate: {}", spec.title));
        out.line(format!("Council: {}", validator_names.join(", ")));
        out.line(format!("Primary: {primary_name}"));
        out.line(format!(
            "Depth: {depth}  ·  Anonymous: {}",
            if anonymize { "True" } else { "False" }
        ));
        out.line(format!("Output: {}\n", out_dir.display()));
        out.flush();
    }

    let generate = |name: &str, prompt: &str, timeout: u64| -> Result<String, String> {
        let Some(validator) = pool.validators.iter().find(|v| v.name() == name) else {
            return Ok(format!("[validator {name} unavailable]"));
        };
        let call_plan = plan_request_full(
            prompt,
            "deliberate",
            &[std::sync::Arc::clone(validator)],
            &cfg.request_policy,
            &lim,
            timeout as f64,
            false,
            false,
            &format!("deliberation turn for {name}"),
            "markdown",
            0,
            0,
            None,
            None,
        );
        scope.ctx.add_request_plan(call_plan.json.clone());
        if call_plan.action == "reject" {
            let mut msg = format!("input_limit: {}", call_plan.reason);
            if !call_plan.mitigation.is_empty() {
                msg.push_str(&format!("; {}", call_plan.mitigation));
            }
            return Err(msg);
        }
        match invoke_generate(
            validator.as_ref(),
            prompt,
            timeout as f64,
            Some(&scope.ctx),
            Some("deliberation"),
        ) {
            Ok(ans) => Ok(ans),
            Err(ProviderError::NotImplemented(e)) => {
                Err(format!("validator {name} cannot generate: {e}"))
            }
            Err(e) => Err(format!("validator {name} errored: {e}")),
        }
    };

    let run = match run_deliberation(
        DeliberationOptions {
            template: spec.clone(),
            scenario: &scenario,
            validators: &validator_names,
            primary: Some(&primary_name),
            depth,
            timeout: timeout_secs.max(1) as u64,
            anonymous: anonymize,
            output_dir: Some(&out_dir),
            parallel: pool.parallel,
            max_workers: validator_names.len().clamp(1, 5),
        },
        generate,
    ) {
        Ok(r) => r,
        Err(DeliberationError::Inconclusive(msg)) => {
            drop(out);
            scope.ctx.budget.mark_partial(&msg);
            eprintln!("lope deliberate: {msg}");
            return Native::Exit(scope::finish(scope, 1));
        }
        Err(DeliberationError::Value(msg)) => {
            drop(out);
            eprintln!("lope deliberate: {msg}");
            return Native::Exit(scope::finish(scope, 2));
        }
        Err(DeliberationError::Runtime(msg)) => {
            drop(out);
            eprintln!("lope deliberate: {msg}");
            return Native::Exit(scope::finish(scope, 1));
        }
    };

    for (kind, path) in [
        ("deliberation_report", out_dir.join("final").join("report.md")),
        (
            "deliberation_minority",
            out_dir.join("final").join("minority-report.md"),
        ),
        ("deliberation_trace", out_dir.join("trace.jsonl")),
    ] {
        if path.exists() {
            let p_str = path.display().to_string();
            scope.ctx.budget.add_artifact(kind, &p_str, vec![]);
        }
    }

    let pass_count = run.rubric.iter().filter(|r| r.status == "PASS").count();
    let fix_count = run
        .rubric
        .iter()
        .filter(|r| r.status == "NEEDS_FIX")
        .count();

    let journal_text = format!(
        "[[Lope]] deliberated {} from `{scenario_arg}` → `{}`. Council: {}. Rubric: {pass_count} PASS, {fix_count} NEEDS_FIX. [[Makakoo OS]]",
        spec.name,
        out_dir.display(),
        run.validators.len(),
    );
    let brain_ack = cli_emit_brain_log(flag(m, "--brain-log"), &journal_text);

    if json_mode {
        let mut payload = run.to_dict();
        payload["runtime"] = scope.ctx.budget.snapshot();
        if let Some(ack) = &brain_ack {
            payload["brain"] = json!({ "note": ack });
        }
        out.line(pyjson::dumps(&payload, &Opts::pretty_sorted()));
    } else {
        out.line(format!(
            "Synthesis: {}",
            out_dir.join("final").join("report.md").display()
        ));
        out.line(format!(
            "Minority report: {}",
            out_dir.join("final").join("minority-report.md").display()
        ));
        out.line(format!(
            "Decision log: {}",
            out_dir.join("final").join("decision-log.md").display()
        ));
        out.line(format!("Trace: {}", out_dir.join("trace.jsonl").display()));
        out.line(format!("Rubric: {pass_count} PASS · {fix_count} NEEDS_FIX"));
        if let Some(ack) = &brain_ack {
            out.line(ack);
        }
    }

    drop(out);
    Native::Exit(scope::finish(scope, 0))
}
