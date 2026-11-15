//! Native handler for `lope execute` (`cli.py`, Sprint S7, `FR33`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use lope_engine::auditor::Auditor;
use lope_engine::executor::{ImplementationResult, PhaseExecutor};
use lope_engine::gates::{
    build_run, default_baseline_path, load_gate_specs, prompt_summary, run_gates,
};
use lope_engine::implement::apply_phase_filter;
use lope_engine::models::{Phase, PhaseVerdict, SprintDoc};
use lope_engine::trust::ensure_gates_trusted;
use lope_providers::invocation::invoke_generate;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn run(m: &ArgMatches) -> Native {
    let _run_lock = match lope_state::runlock::acquire("execute") {
        Ok(l) => l,
        Err(busy) => {
            eprint!("{}", busy.message);
            return Native::Exit(lope_state::runlock::EXIT_TEMPFAIL);
        }
    };
    let Some(doc_path) = scope::os_str(m, "sprint_doc") else {
        eprintln!("lope execute: missing sprint_doc");
        return Native::Exit(2);
    };
    let phase_filter = scope::u64_of(m, "--phase")
        .map(|n| n.to_string())
        .or_else(|| scope::os_str(m, "--phase"));
    let manual = scope::flag(m, "--manual");
    let use_gates = scope::flag(m, "--gates");
    let gate_config = scope::os_str(m, "--gate-config");
    let trust = scope::flag(m, "--trust");

    let text = match fs::read_to_string(Path::new(&doc_path)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lope execute: cannot read {doc_path}: {e}");
            return Native::Exit(1);
        }
    };
    let mut doc = SprintDoc::from_markdown(&text, Some(&doc_path));
    if doc.phases.is_empty() {
        eprintln!(
            "\nERROR: sprint doc at {doc_path} contains 0 phases.\n  \
             Each phase must start with a level-3 heading: `### Phase N: <name>`"
        );
        return Native::Exit(2);
    }
    doc = match apply_phase_filter(&doc, phase_filter.as_deref()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("lope execute: {e}");
            return Native::Exit(2);
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (gate_specs, gate_cfg_path) = if use_gates {
        match load_gate_specs(gate_config.as_deref(), Some(&cwd)) {
            Ok((specs, cfg_path)) => {
                if !specs.is_empty() {
                    let pairs: Vec<(&str, &str)> = specs
                        .iter()
                        .map(|s| (s.name.as_str(), s.cmd.as_str()))
                        .collect();
                    if !ensure_gates_trusted(&pairs, Some(&cwd), trust) {
                        eprintln!("lope execute: gate commands not trusted (pass --trust)");
                        return Native::Exit(3);
                    }
                }
                (specs, Some(cfg_path))
            }
            Err(e) => {
                eprintln!("lope execute: {e}");
                return Native::Exit(2);
            }
        }
    } else {
        (Vec::new(), None)
    };

    let (cfg, pool) = match scope::resolve(m) {
        Ok(x) => x,
        Err(e) => {
            drop(_run_lock);
            return Native::Delegate(format!("execute: {e}"));
        }
    };

    let sc = match scope::start("execute", &cfg, false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lope execute: {e}");
            return Native::Exit(2);
        }
    };

    let timeout = cfg.timeout.as_f64().unwrap_or(960.0);
    let primary = pool.primary_validator();
    let executor = PhaseExecutor::new(pool.clone(), 3, timeout);
    let ctx_ref = &sc.ctx;
    let mut writer_ctx = sc.ctx.clone();
    writer_ctx.implementation = true;
    let writer_ctx_ref = &writer_ctx;
    let mut healer = lope_engine::healer::SelfHealer::new();

    let mut impl_fn = |phase: &Phase, fix_context: Option<&PhaseVerdict>| -> ImplementationResult {
        if manual {
            println!(
                "\n=== Phase {}: {} ===\nGoal: {}",
                phase.index, phase.name, phase.goal
            );
            if let Some(v) = fix_context {
                println!("Fix requested: {}", v.rationale);
                for f in &v.required_fixes {
                    println!("  - {f}");
                }
            }
            print!(
                "Press Enter when Phase {} is ready for validation (or type 'abort'): ",
                phase.index
            );
            let _ = io::stdout().flush();
            let mut line = String::new();
            match io::stdin().lock().read_line(&mut line) {
                Ok(0) | Err(_) => {
                    eprintln!(
                        "\nERROR: --manual mode requires an interactive stdin. \
                         Drop --manual to run autonomously via the primary validator."
                    );
                    return ImplementationResult {
                        ok: false,
                        error: "__manual_eof__".to_string(),
                        ..Default::default()
                    };
                }
                Ok(_) => {}
            }
            if line.trim().eq_ignore_ascii_case("abort") {
                return ImplementationResult {
                    ok: false,
                    error: "aborted by user".to_string(),
                    ..Default::default()
                };
            }
            return ImplementationResult {
                ok: true,
                summary: format!("Manual completion of Phase {}: {}", phase.index, phase.name),
                writer: "manual".to_string(),
                ..Default::default()
            };
        }

        let Some(ref writer) = primary else {
            return ImplementationResult {
                ok: false,
                error: "no primary validator available to implement".to_string(),
                ..Default::default()
            };
        };
        let fixes = fix_context
            .map(|v| format!("\n\nFixes required:\n- {}", v.required_fixes.join("\n- ")))
            .unwrap_or_default();
        let prompt = format!(
            "Implement Phase {} ({}): {}{fixes}",
            phase.index, phase.name, phase.goal
        );
        let t0 = Instant::now();
        match invoke_generate(
            writer.as_ref(),
            &prompt,
            timeout,
            Some(writer_ctx_ref),
            Some("execute_impl"),
        ) {
            Ok(out) => ImplementationResult {
                ok: true,
                summary: out,
                writer: writer.name().to_string(),
                duration_seconds: t0.elapsed().as_secs_f64(),
                ..Default::default()
            },
            Err(e) => {
                let err_msg = e.to_string();
                if lope_providers::verdict::is_flag_error(&err_msg) {
                    if let Some(healed) = lope_engine::healer::try_self_heal_for_validator(
                        &mut healer,
                        writer.as_ref(),
                        &pool.validators,
                        &err_msg,
                        Some(writer_ctx_ref),
                    ) {
                        println!(
                            ">>> self-heal succeeded, retrying phase {} with learned adapter",
                            phase.index
                        );
                        match invoke_generate(
                            healed.as_ref(),
                            &prompt,
                            timeout,
                            Some(writer_ctx_ref),
                            Some("execute_impl"),
                        ) {
                            Ok(out) => {
                                return ImplementationResult {
                                    ok: true,
                                    summary: out,
                                    writer: writer.name().to_string(),
                                    duration_seconds: t0.elapsed().as_secs_f64(),
                                    ..Default::default()
                                };
                            }
                            Err(e2) => {
                                return ImplementationResult {
                                    ok: false,
                                    error: format!(
                                        "{} still failing after self-heal: {e2}",
                                        writer.name()
                                    ),
                                    writer: writer.name().to_string(),
                                    duration_seconds: t0.elapsed().as_secs_f64(),
                                    ..Default::default()
                                };
                            }
                        }
                    }
                    let head: String = err_msg.chars().take(400).collect();
                    return ImplementationResult {
                        ok: false,
                        error: format!(
                            "{} failed with a flag-surface error (upstream CLI likely renamed a flag):\n  {head}\nSet LOPE_SELF_HEAL=1 and re-run to attempt automatic adapter repair.",
                            writer.name()
                        ),
                        writer: writer.name().to_string(),
                        duration_seconds: t0.elapsed().as_secs_f64(),
                        ..Default::default()
                    };
                }
                ImplementationResult {
                    ok: false,
                    error: err_msg,
                    writer: writer.name().to_string(),
                    duration_seconds: t0.elapsed().as_secs_f64(),
                    ..Default::default()
                }
            }
        }
    };

    let base_path = default_baseline_path(Some(&cwd));
    let mut gate_cb = |_phase: &Phase| -> (bool, String) {
        let started = Instant::now();
        let res = run_gates(&gate_specs, Some(&cwd), 60, Some(ctx_ref));
        let run = build_run(
            "execute",
            &gate_specs,
            gate_cfg_path.as_deref(),
            &base_path,
            res,
            Vec::new(),
            started,
            Some(&cwd),
        );
        let summary = prompt_summary(&run);
        (run.passed, summary)
    };
    let gate_opt: Option<&mut dyn FnMut(&Phase) -> (bool, String)> = if gate_specs.is_empty() {
        None
    } else {
        Some(&mut gate_cb)
    };

    let mut log_ev = |msg: &str| eprintln!("[execute] {msg}");
    let run_res = executor.run(&mut doc, &mut impl_fn, gate_opt, Some(ctx_ref), Some(&mut log_ev));
    match run_res {
        Ok(report) => {
            let auditor = Auditor::new(None);
            println!("{}", auditor.scorecard(&report));
            let _ = auditor.write_journal(&report);
            Native::Exit(scope::finish(sc, 0))
        }
        Err(esc) => {
            if esc.reason.contains("__manual_eof__") {
                return Native::Exit(scope::finish(sc, 3));
            }
            eprintln!(
                "Execution escalated on Phase {} ({}): {}",
                esc.phase_index, esc.phase_name, esc.reason
            );
            Native::Exit(scope::finish(sc, 1))
        }
    }
}
