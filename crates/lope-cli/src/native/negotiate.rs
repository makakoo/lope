//! Native handler for `lope negotiate` (`cli.py`, Sprint S7, `FR32`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use lope_engine::negotiator::{
    build_user_prompt, negotiator_system_prompt, ConvergenceOutcome, Negotiator,
};
use lope_providers::pool::Pool;
use std::fs;
use std::path::Path;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn load_context(m: &ArgMatches) -> Result<String, String> {
    let mut parts = Vec::new();
    if let Some(cf) = scope::os_str(m, "--context-file") {
        let text = fs::read_to_string(Path::new(&cf))
            .map_err(|e| format!("cannot read --context-file {cf}: {e}"))?;
        if !text.trim().is_empty() {
            parts.push(format!("## Context file: {cf}\n\n{}", text.trim_end()));
        }
    }
    if let Some(inline) = scope::os_str(m, "--context") {
        if !inline.trim().is_empty() {
            parts.push(inline.trim().to_string());
        }
    }
    Ok(parts.join("\n\n"))
}

pub fn run(m: &ArgMatches) -> Native {
    let Some(goal) = scope::os_str(m, "goal") else {
        eprintln!("lope negotiate: missing goal");
        return Native::Exit(2);
    };
    let out_path = scope::os_str(m, "--out").unwrap_or_else(|| "SPRINT.md".to_string());
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

    let mut context = match load_context(m) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("lope negotiate: {e}");
            return Native::Exit(1);
        }
    };

    let brain_ctx = scope::os_str(m, "--brain-context");
    let brain_budget = scope::u64_of(m, "--brain-budget").map(|v| v as usize);
    let brain_log = scope::flag(m, "--brain-log");
    match lope_makakoo::cli_brain_context_block(brain_ctx.as_deref(), brain_budget) {
        Ok(Some(block)) => {
            if context.is_empty() {
                context = block;
            } else {
                context = format!("{block}\n\n{context}");
            }
        }
        Ok(None) => {}
        Err(code) => return Native::Exit(code),
    }

    let (cfg, pool) = match scope::resolve(m) {
        Ok(x) => x,
        Err(e) => return Native::Delegate(format!("negotiate: {e}")),
    };

    let timeout = cfg.timeout.as_f64().unwrap_or(960.0);
    let max_total = env_usize("LOPE_MAX_TOTAL_ROUNDS", 25);

    if !context.trim().is_empty() {
        let sys = negotiator_system_prompt(&domain);
        let usr = build_user_prompt(&goal, &context);
        let combined = format!("{sys}\n\n{usr}");
        eprintln!("Negotiate preflight:");
        eprintln!(
            "  Context payload: {} bytes, {} lines",
            context.len(),
            context.lines().count()
        );
        eprintln!(
            "  Generated drafter prompt: {} bytes, {} lines",
            combined.len(),
            combined.lines().count()
        );
        eprintln!("  Effective timeout: {timeout:.0}s per drafter/reviewer call");
    }

    let sc = match scope::start("negotiate", &cfg, false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lope negotiate: {e}");
            return Native::Exit(2);
        }
    };

    let pool_for_neg = Pool {
        validators: pool.validators.clone(),
        primary: pool.primary.clone(),
        parallel: pool.parallel,
        ensemble: pool.ensemble,
        unsupported: Vec::new(),
        warnings: Vec::new(),
    };
    let drafter_name = pool.primary.clone();
    let reviewer_names: Vec<String> = pool
        .validators
        .iter()
        .map(|v| v.name().to_string())
        .filter(|n| n != &drafter_name)
        .collect();
    let reviewers_display = if reviewer_names.is_empty() {
        drafter_name.clone()
    } else {
        reviewer_names.join(", ")
    };
    println!("\nLope negotiate: {goal}\n");
    println!("Drafter: {drafter_name}  ·  Reviewers: {reviewers_display}\n");

    let mut negotiator = Negotiator::new(pool_for_neg, max_rounds, timeout, max_total, &domain);
    let mut log_ev = |_: &str| {};
    let outcome = match negotiator.converge(&goal, &context, Some(&sc.ctx), Some(&mut log_ev)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("lope negotiate: {e}");
            return Native::Exit(scope::finish(sc, 1));
        }
    };

    match outcome {
        ConvergenceOutcome::Converged(doc) => {
            let md = doc.to_markdown();
            if let Err(e) = fs::write(&out_path, &md) {
                eprintln!("lope negotiate: failed writing {out_path}: {e}");
                return Native::Exit(scope::finish(sc, 1));
            }
            println!(
                "Negotiation converged -> {out_path} ({} phases)",
                doc.phases.len()
            );
            if let Some(ack) = lope_makakoo::cli_emit_brain_log(
                brain_log,
                &format!("- [[lope]] negotiated `{out_path}`: {goal}"),
            ) {
                println!("{ack}");
            }
            Native::Exit(scope::finish(sc, 0))
        }
        ConvergenceOutcome::Escalated(esc) => {
            println!("Negotiation escalated: {}", esc.reason);
            Native::Exit(scope::finish(sc, 1))
        }
    }
}
