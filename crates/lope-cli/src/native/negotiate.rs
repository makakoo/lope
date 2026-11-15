//! Native handler for `lope negotiate` (`cli.py`, Sprint S7/S8B, `FR32`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use lope_core::config::LopeCfg;
use lope_engine::models::EscalationRequired;
use lope_engine::negotiator::{
    build_user_prompt, negotiator_system_prompt, ConvergenceOutcome, Negotiator,
};
use lope_providers::pool::Pool;
use lope_providers::request_plan::{plan_request_full, Limits};
use lope_state::runlock::{acquire as acquire_runlock, EXIT_TEMPFAIL};
use std::fs;
use std::path::{Path, PathBuf};

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_isize_default(name: &str, default: isize) -> isize {
    std::env::var(name)
        .ok()
        .and_then(|s| s.trim().parse::<isize>().ok())
        .unwrap_or(default)
}

fn expanduser(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    } else if p == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(p)
}

fn utf8_stats(text: &str) -> (usize, usize) {
    if text.is_empty() {
        (0, 0)
    } else {
        (text.len(), text.matches('\n').count() + 1)
    }
}

fn load_context(
    context_file: Option<&str>,
    inline_context: Option<&str>,
) -> Result<String, String> {
    let mut parts = Vec::new();
    if let Some(cf) = context_file.filter(|s| !s.is_empty()) {
        let path = expanduser(cf);
        let bytes = fs::read(&path)
            .map_err(|e| format!("cannot read --context-file {}: {e}", path.display()))?;
        let text = String::from_utf8_lossy(&bytes);
        if !text.trim().is_empty() {
            parts.push(format!(
                "## Context file: {}\n\n{}",
                path.display(),
                text.trim_end()
            ));
        }
    }
    if let Some(inline) = inline_context {
        if !inline.trim().is_empty() {
            parts.push(inline.trim().to_string());
        }
    }
    Ok(parts.join("\n\n"))
}

fn negotiate_prompt_profile(
    goal: &str,
    context: &str,
    domain: &str,
    timeout_secs: usize,
    context_file: Option<&str>,
    inline_context: Option<&str>,
) -> Vec<String> {
    let has_context = !context.trim().is_empty()
        || context_file.is_some_and(|s| !s.is_empty())
        || inline_context.is_some_and(|s| !s.trim().is_empty());
    if !has_context {
        return Vec::new();
    }

    let system = negotiator_system_prompt(domain);
    let user = build_user_prompt(goal, context);
    let combined = format!("{system}\n\n{user}");
    let (context_bytes, context_lines) = utf8_stats(context);
    let (prompt_bytes, prompt_lines) = utf8_stats(&combined);
    let large_bytes = env_isize_default("LOPE_NEGOTIATE_LARGE_PROMPT_BYTES", 12_000).max(0) as usize;
    let large_lines = env_isize_default("LOPE_NEGOTIATE_LARGE_PROMPT_LINES", 250).max(0) as usize;
    let low_timeout = env_isize_default("LOPE_NEGOTIATE_LOW_TIMEOUT_SECONDS", 300).max(0) as usize;

    let mut lines = vec![
        "Negotiate preflight:".to_string(),
        format!("  Context payload: {context_bytes} bytes, {context_lines} lines"),
        format!("  Generated drafter prompt: {prompt_bytes} bytes, {prompt_lines} lines"),
        format!("  Effective timeout: {timeout_secs}s per drafter/reviewer call"),
    ];
    if context_file.is_some_and(|s| !s.is_empty()) {
        lines.push(
            "  Note: --context-file is read and inlined into the model prompt; \
             it is not attached as a separate file."
                .to_string(),
        );
    } else if inline_context.is_some_and(|s| !s.trim().is_empty()) {
        lines.push("  Note: --context is inlined into the model prompt.".to_string());
    }
    if timeout_secs <= low_timeout
        && (prompt_bytes >= large_bytes || prompt_lines >= large_lines)
    {
        lines.push(format!(
            "  WARNING: large generated prompt with low timeout. \
             Thresholds: >= {large_bytes} bytes or >= {large_lines} lines, \
             timeout <= {low_timeout}s. Use a compact LOPE_BRIEF.md or --timeout 300+."
        ));
    }
    lines
}

fn path_with_extra_suffix(path: &Path, extra_suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.push_str(extra_suffix);
    path.with_file_name(name)
}

pub fn write_escalation_artifacts(
    out_path: &str,
    negotiator: &Negotiator,
    result: &EscalationRequired,
) -> Vec<(&'static str, String)> {
    let base = Path::new(out_path);
    if let Some(parent) = base.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = fs::create_dir_all(parent);
    }
    let rounds = &negotiator.rounds;
    let mut written = Vec::new();

    if let Some(last_draft) = rounds.iter().rfind(|r| r.proposer == "drafter") {
        let content = format!("{}\n", last_draft.text.trim_end());
        if fs::write(base, content).is_ok() {
            written.push(("last_proposal", base.display().to_string()));
        }
    }

    let feedback_path = path_with_extra_suffix(base, ".feedback.md");
    let mut lines = vec![
        "# Lope negotiation escalated".to_string(),
        String::new(),
        format!("- Reason: {}", result.reason),
        format!("- Phase: {} {}", result.phase_index, result.phase_name),
    ];
    if let Some(ref last_verdict) = result.last_verdict {
        lines.push(format!("- Last verdict: {}", last_verdict.status.as_str()));
        lines.push(format!("- Confidence: {:.2}", last_verdict.confidence));
        let rationale = if last_verdict.rationale.trim().is_empty() {
            "(none)"
        } else {
            last_verdict.rationale.as_str()
        };
        lines.push(format!("- Rationale: {rationale}"));
        lines.push(String::new());
        lines.push("## Required fixes".to_string());
        lines.push(String::new());
        if last_verdict.required_fixes.is_empty() {
            lines.push("- (none listed)".to_string());
        } else {
            for fix in &last_verdict.required_fixes {
                lines.push(format!("- {fix}"));
            }
        }
    }
    if let Some(last_fb) = rounds.iter().rfind(|r| r.proposer == "validator") {
        lines.push(String::new());
        lines.push("## Raw validator feedback".to_string());
        lines.push(String::new());
        let body = last_fb.text.trim_end();
        lines.push(if body.is_empty() {
            "(empty)".to_string()
        } else {
            body.to_string()
        });
    }
    let feedback_body = format!("{}\n", lines.join("\n").trim_end());
    if fs::write(&feedback_path, feedback_body).is_ok() {
        written.push(("feedback", feedback_path.display().to_string()));
    }

    let rounds_dir = path_with_extra_suffix(base, ".rounds");
    let _ = fs::create_dir_all(&rounds_dir);
    for (idx, round_item) in rounds.iter().enumerate() {
        let round_path = rounds_dir.join(format!(
            "{:02}-round{}-{}.md",
            idx + 1,
            round_item.number,
            round_item.proposer
        ));
        let mut header = vec![
            format!("# Round {} — {}", round_item.number, round_item.proposer),
            String::new(),
        ];
        if let Some(ref v) = round_item.verdict {
            header.push(format!("- status: {}", v.status.as_str()));
            header.push(format!("- confidence: {:.2}", v.confidence));
            header.push(String::new());
        }
        let body = round_item.text.trim_end();
        let text = format!(
            "{}{}\n",
            header.join("\n"),
            if body.is_empty() { "(empty)" } else { body }
        );
        let _ = fs::write(&round_path, text);
    }
    written.push(("rounds_dir", rounds_dir.display().to_string()));
    written
}

pub fn run(m: &ArgMatches) -> Native {
    let _run_lock = match acquire_runlock("negotiate") {
        Ok(l) => l,
        Err(busy) => {
            eprint!("{}", busy.message);
            return Native::Exit(EXIT_TEMPFAIL);
        }
    };
    let Some(goal) = scope::os_str(m, "goal") else {
        eprintln!("lope negotiate: missing goal");
        return Native::Exit(2);
    };
    let out_opt = scope::os_str(m, "--out").filter(|s| !s.is_empty());
    let max_rounds = scope::u64_of(m, "--max-rounds")
        .filter(|&v| v > 0)
        .map(|v| v as usize)
        .unwrap_or(3);
    let domain = m
        .try_get_one::<String>("--domain")
        .ok()
        .flatten()
        .cloned()
        .unwrap_or_else(|| "engineering".to_string());
    let context_file = scope::os_str(m, "--context-file");
    let inline_context = scope::os_str(m, "--context");
    let brain_ctx = scope::os_str(m, "--brain-context");
    let brain_budget = scope::u64_of(m, "--brain-budget").map(|v| v as usize);
    let brain_log = scope::flag(m, "--brain-log");

    let (cfg, pool) = match scope::resolve(m) {
        Ok(x) => x,
        Err(e) => {
            drop(_run_lock);
            return Native::Delegate(format!("negotiate: {e}"));
        }
    };
    let sc = match scope::start("negotiate", &cfg, false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lope negotiate: {e}");
            return Native::Exit(2);
        }
    };

    println!("\nLope negotiate: {goal}\n");

    let drafter_name = pool.primary.clone();
    let rev_names: Vec<String> = pool
        .reviewers()
        .iter()
        .map(|v| v.name().to_string())
        .collect();
    let reviewers_display = if rev_names.is_empty() {
        "(none — need at least 2 validators for real ensemble review)".to_string()
    } else {
        rev_names.join(", ")
    };
    println!("Drafter: {drafter_name}  ·  Reviewers: {reviewers_display}\n");

    let mut context = match load_context(context_file.as_deref(), inline_context.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("lope negotiate failed: {e}");
            return Native::Exit(scope::finish(sc, 2));
        }
    };
    match lope_makakoo::cli_brain_context_block(brain_ctx.as_deref(), brain_budget) {
        Ok(Some(block)) => {
            if context.trim().is_empty() {
                context = block;
            } else {
                context = format!("{block}\n{context}");
            }
        }
        Ok(None) => {}
        Err(code) => return Native::Exit(scope::finish(sc, code)),
    }

    let timeout_secs = cfg.timeout_secs().max(1) as usize;
    let timeout = cfg.timeout.as_f64().unwrap_or(960.0);
    let evidence_doc = format!("## Goal\n{goal}\n\n## Original evidence brief\n{context}");
    let lim = Limits {
        max_chunks: LopeCfg::int_field(&cfg.max_chunks),
        max_calls: LopeCfg::int_field(&cfg.max_calls),
        max_input_bytes: LopeCfg::int_field(&cfg.max_input_bytes),
    };
    let plan = plan_request_full(
        &evidence_doc,
        "negotiate",
        &pool.validators,
        &cfg.request_policy,
        &lim,
        timeout,
        pool.parallel,
        false,
        "negotiation evidence",
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
    let advice = lope_providers::latency::fit_warnings(&tv, Some(cfg.timeout_secs()));
    let mut plan_json = plan.json.clone();
    plan_json["budget_advice"] = serde_json::json!(advice);
    plan_json["execution_profiles"] = serde_json::json!([]);
    sc.ctx.add_request_plan(plan_json);

    if plan.action == "reject" {
        eprintln!("Request rejected before launch: {}", plan.reason);
        if !plan.mitigation.is_empty() {
            eprintln!("  {}", plan.mitigation);
        }
        return Native::Exit(scope::finish(sc, 2));
    }
    eprintln!(
        "Request plan: {} · {} bytes · {} chunk(s) · {} call(s) · concurrency {} · nominal ceiling {}s",
        plan.action,
        plan.utf8_bytes,
        plan.required_chunks,
        plan.planned_calls,
        plan.maximum_concurrency,
        lope_providers::progress::g(plan.nominal)
    );
    for l in &advice {
        eprintln!("{l}");
    }

    let preflight = negotiate_prompt_profile(
        &goal,
        &context,
        &domain,
        timeout_secs,
        context_file.as_deref(),
        inline_context.as_deref(),
    );
    if !preflight.is_empty() {
        for line in &preflight {
            println!("{line}");
        }
        println!();
    }

    let max_total = env_usize("LOPE_MAX_TOTAL_ROUNDS", 25);
    let pool_for_neg = Pool {
        validators: pool.validators.clone(),
        primary: pool.primary.clone(),
        parallel: pool.parallel,
        ensemble: pool.ensemble,
        unsupported: Vec::new(),
        warnings: Vec::new(),
    };
    let mut negotiator = Negotiator::new(pool_for_neg, max_rounds, timeout, max_total, &domain);
    let outcome = match negotiator.converge(&goal, &context, Some(&sc.ctx), None) {
        Ok(o) => o,
        Err(e) => {
            println!();
            println!("lope negotiate failed:");
            for line in e.lines() {
                println!("  {line}");
            }
            println!();
            return Native::Exit(scope::finish(sc, 2));
        }
    };

    match outcome {
        ConvergenceOutcome::Converged(mut doc) => {
            let final_out = out_opt
                .unwrap_or_else(|| format!("SPRINT-{}.md", doc.slug.to_ascii_uppercase()));
            if let Err(e) = doc.save(Path::new(&final_out)) {
                eprintln!("lope negotiate: failed writing {final_out}: {e}");
                return Native::Exit(scope::finish(sc, 1));
            }
            println!("Sprint doc saved to: {final_out}");
            println!("Rounds: {}", negotiator.rounds.len());
            println!("\nRun: lope execute {final_out}");
            let goal_short: String = goal.chars().take(120).collect();
            if let Some(ack) = lope_makakoo::cli_emit_brain_log(
                brain_log,
                &format!(
                    "[[Lope]] negotiated sprint `{final_out}` for goal: {goal_short}. Rounds: {}. [[Makakoo OS]]",
                    negotiator.rounds.len()
                ),
            ) {
                println!("{ack}");
            }
            Native::Exit(scope::finish(sc, 0))
        }
        ConvergenceOutcome::Escalated(esc) => {
            println!("Negotiation escalated: {esc}");
            if let Some(ref out_p) = out_opt {
                let written = write_escalation_artifacts(out_p, &negotiator, &esc);
                if !written.is_empty() {
                    println!("Escalation artifacts:");
                    for (label, path) in written {
                        println!("  {label}: {path}");
                    }
                }
            }
            Native::Exit(scope::finish(sc, 1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lope_engine::models::{PhaseVerdict, Round, VerdictStatus};
    use tempfile::tempdir;

    #[test]
    fn escalation_artifacts_write_proposal_feedback_and_rounds() {
        let dir = tempdir().unwrap();
        let out = dir.path().join("SPRINT.md");
        let mut neg = Negotiator::new(
            Pool {
                validators: Vec::new(),
                primary: "a".into(),
                parallel: false,
                ensemble: false,
                unsupported: Vec::new(),
                warnings: Vec::new(),
            },
            2,
            30.0,
            10,
            "engineering",
        );
        neg.rounds.push(Round {
            number: 1,
            proposer: "drafter".into(),
            text: "# Draft 1".into(),
            verdict: None,
        });
        let mut v = PhaseVerdict::new(VerdictStatus::NeedsFix, "add tests", 1.2, "rev");
        v.confidence = 0.6;
        v.required_fixes = vec!["Add unit test".into()];
        neg.rounds.push(Round {
            number: 1,
            proposer: "validator".into(),
            text: "raw feedback body".into(),
            verdict: Some(v.clone()),
        });
        let esc = EscalationRequired {
            reason: "1 NEEDS_FIX rounds exhausted without PASS".into(),
            phase_index: 0,
            phase_name: "negotiation-round-1".into(),
            rounds: neg.rounds.clone(),
            last_verdict: Some(v),
        };
        let written = write_escalation_artifacts(out.to_str().unwrap(), &neg, &esc);
        assert_eq!(written.len(), 3);
        assert_eq!(fs::read_to_string(&out).unwrap(), "# Draft 1\n");
        let fb = fs::read_to_string(dir.path().join("SPRINT.md.feedback.md")).unwrap();
        assert!(fb.contains("## Required fixes") && fb.contains("- Add unit test"));
        assert!(dir.path().join("SPRINT.md.rounds/01-round1-drafter.md").exists());
        assert!(dir.path().join("SPRINT.md.rounds/02-round1-validator.md").exists());
    }
}
