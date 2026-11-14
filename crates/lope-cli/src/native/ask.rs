//! `lope ask` (`cli.py:_cmd_ask`) and `lope pipe` (`cli.py:_cmd_pipe`).

use super::fanout;
use super::scope::{self, flag, os_str};
use super::{Native, Out};
use clap::ArgMatches;
use lope_engine::synthesis::format_synthesis;
use lope_providers::request_plan::{plan_direct, Limits};

pub fn run(m: &ArgMatches) -> Native {
    let question = os_str(m, "question").unwrap_or_default();
    let mut prompt = question.clone();
    if let Some(c) = os_str(m, "--context").filter(|c| !c.is_empty()) {
        prompt = format!("{c}\n\n{prompt}");
    }
    simple_fanout(m, "ask", prompt, None)
}

/// `lope pipe` (`cli.py:_cmd_pipe`).
pub fn run_pipe(m: &ArgMatches) -> Native {
    if unsafe { libc::isatty(0) } == 1 {
        eprintln!("lope pipe: no stdin detected (run with a pipe, e.g. `echo 'Q' | lope pipe`)");
        return Native::Exit(2);
    }
    let mut buf = Vec::new();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut buf);
    let prompt = String::from_utf8_lossy(&buf).into_owned();
    if prompt.trim().is_empty() {
        eprintln!("lope pipe: stdin was empty");
        return Native::Exit(2);
    }
    // stdin is consumed: any hand-back replays it to Python.
    let require_all = flag(m, "--require-all");
    simple_fanout(m, "pipe", prompt, Some(require_all)).with_stdin(buf)
}

pub struct Prepared {
    pub cfg: lope_core::config::LopeCfg,
    pub pool: lope_providers::pool::Pool,
    pub scope: scope::Scope,
    pub json_mode: bool,
}

/// `_ensure_config` + `_plan_external_request` (DIRECT only) + runtime start.
pub fn prepare(m: &ArgMatches, mode: &str, prompt: &str) -> Result<Prepared, Native> {
    let (cfg, pool) = scope::resolve(m).map_err(Native::Delegate)?;
    let json_mode = flag(m, "--json");
    let lim = Limits {
        max_chunks: lope_core::config::LopeCfg::int_field(&cfg.max_chunks),
        max_calls: lope_core::config::LopeCfg::int_field(&cfg.max_calls),
        max_input_bytes: lope_core::config::LopeCfg::int_field(&cfg.max_input_bytes),
    };
    let timeout = cfg.timeout_secs() as f64;
    let extra_calls = usize::from(flag(m, "--synth"));
    let plan = plan_direct(
        prompt,
        mode,
        &pool.validators,
        &cfg.request_policy,
        &lim,
        timeout,
        pool.parallel,
        extra_calls,
    );
    if !plan.direct {
        return Err(Native::Delegate("request plan is not direct".into()));
    }
    for w in &pool.warnings {
        eprintln!("{w}");
    }
    let scope =
        scope::start(mode, &cfg, json_mode).map_err(|e| Native::Delegate(format!("runtime start failed: {e}")))?;
    let mut plan_json = plan.json.clone();
    let tv: Vec<(String, Option<i64>)> =
        pool.validators.iter().map(|v| (v.name().to_string(), v.timeout_override())).collect();
    let advice = lope_providers::latency::fit_warnings(&tv, Some(cfg.timeout_secs()));
    let mut exec_profiles = Vec::new();
    if mode == "review" {
        for v in &pool.validators {
            if let Some(ep) = v.execution_profile(mode) {
                exec_profiles.push(ep);
            }
        }
    }
    plan_json["budget_advice"] = serde_json::json!(advice);
    plan_json["execution_profiles"] = serde_json::json!(exec_profiles);
    scope.ctx.add_request_plan(plan_json);
    if !json_mode {
        eprintln!(
            "Request plan: direct · {} bytes · 1 chunk(s) · {} call(s) · concurrency {} · nominal ceiling {}s",
            plan.utf8_bytes,
            plan.planned_calls,
            plan.maximum_concurrency,
            lope_providers::progress::g(plan.nominal)
        );
    }
    for l in &advice {
        eprintln!("{l}");
    }
    for ep in &exec_profiles {
        if let Some(ws) = ep.get("warnings").and_then(serde_json::Value::as_array) {
            for w in ws {
                if let Some(s) = w.as_str() {
                    eprintln!("{s}");
                }
            }
        }
    }
    Ok(Prepared { cfg, pool, scope, json_mode })
}

/// Shared body of ask / pipe (DIRECT plans).
fn simple_fanout(m: &ArgMatches, mode: &str, prompt: String, require_all: Option<bool>) -> Native {
    let Prepared { cfg, pool, scope, json_mode } = match prepare(m, mode, &prompt) {
        Ok(p) => p,
        Err(n) => return n,
    };
    let timeout = cfg.timeout_secs() as f64;
    let mut out = Out::new();
    let names = pool.names();
    if !json_mode {
        let (src, n) = if mode == "pipe" { (prompt.trim(), 80) } else { (prompt.as_str(), 100) };
        let preview: String = lope_providers::pyre::head(src, n).replace('\n', " ");
        let dots = if prompt.chars().count() > n { "..." } else { "" };
        out.line(format!("\nLope {mode}: {preview}{dots}"));
        out.line(format!("Validators: {}", names.join(", ")));
        out.line(format!("Timeout: {}s per validator\n", lope_core::config::py_str(&cfg.timeout)));
        out.flush();
    }
    let rows = fanout::fanout_generate(&pool.validators, pool.parallel, &prompt, timeout, &scope.ctx);
    if rows.is_empty() {
        eprintln!("No validators available. Run: lope status");
        drop(out);
        return Native::Exit(scope::finish(scope, 1));
    }
    let (ok, reason) = fanout::quorum(&rows, &names);
    if !ok {
        scope.ctx.budget.mark_partial(&reason);
    }
    let synth = if ok {
        fanout::maybe_synthesize(m, &cfg, &pool, &scope.ctx, &rows, &prompt, None, false)
    } else {
        None
    };
    if json_mode {
        let mut payload = scope.ctx.budget.snapshot();
        payload["responses"] = fanout::payload("answer", &rows);
        if let Some(s) = &synth {
            payload["synthesis"] = fanout::synth_json_value(s);
        }
        out.line(lope_core::output::json_text(&payload));
    } else {
        fanout::render(&rows, &mut out);
        if let Some(s) = &synth {
            out.line(format_synthesis(s, false));
        }
    }
    drop(out);
    let mut code = 0;
    if !ok {
        if !json_mode {
            eprintln!("{reason}");
        }
        code = 1;
    } else if require_all == Some(true) && rows.iter().any(|r| r.2.is_some()) {
        code = 1;
    }
    Native::Exit(scope::finish(scope, code))
}
