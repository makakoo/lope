//! Native handler for `lope implement` (`cli.py` + `implement.py`, Sprint S7, `FR35`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use lope_engine::auditor::Auditor;
use lope_engine::gates::{
    build_run, default_baseline_path, load_gate_specs, prompt_summary, run_gates,
};
use lope_engine::implement::{
    apply_phase_filter, render_dry_run, resolve_implement_roster, run_implement,
};
use lope_engine::models::{Phase, SprintDoc};
use lope_engine::trust::ensure_gates_trusted;
use lope_providers::pool::Pool;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn run(m: &ArgMatches) -> Native {
    let Some(doc_path) = scope::os_str(m, "sprint_doc") else {
        eprintln!("lope implement: missing sprint_doc");
        return Native::Exit(2);
    };
    let agents = scope::os_str(m, "--agents");
    let escalate_to = scope::os_str(m, "--escalate-to");
    let phase_filter = scope::os_str(m, "--phase");
    let use_gates = scope::flag(m, "--gates");
    let gate_config = scope::os_str(m, "--gate-config");
    let trust = scope::flag(m, "--trust");
    let dry_run = scope::flag(m, "--dry-run");
    let _interactive = scope::flag(m, "--interactive");

    let text = match fs::read_to_string(Path::new(&doc_path)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lope implement: cannot read {doc_path}: {e}");
            return Native::Exit(1);
        }
    };
    let mut doc = SprintDoc::from_markdown(&text, Some(&doc_path));
    if doc.phases.is_empty() {
        eprintln!("lope implement: no phases found in {doc_path}");
        return Native::Exit(1);
    }
    doc = match apply_phase_filter(&doc, phase_filter.as_deref()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("lope implement: {e}");
            return Native::Exit(2);
        }
    };

    let (cfg, pool) = match scope::resolve(m) {
        Ok(x) => x,
        Err(e) => return Native::Delegate(format!("implement: {e}")),
    };

    let avail = pool.names();
    let roster = match resolve_implement_roster(agents.as_deref(), escalate_to.as_deref(), &avail) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("lope implement: {e}");
            return Native::Exit(2);
        }
    };

    let timeout = cfg.timeout.as_f64().unwrap_or(960.0);
    if dry_run {
        println!("{}", render_dry_run(&doc, &roster, cfg.parallel, timeout));
        return Native::Exit(0);
    }

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
                        eprintln!("lope implement: gate commands not trusted (pass --trust)");
                        return Native::Exit(2);
                    }
                }
                (specs, Some(cfg_path))
            }
            Err(e) => {
                eprintln!("lope implement: {e}");
                return Native::Exit(2);
            }
        }
    } else {
        (Vec::new(), None)
    };

    let writers: Vec<_> = roster
        .implementers
        .iter()
        .filter_map(|name| pool.validators.iter().find(|v| v.name() == name).cloned())
        .collect();
    let escalators: Vec<_> = roster
        .escalations
        .iter()
        .filter_map(|name| pool.validators.iter().find(|v| v.name() == name).cloned())
        .collect();

    let esc_pool = Pool {
        validators: escalators,
        primary: roster.escalations.first().cloned().unwrap_or_default(),
        parallel: cfg.parallel,
        ensemble: true,
        unsupported: Vec::new(),
        warnings: Vec::new(),
    };

    let sc = match scope::start("implement", &cfg, false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lope implement: {e}");
            return Native::Exit(2);
        }
    };

    let ctx_ref = &sc.ctx;
    let base_path = default_baseline_path(Some(&cwd));
    let mut gate_cb = |_phase: &Phase| -> (bool, String) {
        let started = Instant::now();
        let res = run_gates(&gate_specs, Some(&cwd), 60, Some(ctx_ref));
        let run = build_run(
            "implement",
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

    let mut log_ev = |msg: &str| eprintln!("[implement] {msg}");
    let run_res = run_implement(
        &mut doc,
        &roster,
        &writers,
        esc_pool,
        3,
        timeout,
        gate_opt,
        Some(ctx_ref),
        Some(&mut log_ev),
    );
    match run_res {
        Ok(report) => {
            let auditor = Auditor::new(None);
            println!("{}", auditor.scorecard(&report));
            let _ = auditor.write_journal(&report);
            Native::Exit(scope::finish(sc, 0))
        }
        Err(esc) => {
            eprintln!(
                "Implement escalated on Phase {} ({}): {}",
                esc.phase_index, esc.phase_name, esc.reason
            );
            Native::Exit(scope::finish(sc, 1))
        }
    }
}
