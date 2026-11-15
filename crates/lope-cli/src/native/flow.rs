//! Native handler for `lope flow {list,init,validate,render,run}` (`cli.py`, Sprint S7, `FR36`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use lope_engine::auditor::Auditor;
use lope_flow::{
    flow_report_to_execution_report, forecast_graph, get_template, load_flow_graph, render_file,
    template_names, validate_graph, FlowRunner,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn run(sub: Option<&str>, m: &ArgMatches) -> Native {
    match sub {
        Some("list") => run_list(),
        Some("init") => run_init(m),
        Some("validate") => run_validate(m),
        Some("render") => run_render(m),
        Some("run") => run_run(m),
        _ => {
            eprintln!("usage: lope flow {{run,validate,render,init,list}} ...");
            Native::Exit(2)
        }
    }
}

fn run_list() -> Native {
    println!("Bundled flow templates:");
    for name in template_names() {
        println!("  {name}");
    }
    println!("\nCreate one with: lope flow init <name>");
    Native::Exit(0)
}

fn run_init(m: &ArgMatches) -> Native {
    let name = m
        .try_get_one::<String>("template")
        .ok()
        .flatten()
        .cloned()
        .or_else(|| scope::os_str(m, "template"))
        .unwrap_or_else(|| "consensus".to_string());
    let text = match get_template(&name) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return Native::Exit(2);
        }
    };
    let out = scope::os_str(m, "--out").unwrap_or_else(|| format!(".lope/flow/{name}.dot"));
    let out_path = Path::new(&out);
    if out_path.exists() {
        eprintln!("refusing to overwrite existing file: {out}");
        return Native::Exit(1);
    }
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    if let Err(e) = fs::write(out_path, text) {
        eprintln!("lope flow init: cannot write {out}: {e}");
        return Native::Exit(1);
    }
    println!("Wrote {out}");
    println!("Next: lope flow run {out} --task \"<your goal>\"");
    Native::Exit(0)
}

fn run_validate(m: &ArgMatches) -> Native {
    let Some(wf) = scope::os_str(m, "workflow") else {
        eprintln!("lope flow validate: missing workflow path");
        return Native::Exit(2);
    };
    let graph = match load_flow_graph(Path::new(&wf)) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("INVALID: {e}");
            return Native::Exit(1);
        }
    };
    let warnings = match validate_graph(&graph) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("INVALID: {e}");
            return Native::Exit(1);
        }
    };
    let cap = graph.max_node_visits().unwrap_or(50);
    println!(
        "OK: {} — {} nodes, {} edges, runaway cap {}",
        graph.name,
        graph.nodes.len(),
        graph.edges.len(),
        cap
    );
    let cwd = std::env::current_dir().unwrap_or_default();
    let cfg = lope_core::config::load_layered(
        &cwd,
        &|k| std::env::var(k).ok(),
        &lope_core::config::CliOverrides::default(),
    );
    let pool = lope_providers::pool::build(&cfg);
    let forecast = forecast_graph(&graph, &pool.validators, &cfg);
    let deadline_str = match forecast["run_timeout_seconds"].as_f64() {
        Some(rt) => format!("{rt}"),
        None => "unbounded".to_string(),
    };
    println!(
        "Model calls: {} theoretical · cap {} · chunks {} · deadline {}s",
        forecast["theoretical_model_calls"],
        forecast["max_model_calls"],
        forecast["static_prompt_chunks"],
        deadline_str
    );
    for w in &warnings {
        println!("  warning: {w}");
    }
    if let Some(arr) = forecast["warnings"].as_array() {
        for w in arr {
            if let Some(s) = w.as_str() {
                println!("  warning: {s}");
            }
        }
    }
    Native::Exit(0)
}

fn run_render(m: &ArgMatches) -> Native {
    let Some(wf) = scope::os_str(m, "workflow") else {
        eprintln!("lope flow render: missing workflow path");
        return Native::Exit(2);
    };
    let out = scope::os_str(m, "--out");
    let fmt = scope::os_str(m, "--format").unwrap_or_else(|| "svg".to_string());
    match render_file(Path::new(&wf), out.as_deref().map(Path::new), &fmt) {
        Ok(msg) => {
            println!("{msg}");
            Native::Exit(0)
        }
        Err(e) => {
            eprintln!("lope flow render: {e}");
            Native::Exit(1)
        }
    }
}

fn run_run(m: &ArgMatches) -> Native {
    let Some(wf) = scope::os_str(m, "workflow") else {
        eprintln!("lope flow run: missing workflow path");
        return Native::Exit(2);
    };
    let task = scope::os_str(m, "--task").unwrap_or_default();
    let cwd = scope::os_str(m, "--cwd").map(PathBuf::from);
    let out_path = scope::os_str(m, "--out");
    let max_node_visits = scope::u64_of(m, "--max-node-visits").map(|v| v as usize);
    let no_journal = scope::flag(m, "--no-journal");
    let dry_run = scope::flag(m, "--dry-run");

    let graph = match load_flow_graph(Path::new(&wf)) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("lope flow run: {e}");
            return Native::Exit(1);
        }
    };
    let warnings = match validate_graph(&graph) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("lope flow run: {e}");
            return Native::Exit(1);
        }
    };
    for w in &warnings {
        eprintln!("WARNING: {w}");
    }

    let (cfg, pool) = match scope::resolve(m) {
        Ok(x) => x,
        Err(e) => return Native::Delegate(format!("flow run: {e}")),
    };

    if dry_run {
        let fc = forecast_graph(&graph, &pool.validators, &cfg);
        println!("{}", serde_json::to_string_pretty(&fc).unwrap_or_default());
        return Native::Exit(0);
    }

    let sc = match scope::start("flow", &cfg, false) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lope flow run: {e}");
            return Native::Exit(2);
        }
    };

    let ctx_arc = Arc::new(sc.ctx.clone());
    let mut runner = match FlowRunner::new(graph, pool, cfg, cwd, Some(ctx_arc)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("lope flow run: {e}");
            return Native::Exit(scope::finish(sc, 1));
        }
    };
    if let Some(mv) = max_node_visits {
        runner.max_node_visits = mv;
    }

    let report = match runner.run(&task) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("lope flow run: {e}");
            return Native::Exit(scope::finish(sc, 1));
        }
    };

    println!("{}", report.scorecard());
    if let Some(path) = out_path {
        let json_str = serde_json::to_string_pretty(&report.to_json()).unwrap_or_default();
        if let Err(e) = fs::write(&path, json_str) {
            eprintln!("lope flow run: warning: failed writing {path}: {e}");
        }
    }
    if !no_journal {
        let exec_rep = flow_report_to_execution_report(&report);
        let _ = Auditor::new(None).write_journal(&exec_rep);
    }
    let exit_code = if report.ok { 0 } else { 1 };
    Native::Exit(scope::finish(sc, exit_code))
}
