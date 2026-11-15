//! `lope vote` (`cli.py:_cmd_vote`, `_parse_vote`, `_choice_quorum`, `_vote_winner`).

use super::ask::{prepare, Prepared};
use super::fanout::{self, err_text, fanout_generate};
use super::scope::{self, os_str};
use super::{Native, Out};
use clap::ArgMatches;
use lope_core::pyjson::{dumps, Opts};
use lope_engine::synthesis::format_synthesis;
use serde_json::{json, Map, Value};

/// `_parse_vote`: longest option first, whole-token, case-insensitive.
pub fn parse_vote(raw: &str, options: &[String]) -> Option<String> {
    let text = raw.trim();
    let mut sorted: Vec<&String> = options.iter().collect();
    sorted.sort_by_key(|o| std::cmp::Reverse(o.chars().count())); // stable, like Python
    for opt in sorted {
        let pat = format!("(?i)(?<![A-Za-z0-9_]){}(?![A-Za-z0-9_])", fancy_escape(opt));
        if lope_providers::pyre::is_match(&lope_providers::pyre::re(&pat), text) {
            return Some(opt.clone());
        }
    }
    None
}

fn fancy_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    out
}

pub fn winner(tally: &[(String, i64)]) -> Option<String> {
    let max = tally.iter().map(|t| t.1).max()?;
    if max == 0 {
        return None;
    }
    let w: Vec<&(String, i64)> = tally.iter().filter(|t| t.1 == max).collect();
    (w.len() == 1).then(|| w[0].0.clone())
}

pub fn run(m: &ArgMatches) -> Native {
    let options: Vec<String> = os_str(m, "--options")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if options.len() < 2 {
        eprintln!("--options needs at least 2 comma-separated labels");
        return Native::Exit(2);
    }
    let mut lower: Vec<String> = options.iter().map(|o| o.to_lowercase()).collect();
    lower.sort();
    lower.dedup();
    if lower.len() != options.len() {
        eprintln!("--options labels must be unique (case-insensitive)");
        return Native::Exit(2);
    }
    let question = os_str(m, "prompt").unwrap_or_default();
    let prompt = format!(
        "{question}\n\nOptions (pick EXACTLY one):\n{}\n\nReply with ONLY the option label. No explanation. No prose. Just one of: {}",
        options.iter().map(|o| format!("  - {o}")).collect::<Vec<_>>().join("\n"),
        options.join(", ")
    );
    let Prepared { cfg, pool, scope, json_mode } = match prepare(m, "vote", &prompt) {
        Ok(p) => p,
        Err(n) => return n,
    };
    let mut out = Out::new();
    if !json_mode {
        let dots = if question.chars().count() > 80 { "..." } else { "" };
        out.line(format!("\nLope vote: {}{dots}", lope_providers::pyre::head(&question, 80)));
        out.line(format!("Options: {}", options.join(", ")));
        out.line(format!("Validators: {}", pool.names().join(", ")));
        out.line(format!("Timeout: {}s per validator\n", lope_core::config::py_str(&cfg.timeout)));
        out.flush();
    }
    let rows = fanout_generate(&pool.validators, pool.parallel, &prompt, cfg.timeout_secs() as f64, &scope.ctx);
    if rows.is_empty() {
        eprintln!("No validators available. Run: lope status");
        drop(out);
        return Native::Exit(scope::finish(scope, 1));
    }
    let mut tally: Vec<(String, i64)> = options.iter().map(|o| (o.clone(), 0)).collect();
    let (mut unparseable, mut errored) = (0, 0);
    let mut picks: Vec<(String, Option<String>, String, Option<String>)> = vec![];
    for (name, answer, err) in &rows {
        if let Some(e) = err {
            errored += 1;
            picks.push((name.clone(), None, answer.clone(), Some(err_text(e))));
            continue;
        }
        let chosen = parse_vote(answer, &options);
        match &chosen {
            Some(c) => tally.iter_mut().find(|t| &t.0 == c).unwrap().1 += 1,
            None => unparseable += 1,
        }
        picks.push((name.clone(), chosen, answer.clone(), None));
    }
    let mut expected: Vec<String> = pool.names();
    expected.sort();
    expected.dedup();
    let mut valid: Vec<&str> = picks.iter().filter(|p| p.1.is_some() && p.3.is_none()).map(|p| p.0.as_str()).collect();
    valid.sort();
    valid.dedup();
    let required = expected.len() / 2 + 1;
    let ok = valid.len() >= required;
    let reason = if ok {
        String::new()
    } else {
        format!(
            "inconclusive: parseable-choice quorum {}/{required} not met from {} validators",
            valid.len(),
            expected.len()
        )
    };
    if !ok {
        scope.ctx.budget.mark_partial(&reason);
    }
    let synth = if ok {
        let tally_str = tally.iter().map(|(o, n)| format!("{o}={n}")).collect::<Vec<_>>().join(", ");
        let task = format!("Vote on: {question}\nOptions: {}\nTally: {tally_str}", options.join(", "));
        fanout::maybe_synthesize(m, &cfg, &pool, &scope.ctx, &rows, &task, None, false)
    } else {
        None
    };
    let win = winner(&tally);
    if json_mode {
        let mut p = Map::new();
        p.insert("prompt".into(), json!(question));
        p.insert("options".into(), json!(options));
        p.insert("tally".into(), Value::Object(tally.iter().map(|(k, v)| (k.clone(), json!(v))).collect()));
        p.insert("winner".into(), json!(win));
        p.insert(
            "picks".into(),
            Value::Array(
                picks.iter().map(|(n, c, r, e)| json!({"validator": n, "chose": c, "raw": r, "error": e})).collect(),
            ),
        );
        p.insert("unparseable".into(), json!(unparseable));
        p.insert("errored".into(), json!(errored));
        if let Some(s) = &synth {
            p.insert("synthesis".into(), fanout::synth_json_value(s));
        }
        p.insert("runtime".into(), scope.ctx.budget.snapshot());
        if !ok {
            p.insert("partial".into(), json!(true));
            p.insert("reason".into(), json!(reason));
        }
        out.line(dumps(&Value::Object(p), &Opts::pretty()));
        drop(out);
        return Native::Exit(scope::finish(scope, i32::from(!ok)));
    }
    for (name, chosen, raw, err) in &picks {
        out.line(format!("━━━ {name} ━━━"));
        if let Some(e) = err {
            out.line(format!("[ERROR] {e}"));
        } else if let Some(c) = chosen {
            out.line(format!("  chose: {c}"));
        } else {
            let preview = raw.trim().replace('\n', " ");
            out.line(format!("  [UNPARSEABLE — no option label found] {}", lope_providers::pyre::head(&preview, 120)));
        }
    }
    out.blank();
    out.line("Tally:");
    for (opt, n) in &tally {
        out.line(format!("  {} {n:>2}  {}", lope_core::output::ljust_str(opt, 20), "█".repeat(*n as usize)));
    }
    if errored > 0 || unparseable > 0 {
        out.line(format!("  (errored: {errored}, unparseable: {unparseable})"));
    }
    out.blank();
    match &win {
        Some(w) => out.line(format!("Winner: {w}")),
        None => out.line("No winner — tie or no votes. See tally above."),
    }
    if let Some(s) = &synth {
        out.blank();
        out.line(format_synthesis(s, false));
    }
    drop(out);
    if !ok {
        eprintln!("{reason}");
    }
    Native::Exit(scope::finish(scope, i32::from(!ok)))
}
