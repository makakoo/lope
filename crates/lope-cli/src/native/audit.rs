//! Native handler for `lope audit` (`cli.py`, Sprint S7, `FR34`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use lope_engine::auditor::Auditor;
use lope_engine::models::{ExecutionReport, PhaseVerdict, SprintDoc, VerdictStatus};
use std::fs;
use std::path::Path;

pub fn run(m: &ArgMatches) -> Native {
    let Some(doc_path) = scope::os_str(m, "sprint_doc") else {
        eprintln!("lope audit: missing sprint_doc");
        return Native::Exit(2);
    };
    let no_journal = scope::flag(m, "--no-journal");

    let text = match fs::read_to_string(Path::new(&doc_path)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lope audit: cannot read {doc_path}: {e}");
            return Native::Exit(1);
        }
    };
    let doc = SprintDoc::from_markdown(&text, Some(&doc_path));
    if doc.phases.is_empty() {
        eprintln!("lope audit: no phases found in {doc_path}");
        return Native::Exit(1);
    }

    let verdicts: Vec<PhaseVerdict> = doc
        .phases
        .iter()
        .map(|p| {
            p.verdict.clone().unwrap_or_else(|| {
                let mut v = PhaseVerdict::new(
                    VerdictStatus::Inconclusive,
                    "phase not validated",
                    0.0,
                    "audit",
                );
                v.confidence = 0.0;
                v
            })
        })
        .collect();

    let all_pass = verdicts.iter().all(|v| v.status == VerdictStatus::Pass);
    let report = ExecutionReport {
        sprint_doc: doc,
        phase_verdicts: verdicts,
        ok: all_pass,
        error: if all_pass {
            String::new()
        } else {
            "one or more phases incomplete or not PASS".to_string()
        },
        total_duration_seconds: 0.0,
    };

    let auditor = Auditor::new(None);
    println!("{}", auditor.scorecard(&report));
    if !no_journal {
        if let Err(e) = auditor.write_journal(&report) {
            eprintln!("lope audit: warning: failed to write journal: {e}");
        }
    }
    Native::Exit(if report.ok { 0 } else { 1 })
}
