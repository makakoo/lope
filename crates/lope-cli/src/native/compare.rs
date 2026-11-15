//! `lope compare` (`cli.py:_cmd_compare`), DIRECT plans only.

use super::ask::{prepare, Prepared};
use super::fanout::{self, err_text, fanout_generate};
use super::scope::{self, os_str};
use super::vote::{parse_vote, winner};
use super::{Native, Out};
use clap::ArgMatches;
use lope_core::pyjson::{dumps, Opts};
use lope_engine::synthesis::format_synthesis;
use serde_json::{json, Value};

pub fn run(m: &ArgMatches) -> Native {
    let fa = os_str(m, "file_a").unwrap_or_default();
    let fb = os_str(m, "file_b").unwrap_or_default();
    // Python's str(Path(x)) normalises "a//b" and "./a"; delegate unusual paths.
    let norm =
        |p: &str| p.contains("//") || p.starts_with("./") || p.contains("/./") || (p.ends_with('/') && p.len() > 1);
    if norm(&fa) || norm(&fb) {
        return Native::Delegate("path needs pathlib normalisation".into());
    }
    for (label, f) in [("A", &fa), ("B", &fb)] {
        if !std::path::Path::new(f).is_file() {
            eprintln!("File {label} not found: {f}");
            return Native::Exit(1);
        }
    }
    let read = |p: &str| std::fs::read(p).map(|b| String::from_utf8_lossy(&b).into_owned());
    let (ca, cb) = match (read(&fa), read(&fb)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return Native::Delegate("unreadable file (Python error text)".into()),
    };
    let crit_raw = os_str(m, "--criteria").unwrap_or_default();
    let criteria =
        if crit_raw.trim().is_empty() { "correctness and clarity".to_string() } else { crit_raw.trim().to_string() };
    let prompt = format!(
        "Compare two files and pick the better one against explicit criteria: {criteria}.\n\n━━━ File A ({fa}) ━━━\n```\n{ca}\n```\n\n━━━ File B ({fb}) ━━━\n```\n{cb}\n```\n\nReply with ONLY the letter A or B. No explanation. No prose. Just: A or B."
    );
    let Prepared { cfg, pool, scope, json_mode } = match prepare(m, "compare", &prompt) {
        Ok(p) => p,
        Err(n) => return n,
    };
    let mut out = Out::new();
    if !json_mode {
        out.line("\nLope compare:");
        out.line(format!("  A: {fa}  ({} chars)", ca.chars().count()));
        out.line(format!("  B: {fb}  ({} chars)", cb.chars().count()));
        out.line(format!("  Criteria: {criteria}"));
        out.line(format!("  Validators: {}", pool.names().join(", ")));
        out.line(format!("  Timeout: {}s per validator\n", lope_core::config::py_str(&cfg.timeout)));
        out.flush();
    }
    let rows = fanout_generate(&pool.validators, pool.parallel, &prompt, cfg.timeout_secs() as f64, &scope.ctx);
    if rows.is_empty() {
        eprintln!("No validators available. Run: lope status");
        drop(out);
        return Native::Exit(scope::finish(scope, 1));
    }
    let opts = vec!["A".to_string(), "B".to_string()];
    let mut tally: Vec<(String, i64)> = vec![("A".into(), 0), ("B".into(), 0)];
    let (mut errored, mut unparseable) = (0, 0);
    let mut picks: Vec<(String, Option<String>, String, Option<String>)> = vec![];
    for (name, answer, err) in &rows {
        if let Some(e) = err {
            errored += 1;
            picks.push((name.clone(), None, answer.clone(), Some(err_text(e))));
            continue;
        }
        let c = parse_vote(answer, &opts);
        match &c {
            Some(x) => tally.iter_mut().find(|t| &t.0 == x).unwrap().1 += 1,
            None => unparseable += 1,
        }
        picks.push((name.clone(), c, answer.clone(), None));
    }
    let mut expected = pool.names();
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
        let task = format!(
            "Compare {fa} (A) vs {fb} (B) against criteria: {criteria}\nTally: A={}  B={}",
            tally[0].1, tally[1].1
        );
        fanout::maybe_synthesize(m, &cfg, &pool, &scope.ctx, &rows, &task, None, false)
    } else {
        None
    };
    let win = winner(&tally);
    if json_mode {
        let mut p = serde_json::Map::new();
        p.insert("file_a".into(), json!(fa));
        p.insert("file_b".into(), json!(fb));
        p.insert("criteria".into(), json!(criteria));
        p.insert("tally".into(), json!({"A": tally[0].1, "B": tally[1].1}));
        p.insert("winner".into(), json!(win));
        p.insert(
            "picks".into(),
            Value::Array(
                picks.iter().map(|(n, c, r, e)| json!({"validator": n, "chose": c, "raw": r, "error": e})).collect(),
            ),
        );
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
            out.line(format!("  [UNPARSEABLE] {}", lope_providers::pyre::head(&raw.trim().replace('\n', " "), 120)));
        }
    }
    out.blank();
    out.line(format!("Tally:  A={}  B={}", tally[0].1, tally[1].1));
    if errored > 0 || unparseable > 0 {
        out.line(format!("  (errored: {errored}, unparseable: {unparseable})"));
    }
    out.blank();
    match win.as_deref() {
        Some("A") => out.line(format!("Winner: A  ({fa})")),
        Some("B") => out.line(format!("Winner: B  ({fb})")),
        _ => out.line("No winner — tie."),
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
