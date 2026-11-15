//! Native handler for `lope audit` (`cli.py`, Sprint S7, `FR34`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use lope_engine::auditor::Auditor;
use lope_engine::models::{ExecutionReport, SprintDoc};
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
    let report = ExecutionReport {
        sprint_doc: doc,
        phase_verdicts: Vec::new(),
        ok: true,
        error: String::new(),
        total_duration_seconds: 0.0,
    };

    let auditor = Auditor::new(None);
    println!("{}", auditor.scorecard(&report));
    if !no_journal {
        match auditor.write_journal(&report) {
            Ok(path) => println!("\nJournal written to: {}", path.display()),
            Err(e) => eprintln!("lope audit: warning: failed to write journal: {e}"),
        }
    }
    Native::Exit(0)
}
