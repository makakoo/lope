//! Native handlers for `lope gate {save,check}` and `lope check` (`cli.py`, Sprint S7, `FR34`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use lope_engine::gates::{
    build_run, compare_results, default_baseline_path, load_baseline, load_gate_specs, run_gates,
    save_baseline, GateComparison, GateRun,
};
use lope_engine::trust::ensure_gates_trusted;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn run_gate(sub: Option<&str>, m: &ArgMatches) -> Native {
    match sub {
        Some("save") => run_gate_save(m),
        Some("check") => run_gate_check(m),
        _ => {
            eprintln!("usage: lope gate {{save,check}} ...");
            Native::Exit(2)
        }
    }
}

pub fn run_check(m: &ArgMatches) -> Native {
    let config = scope::os_str(m, "--config");
    let timeout = scope::u64_of(m, "--timeout").unwrap_or(60);
    let as_json = scope::flag(m, "--json");
    let trust = scope::flag(m, "--trust");
    let remember = scope::flag(m, "--remember");

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (specs, cfg_path) = match load_gate_specs(config.as_deref(), Some(&cwd)) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("lope check: {e}");
            return Native::Exit(2);
        }
    };
    if specs.is_empty() {
        eprintln!("lope check: no gates configured in {}", cfg_path.display());
        return Native::Exit(2);
    }

    let pairs: Vec<(&str, &str)> = specs
        .iter()
        .map(|s| (s.name.as_str(), s.cmd.as_str()))
        .collect();
    if !ensure_gates_trusted(&pairs, Some(&cwd), trust) {
        eprintln!("lope check: gate commands not trusted (pass --trust or LOPE_TRUST_GATES=1)");
        return Native::Exit(2);
    }

    let started = Instant::now();
    let results = run_gates(&specs, Some(&cwd), timeout, None);
    let base_path = default_baseline_path(Some(&cwd));
    let run = build_run(
        "check",
        &specs,
        Some(&cfg_path),
        &base_path,
        results,
        Vec::new(),
        started,
        Some(&cwd),
    );

    if remember {
        remember_gate_run(&run, "check");
    }
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&gate_run_json(&run)).unwrap_or_default()
        );
    } else {
        print_gate_run(&run);
    }
    Native::Exit(if run.passed { 0 } else { 1 })
}

fn run_gate_save(m: &ArgMatches) -> Native {
    let config = scope::os_str(m, "--config");
    let baseline = scope::os_str(m, "--baseline");
    let timeout = scope::u64_of(m, "--timeout").unwrap_or(60);
    let as_json = scope::flag(m, "--json");
    let trust = scope::flag(m, "--trust");
    let remember = scope::flag(m, "--remember");

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (specs, cfg_path) = match load_gate_specs(config.as_deref(), Some(&cwd)) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("lope gate save: {e}");
            return Native::Exit(2);
        }
    };
    if specs.is_empty() {
        eprintln!(
            "lope gate save: no gates configured in {}",
            cfg_path.display()
        );
        return Native::Exit(2);
    }

    let pairs: Vec<(&str, &str)> = specs
        .iter()
        .map(|s| (s.name.as_str(), s.cmd.as_str()))
        .collect();
    if !ensure_gates_trusted(&pairs, Some(&cwd), trust) {
        eprintln!("lope gate save: gate commands not trusted (pass --trust or LOPE_TRUST_GATES=1)");
        return Native::Exit(2);
    }

    let started = Instant::now();
    let results = run_gates(&specs, Some(&cwd), timeout, None);
    let base_path = match save_baseline(&results, baseline.as_deref().map(Path::new), Some(&cwd)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lope gate save: {e}");
            return Native::Exit(1);
        }
    };
    let run = build_run(
        "save",
        &specs,
        Some(&cfg_path),
        &base_path,
        results,
        Vec::new(),
        started,
        Some(&cwd),
    );
    if remember {
        remember_gate_run(&run, "gate save");
    }
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&gate_run_json(&run)).unwrap_or_default()
        );
    } else {
        print_gate_run(&run);
        println!("Saved baseline to {}", base_path.display());
    }
    Native::Exit(if run.passed { 0 } else { 1 })
}

fn run_gate_check(m: &ArgMatches) -> Native {
    let config = scope::os_str(m, "--config");
    let baseline = scope::os_str(m, "--baseline");
    let timeout = scope::u64_of(m, "--timeout").unwrap_or(60);
    let as_json = scope::flag(m, "--json");
    let trust = scope::flag(m, "--trust");
    let remember = scope::flag(m, "--remember");

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (specs, cfg_path) = match load_gate_specs(config.as_deref(), Some(&cwd)) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("lope gate check: {e}");
            return Native::Exit(2);
        }
    };
    if specs.is_empty() {
        eprintln!(
            "lope gate check: no gates configured in {}",
            cfg_path.display()
        );
        return Native::Exit(2);
    }

    let pairs: Vec<(&str, &str)> = specs
        .iter()
        .map(|s| (s.name.as_str(), s.cmd.as_str()))
        .collect();
    if !ensure_gates_trusted(&pairs, Some(&cwd), trust) {
        eprintln!("lope gate check: gate commands not trusted (pass --trust or LOPE_TRUST_GATES=1)");
        return Native::Exit(2);
    }

    let base_arg = baseline.as_deref().map(Path::new);
    let base_path = base_arg
        .map(PathBuf::from)
        .unwrap_or_else(|| default_baseline_path(Some(&cwd)));
    let before = match load_baseline(base_arg, Some(&cwd)) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("lope gate check: {e}");
            return Native::Exit(2);
        }
    };
    let started = Instant::now();
    let results = run_gates(&specs, Some(&cwd), timeout, None);
    let comparisons = compare_results(&specs, &before, &results);
    let run = build_run(
        "check",
        &specs,
        Some(&cfg_path),
        &base_path,
        results,
        comparisons,
        started,
        Some(&cwd),
    );
    if remember {
        remember_gate_run(&run, "gate check");
    }
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&gate_run_json(&run)).unwrap_or_default()
        );
    } else {
        print_gate_run(&run);
    }
    Native::Exit(if run.passed { 0 } else { 1 })
}

fn gate_run_json(run: &GateRun) -> serde_json::Value {
    json!({
        "passed": run.passed,
        "mode": run.mode,
        "project_root": run.project_root,
        "config_path": run.config_path,
        "baseline_path": run.baseline_path,
        "duration_ms": run.duration_ms,
        "results": run.results.iter().map(|r| r.to_value()).collect::<Vec<_>>(),
        "comparisons": run.comparisons.iter().map(|c: &GateComparison| json!({
            "name": c.name,
            "passed": c.passed,
            "required": c.required,
            "delta": c.delta,
            "reason": c.reason,
        })).collect::<Vec<_>>(),
    })
}

fn print_gate_run(run: &GateRun) {
    for r in &run.results {
        let mark = if r.ok { "PASS" } else { "FAIL" };
        let mut extra = Vec::new();
        if let Some(val) = r.value {
            extra.push(format!("value={val}"));
        }
        extra.push(format!("{}ms", r.duration_ms));
        if let Some(err) = &r.error {
            if !err.is_empty() {
                extra.push(err.clone());
            }
        }
        println!("[{mark}] {} ({})", r.name, extra.join(", "));
    }
    for c in &run.comparisons {
        if !c.passed {
            println!("[REGRESSION] {}: {}", c.name, c.reason);
        }
    }
}

fn remember_gate_run(run: &GateRun, mode: &str) {
    if let Ok(Some(mem)) = lope_memory::open_memory() {
        let failed = run.results.iter().filter(|r| !r.ok).count() as i64;
        let payload = gate_run_json(run);
        let _ = mem.store_gate_session(
            &run.project_root,
            mode,
            &run.baseline_path,
            run.passed,
            run.results.len() as i64,
            failed,
            &payload,
        );
    }
}
