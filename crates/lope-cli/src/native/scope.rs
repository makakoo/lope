//! `_ensure_config` + `_ensure_runtime` + `_command_runtime_scope` for
//! native fan-out verbs.

use clap::ArgMatches;
use lope_core::config::{self, CliOverrides, LopeCfg};
use lope_providers::budget::{Context, RunBudget};
use lope_providers::pool::{self, Pool};
use lope_providers::progress::{format_runtime_summary, Progress};
use lope_state::registry::RunRegistry;
use std::sync::{Arc, Mutex};

pub fn flag(m: &ArgMatches, id: &str) -> bool {
    m.try_get_one::<bool>(id).ok().flatten().copied().unwrap_or(false)
}

pub fn os_str(m: &ArgMatches, id: &str) -> Option<String> {
    m.try_get_one::<std::ffi::OsString>(id).ok().flatten().map(|s| s.to_string_lossy().into_owned())
}

fn u64_of(m: &ArgMatches, id: &str) -> Option<i64> {
    m.try_get_one::<u64>(id).ok().flatten().map(|v| *v as i64)
}

/// CLI overrides exactly as `_ensure_config` extracts them.
pub fn overrides(m: &ArgMatches) -> CliOverrides {
    let mut o = CliOverrides::default();
    if let Some(v) = os_str(m, "--validators").filter(|v| !v.is_empty()) {
        o.validators = Some(v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect());
    }
    o.primary = os_str(m, "--primary").filter(|v| !v.is_empty());
    o.timeout = u64_of(m, "--timeout");
    // --parallel/--sequential share dest `parallel` (default None).
    if m.value_source("--parallel") == Some(clap::parser::ValueSource::CommandLine) {
        o.parallel = Some(true);
    }
    if m.value_source("--sequential") == Some(clap::parser::ValueSource::CommandLine) {
        o.parallel = Some(false);
    }
    o.run_timeout = u64_of(m, "--run-timeout");
    if flag(m, "--allow-unbounded-run") {
        o.allow_unbounded_run = Some(true);
    }
    o.max_calls = u64_of(m, "--max-calls");
    o.max_chunks = u64_of(m, "--max-chunks");
    o.request_policy = m.try_get_one::<String>("--request-policy").ok().flatten().cloned();
    o
}

/// Resolve config + pool; None means "delegate" (first-run UX, unsupported
/// providers, ...), with the reason.
pub fn resolve(m: &ArgMatches) -> Result<(LopeCfg, Pool), String> {
    if flag(m, "--respect-provider-timeout") {
        std::env::set_var("LOPE_RESPECT_PROVIDER_TIMEOUT", "1");
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let cfg = config::load_layered(&cwd, &|k| std::env::var(k).ok(), &overrides(m));
    if cfg.validators.is_empty() {
        return Err("no validators configured (first-run flow)".into());
    }
    let pool = pool::build(&cfg);
    if !pool.unsupported.is_empty() {
        return Err(pool.unsupported.join("; "));
    }
    Ok((cfg, pool))
}

pub struct Scope {
    pub ctx: Context,
    pub registry: Arc<RunRegistry>,
    pub progress: Arc<Progress>,
    pub machine_output: bool,
}

/// `_ensure_runtime`: budget, reconcile, start_run, progress.
pub fn start(mode: &str, cfg: &LopeCfg, machine_output: bool) -> Result<Scope, String> {
    let budget = Arc::new(RunBudget::new(
        mode,
        cfg.run_timeout_secs().map(|v| v as f64),
        cfg.allow_unbounded_run,
        LopeCfg::int_field(&cfg.max_calls) as usize,
        LopeCfg::int_field(&cfg.max_input_bytes) as usize,
        LopeCfg::int_field(&cfg.max_output_bytes) as usize,
    ));
    let registry = Arc::new(RunRegistry::open_default().map_err(|e| e.to_string())?);
    let _ = registry.reconcile(false);
    registry
        .start_run(mode, Some(&budget.run_id), budget.deadline.map(|d| d - budget.started_at), None)
        .map_err(|e| e.0)?;
    let progress = Progress::new(Arc::clone(&budget), Some(Arc::clone(&registry)));
    progress.start();
    let ctx = Context {
        budget,
        mode: mode.into(),
        stage: "provider".into(),
        validator: String::new(),
        registry: Some(Arc::clone(&registry)),
        progress: Some(Arc::clone(&progress)),
        call_id: None,
        implementation: false,
        cleanup_result: Arc::new(Mutex::new(None)),
        owned_paths: Arc::new(Mutex::new(vec![])),
    };
    Ok(Scope { ctx, registry, progress, machine_output })
}

/// The `finally:` of `_command_runtime_scope`; returns the final exit code.
pub fn finish(s: Scope, code: i32) -> i32 {
    let failed = code != 0;
    let mut code = code;
    if failed && !matches!(code, 2 | 3 | 75 | 124) {
        let b = &s.ctx.budget;
        let reason = b.snapshot()["reason"].as_str().unwrap_or("").to_string();
        if (b.deadline.is_some() && b.remaining(false) <= 0.0) || reason.starts_with("run_budget_exhausted") {
            b.mark_partial("run_budget_exhausted: command deadline reached");
            code = 124;
        }
    }
    s.progress.stop();
    let snap = s.ctx.budget.snapshot();
    if !s.machine_output {
        eprintln!("{}", format_runtime_summary(&snap));
    }
    let partial = failed || snap["partial"].as_bool().unwrap_or(false);
    let state = if failed {
        "failed"
    } else if partial {
        "partial"
    } else {
        "completed"
    };
    let reason = match snap["reason"].as_str() {
        Some(r) if !r.is_empty() => r.to_string(),
        _ if failed => "command failed".into(),
        _ => String::new(),
    };
    match s.registry.close_run(s.ctx.run_id(), state, &reason) {
        Ok(close) => {
            let _ = s.registry.reconcile(false);
            if close["action"] == "cleanup_failed" {
                let r = lope_core::config::py_str(&close["reason"]);
                s.ctx.budget.mark_partial(&r);
                eprintln!("lope runtime closeout warning: {r}; inspect: lope jobs list");
                if !failed {
                    return 1;
                }
            }
        }
        Err(e) => {
            eprintln!("lope runtime closeout warning: {e}");
            if !failed {
                return 1;
            }
        }
    }
    code
}
