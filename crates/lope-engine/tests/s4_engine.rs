//! S4 unit and integration tests for `lope-engine` (FR24, FR25, FR26, A10).

use lope_engine::caveman::get_directive;
use lope_engine::divide::{
    assign_roles, build_role_prompt, pack_file_chunks, pack_hunk_chunks, parse_roles,
    split_diff_hunks, split_files, DEFAULT_MAX_CHARS, DEFAULT_MAX_FILE_BYTES,
};
use lope_engine::findings::{
    format_consensus_markdown, merge_findings, parse_findings, score_consensus, ConsensusLevel,
};
use lope_engine::minimality::{implementation_directive, resolve_review_focus, validator_rubric};
use lope_engine::review::{build_consensus_report, render_report};
use lope_engine::sarif;
use lope_engine::synthesis::{
    build_synthesis_prompt, format_synthesis, DEFAULT_SOURCE_BYTE_LIMIT,
    DEFAULT_TOTAL_PROMPT_BYTE_LIMIT,
};
use lope_engine::verdict_events::{classify_parse_error, record_verdict_event, ValidatorResult};
use lope_engine::verdict_repair::{
    build_repair_prompt, evaluate_repair_reply, parse_verdict_block, ParseErrorCategory,
    RepairStatus, VerdictStatus,
};
use lope_memory::LopeMemory;
use serde_json::Value;

#[test]
fn a10_findings_parse_merge_and_score_consensus() {
    let r1 = parse_findings(
        "- [HIGH] src/auth.py:42 — SQL injection in login query (confidence: 0.9)\n- [LOW] src/auth.py:10 — Unused import os (confidence: 0.7)\n",
        "claude",
        Some("src/auth.py"),
    );
    let r2 = parse_findings(
        "- [HIGH] src/auth.py:43 — SQL injection in login query (confidence: 0.85)\n",
        "codex",
        Some("src/auth.py"),
    );
    assert_eq!(r1.findings.len(), 2);
    assert_eq!(r2.findings.len(), 1);

    let mut all = r1.findings;
    all.extend(r2.findings);
    let merged = merge_findings(&all, 0.85);
    assert_eq!(merged.len(), 2);

    let roster = vec!["claude".to_string(), "codex".to_string(), "gemini".to_string()];
    let scored = score_consensus(&merged, &roster);
    assert_eq!(scored.len(), 2);
    assert_eq!(scored[0].agreement_count, 2);
    assert_eq!(scored[0].total_validators, 3);
    assert_eq!(scored[0].consensus_level, ConsensusLevel::Confirmed);

    let md = format_consensus_markdown(&scored, Some("src/auth.py"));
    assert!(md.contains("SQL injection in login query"));
    assert!(md.contains("CONFIRMED"));
}

#[test]
fn a10_sarif_validates_2_1_0_schema() {
    let raw = vec![
        (
            "claude".to_string(),
            "- [CRITICAL] src/db.rs:99 — Unchecked SQL string interpolation (confidence: 0.95)".to_string(),
            None,
        ),
        (
            "codex".to_string(),
            "- [CRITICAL] src/db.rs:99 — Unchecked SQL string interpolation (confidence: 0.90)".to_string(),
            None,
        ),
    ];
    let roster = vec!["claude".to_string(), "codex".to_string()];
    let report = build_consensus_report("src/db.rs", "security", &roster, &raw, 0.85, 0.0, Some("src/db.rs"));
    assert_eq!(report.merged_count, 1);
    assert_eq!(report.scored[0].consensus_level, ConsensusLevel::Confirmed);

    for fmt in ["text", "json", "markdown", "markdown-pr", "sarif"] {
        let rendered = render_report(&report, fmt, true).expect(fmt);
        assert!(!rendered.is_empty(), "{fmt} rendered empty");
        if fmt == "sarif" {
            let doc: Value = serde_json::from_str(&rendered).expect("valid sarif json");
            assert_eq!(doc["version"], "2.1.0");
            assert_eq!(doc["$schema"], sarif::SARIF_SCHEMA);
            let runs = doc["runs"].as_array().expect("runs array");
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0]["tool"]["driver"]["name"], "lope");
            let results = runs[0]["results"].as_array().expect("results array");
            assert_eq!(results.len(), 1);
            assert_eq!(results[0]["level"], "error");
        }
    }
}

#[test]
fn fr25_verdict_repair_and_events() {
    let broken = "Here is my review without a valid verdict block.\n";
    let v = parse_verdict_block(broken, "claude", 1.25);
    assert_eq!(v.status, VerdictStatus::InfraError);
    let vr = ValidatorResult {
        validator_name: "claude".into(),
        verdict: v.clone(),
        raw_response: broken.into(),
        error: "no verdict block".into(),
        flag_error_hint: String::new(),
        repair_attempted: false,
        repair_status: None,
        repaired_response: String::new(),
        initial_parse_status: None,
        parse_error_category: None,
    };
    assert_eq!(
        classify_parse_error(&vr),
        Some(ParseErrorCategory::MissingVerdictBlock)
    );

    let prompt = build_repair_prompt(broken, "claude").unwrap();
    assert!(prompt.contains("VERDICT:"));

    let reply = "VERDICT: PASS (confidence=0.9, 1.25s)\nRATIONALE:\nLooks good.\nREQUIRED_FIXES:\n- None";
    let outcome = evaluate_repair_reply(reply, "claude", 1.25);
    assert_eq!(outcome.status, RepairStatus::Accepted);
    assert_eq!(outcome.verdict.unwrap().status, VerdictStatus::Pass);

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("mem.db");
    let mem = LopeMemory::open(&db).unwrap();
    std::env::set_var("LOPE_VERDICT_EVENTS", "1");
    let ok = record_verdict_event(
        &vr,
        "prompt text",
        "run-1",
        "gate-1",
        "1.0",
        "",
        "",
        Some(1.25),
        Some(0),
        true,
        Some(RepairStatus::Accepted),
        reply,
        Some(VerdictStatus::InfraError),
        Some(&mem),
    );
    assert!(ok);
}

#[test]
fn fr26_caveman_and_minimality_and_divide_and_synthesis() {
    std::env::set_var("LOPE_CAVEMAN", "lite");
    assert!(!get_directive().is_empty());
    std::env::set_var("LOPE_CAVEMAN", "full");
    assert!(!get_directive().is_empty());
    assert!(resolve_review_focus(Some("minimality")).contains("over-engineering"));
    assert!(!implementation_directive(Some("on"), Some("engineering")).is_empty());
    assert!(!validator_rubric(Some("on"), Some("quality"), Some("engineering")).is_empty());

    let roles = parse_roles("sec,perf").unwrap();
    assert_eq!(roles.len(), 2);
    let assigned = assign_roles(&["v1".into(), "v2".into(), "v3".into()], &roles).unwrap();
    assert_eq!(assigned["v1"].name, "security");
    assert_eq!(assigned["v2"].name, "performance");
    assert_eq!(assigned["v3"].name, "security");
    assert!(build_role_prompt(&roles[0], "base").contains("SECURITY"));

    let diff = "diff --git a/x.py b/x.py\n--- a/x.py\n+++ b/x.py\n@@ -1,2 +1,3 @@\n a = 1\n+b = 2\n c = 3\n";
    let hunks = split_diff_hunks(diff);
    assert_eq!(hunks.len(), 1);
    let packed_hunks = pack_hunk_chunks(&hunks, DEFAULT_MAX_CHARS);
    assert_eq!(packed_hunks.len(), 1);

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.py"), "x = 1\n").unwrap();
    let (file_chunks, skipped) = split_files(dir.path(), DEFAULT_MAX_CHARS, DEFAULT_MAX_FILE_BYTES).unwrap();
    assert_eq!(file_chunks.len(), 1);
    assert!(skipped.is_empty());
    let packed_files = pack_file_chunks(&file_chunks, DEFAULT_MAX_CHARS);
    assert_eq!(packed_files.len(), 1);

    let mut trunc = Vec::new();
    let sp = build_synthesis_prompt(
        "Review x.py",
        &[("claude".into(), "Looks good".into(), None)],
        None,
        true,
        DEFAULT_SOURCE_BYTE_LIMIT,
        DEFAULT_TOTAL_PROMPT_BYTE_LIMIT,
        &mut trunc,
    );
    assert!(sp.contains("Response A"));
    assert!(!sp.contains("claude"));
    let formatted = format_synthesis(
        &lope_engine::synthesis::SynthesisResult {
            ok: true,
            text: "## Executive Summary\nAll good.".into(),
            primary: "claude".into(),
            error: String::new(),
            truncations: vec![],
        },
        false,
    );
    assert!(formatted.contains("Executive Summary"));
}
