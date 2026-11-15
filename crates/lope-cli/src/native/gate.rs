//! Native handlers for `lope gate {save,check}` and `lope check` (`cli.py`, Sprint S7, `FR34`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use lope_engine::gates::{
    build_run, compare_results, default_baseline_path, load_baseline, load_gate_specs, run_gates,
    save_baseline, GateRun,
};
use lope_engine::trust::ensure_gates_trusted;
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

    let sc = scope::start_gate_runtime("check", m).ok();
    let stop_progress = |sc: &Option<scope::Scope>| {
        if let Some(s) = sc {
            s.progress.stop();
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (specs, cfg_path) = match load_gate_specs(config.as_deref(), Some(&cwd)) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("lope check: {e}");
            stop_progress(&sc);
            return Native::Exit(2);
        }
    };
    let pairs: Vec<(&str, &str)> = specs
        .iter()
        .map(|s| (s.name.as_str(), s.cmd.as_str()))
        .collect();
    if !ensure_gates_trusted(&pairs, Some(&cwd), trust) {
        stop_progress(&sc);
        return Native::Exit(3);
    }

    let started = Instant::now();
    let results = run_gates(&specs, Some(&cwd), timeout, sc.as_ref().map(|s| &s.ctx));
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
            lope_core::pyjson::dumps(&run.to_value(), &lope_core::pyjson::Opts::pretty_sorted())
        );
    } else {
        print_gate_run(&run);
    }
    stop_progress(&sc);
    Native::Exit(if run.passed { 0 } else { 1 })
}

fn run_gate_save(m: &ArgMatches) -> Native {
    let config = scope::os_str(m, "--config");
    let baseline = scope::os_str(m, "--baseline");
    let timeout = scope::u64_of(m, "--timeout").unwrap_or(60);
    let as_json = scope::flag(m, "--json");
    let trust = scope::flag(m, "--trust");
    let remember = scope::flag(m, "--remember");

    let sc = scope::start_gate_runtime("gate", m).ok();
    let stop_progress = |sc: &Option<scope::Scope>| {
        if let Some(s) = sc {
            s.progress.stop();
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (specs, cfg_path) = match load_gate_specs(config.as_deref(), Some(&cwd)) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("lope gate: {e}");
            stop_progress(&sc);
            return Native::Exit(2);
        }
    };

    let pairs: Vec<(&str, &str)> = specs
        .iter()
        .map(|s| (s.name.as_str(), s.cmd.as_str()))
        .collect();
    if !ensure_gates_trusted(&pairs, Some(&cwd), trust) {
        stop_progress(&sc);
        return Native::Exit(3);
    }

    let started = Instant::now();
    let results = run_gates(&specs, Some(&cwd), timeout, sc.as_ref().map(|s| &s.ctx));
    let base_path = match save_baseline(&results, baseline.as_deref().map(Path::new), Some(&cwd)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("lope gate: {e}");
            stop_progress(&sc);
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
            lope_core::pyjson::dumps(&run.to_value(), &lope_core::pyjson::Opts::pretty_sorted())
        );
    } else {
        print_gate_run(&run);
    }
    stop_progress(&sc);
    Native::Exit(if run.passed { 0 } else { 1 })
}

fn run_gate_check(m: &ArgMatches) -> Native {
    let config = scope::os_str(m, "--config");
    let baseline = scope::os_str(m, "--baseline");
    let timeout = scope::u64_of(m, "--timeout").unwrap_or(60);
    let as_json = scope::flag(m, "--json");
    let trust = scope::flag(m, "--trust");
    let remember = scope::flag(m, "--remember");

    let sc = scope::start_gate_runtime("gate", m).ok();
    let stop_progress = |sc: &Option<scope::Scope>| {
        if let Some(s) = sc {
            s.progress.stop();
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (specs, cfg_path) = match load_gate_specs(config.as_deref(), Some(&cwd)) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("lope gate: {e}");
            stop_progress(&sc);
            return Native::Exit(2);
        }
    };

    let pairs: Vec<(&str, &str)> = specs
        .iter()
        .map(|s| (s.name.as_str(), s.cmd.as_str()))
        .collect();
    if !ensure_gates_trusted(&pairs, Some(&cwd), trust) {
        stop_progress(&sc);
        return Native::Exit(3);
    }

    let base_arg = baseline.as_deref().map(Path::new);
    let base_path = base_arg
        .map(PathBuf::from)
        .unwrap_or_else(|| default_baseline_path(Some(&cwd)));
    let before = if specs.is_empty() {
        None
    } else {
        match load_baseline(base_arg, Some(&cwd)) {
            Ok(x) => Some(x),
            Err(e) => {
                eprintln!("lope gate: {e}");
                stop_progress(&sc);
                return Native::Exit(2);
            }
        }
    };
    let started = Instant::now();
    let results = run_gates(&specs, Some(&cwd), timeout, sc.as_ref().map(|s| &s.ctx));
    let comparisons = match before {
        Some(ref b) => compare_results(&specs, b, &results),
        None => Vec::new(),
    };
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
            lope_core::pyjson::dumps(&run.to_value(), &lope_core::pyjson::Opts::pretty_sorted())
        );
    } else {
        print_gate_run(&run);
    }
    stop_progress(&sc);
    Native::Exit(if run.passed { 0 } else { 1 })
}

fn fmt_4g(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let sign = if v.is_sign_negative() { "-" } else { "" };
    let sci = format!("{:.3e}", v.abs());
    let Some((mant, exp_str)) = sci.split_once('e') else {
        return v.to_string();
    };
    let exp: i32 = exp_str.parse().unwrap_or(0);
    if (-4..4).contains(&exp) {
        let digits: String = mant.chars().filter(|c| *c != '.').collect();
        let mut s = if exp < 0 {
            let zeros = "0".repeat((-exp - 1) as usize);
            format!("0.{zeros}{digits}")
        } else {
            let split = (exp + 1) as usize;
            if split >= digits.len() {
                digits
            } else {
                format!("{}.{}", &digits[..split], &digits[split..])
            }
        };
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        format!("{sign}{s}")
    } else {
        let mut m = mant.to_string();
        if m.contains('.') {
            while m.ends_with('0') {
                m.pop();
            }
            if m.ends_with('.') {
                m.pop();
            }
        }
        format!("{sign}{m}e{exp:+03}")
    }
}

fn print_gate_run(run: &GateRun) {
    let verdict = if run.passed { "PASS" } else { "FAIL" };
    println!("\nLope {}: {verdict}", run.mode);
    println!("{}", "-".repeat(40));
    if !run.comparisons.is_empty() {
        for c in &run.comparisons {
            let status = if c.passed { "PASS" } else { "FAIL" };
            let req = if c.required { "" } else { " (optional)" };
            if let (Some(b), Some(a_val)) = (c.before.as_ref().and_then(|b| b.value), c.after.value)
            {
                println!(
                    "  {status:<4} {}{req}: {} -> {}  {}",
                    c.name,
                    fmt_4g(b),
                    fmt_4g(a_val),
                    c.reason
                );
            } else {
                println!("  {status:<4} {}{req}: {}", c.name, c.reason);
            }
        }
    } else {
        for r in &run.results {
            let status = if r.ok { "PASS" } else { "FAIL" };
            let req = if r.required { "" } else { " (optional)" };
            let value = r
                .value
                .map(|v| format!(" value={}", fmt_4g(v)))
                .unwrap_or_default();
            let reason = if r.ok {
                String::new()
            } else {
                let err = r
                    .error
                    .as_deref()
                    .filter(|e| !e.is_empty())
                    .unwrap_or("failed");
                format!(" \u{2014} {err}")
            };
            println!("  {status:<4} {}{req}{value}{reason}", r.name);
        }
    }
    println!("Baseline: {}", run.baseline_path);
    println!();
}

fn remember_gate_run(run: &GateRun, task: &str) {
    if let Ok(Some(mem)) = lope_memory::open_memory() {
        let failed = run.blocking_failures().len() as i64;
        let payload = run.to_value();
        let _ = mem.store_gate_session(
            task,
            &run.mode,
            &run.baseline_path,
            run.passed,
            run.results.len() as i64,
            failed,
            &payload,
        );
    }
}
