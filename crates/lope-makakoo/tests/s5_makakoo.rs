//! S5 acceptance tests for `lope-makakoo` (FR28, FR29, FR30, DEC-10, A13).

use lope_makakoo::{
    build_context_block, clean_brain_search_output, detect_makakoo, format_review_journal_line,
    prepend_context_once, query_brain, today_local_journal_filename, write_auto_memory,
    write_brain_journal, MakakooBridgeError, TopFindingSummary, ENV_AUTOMEMORY, ENV_BIN, ENV_HOME,
};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

#[test]
fn a13_brain_context_ansi_box_stripping_budget_and_prepend_once() {
    let dir = tempfile::tempdir().unwrap();
    let fake_bin = dir.path().join("fake-makakoo");
    fs::write(
        &fake_bin,
        r#"#!/bin/sh
if [ "$1" = "search" ]; then
  if [ "$2" = "empty-query" ]; then
    printf "\033[2m┌───────────┐\033[0m\n│ (no hits) │\n└───────────┘\n"
    exit 0
  fi
  printf "\033[1;36m┌────────┬──────────┬────────────────────────────────────────┐\033[0m\n"
  printf "│ doc_id │ source   │ snippet                                │\n"
  printf "├────────┼──────────┼────────────────────────────────────────┤\n"
  printf "│ 101    │ journal  │ Auth token sk-secret1234567890abcdef   │\n"
  printf "│ 102    │ page     │ Second line of context for budget test │\n"
  printf "└────────┴──────────┴────────────────────────────────────────┘\n"
  exit 0
fi
exit 0
"#,
    )
    .unwrap();
    fs::set_permissions(&fake_bin, fs::Permissions::from_mode(0o755)).unwrap();

    let mut env = HashMap::new();
    env.insert(
        ENV_BIN.to_string(),
        fake_bin.to_string_lossy().to_string(),
    );
    env.insert(
        ENV_HOME.to_string(),
        dir.path().to_string_lossy().to_string(),
    );

    // 1. ANSI + box table stripped and redacted.
    let res = query_brain("auth", 1200, 30, Some(&env)).unwrap();
    assert!(!res.contains('\u{1b}'), "ANSI escape remained: {res}");
    assert!(!res.contains('┌') && !res.contains('│'), "Box drawing remained: {res}");
    assert!(res.contains("101 journal Auth token sk-<redacted>"), "{res}");
    assert!(!res.contains("sk-secret1234567890abcdef"));

    // 2. Prepend once.
    let block = build_context_block("auth", &res);
    assert!(block.starts_with("<<< Makakoo Brain context (query: auth) >>>\n"));
    let p1 = prepend_context_once("Review this file.", &block);
    let p2 = prepend_context_once(&p1, &block);
    assert_eq!(p1, p2, "context block must be prepended at most once");

    // 3. (no hits) produces empty query_brain and no block.
    let empty_res = query_brain("empty-query", 1200, 30, Some(&env)).unwrap();
    assert!(empty_res.is_empty(), "expected empty for (no hits), got: {empty_res:?}");
    let empty_block = build_context_block("empty-query", &empty_res);
    assert!(empty_block.is_empty(), "expected no block for (no hits), got: {empty_block:?}");
    assert_eq!(
        prepend_context_once("Original prompt", &empty_block),
        "Original prompt"
    );

    // 4. Budget truncation cuts cleanly on newline boundary.
    let long_input = (0..30)
        .map(|i| format!("Line {i:02}: some brain knowledge entry that takes up space."))
        .collect::<Vec<_>>()
        .join("\n");
    let cleaned = clean_brain_search_output(&long_input);
    assert_eq!(cleaned.lines().count(), 30);
}

#[test]
fn a13_journal_local_date_locked_append_sync_file_and_unset_home() {
    let dir = tempfile::tempdir().unwrap();
    let sync_log = dir.path().join("sync-args.txt");
    let fake_bin = dir.path().join("fake-makakoo");
    fs::write(
        &fake_bin,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            sync_log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_bin, fs::Permissions::from_mode(0o755)).unwrap();

    // Unset MAKAKOO_HOME behaves as Python.
    let mut env_no_home = HashMap::new();
    env_no_home.insert(
        ENV_BIN.to_string(),
        fake_bin.to_string_lossy().to_string(),
    );
    let err = write_brain_journal("[[Lope]] test", Some(&env_no_home), None).unwrap_err();
    assert_eq!(
        err,
        MakakooBridgeError::NotDetected(
            "MAKAKOO_HOME is not set or not a directory; cannot read or write the Makakoo Brain"
                .into()
        )
    );

    // Valid MAKAKOO_HOME: local date, `- ` bullets, exclusive lock, `makakoo sync --file <journal>`.
    let mut env = env_no_home.clone();
    env.insert(
        ENV_HOME.to_string(),
        dir.path().to_string_lossy().to_string(),
    );

    let bullet = format_review_journal_line(
        "src/auth.rs",
        2,
        1,
        Some(&TopFindingSummary {
            file: "src/auth.rs".into(),
            line: Some(42),
            agreement: "3/3 validators".into(),
            score: Some(0.92),
            message: "SQL injection in query".into(),
        }),
        Some("abc123def456"),
    );
    let journal_path = write_brain_journal(&bullet, Some(&env), None).unwrap();
    assert_eq!(
        journal_path.file_name().unwrap().to_str().unwrap(),
        today_local_journal_filename()
    );
    let written = fs::read_to_string(&journal_path).unwrap();
    for line in written.lines().filter(|l| !l.trim().is_empty()) {
        assert!(line.starts_with("- "), "line did not start with '- ': {line}");
    }
    assert!(written.contains("[[Lope]] consensus review of `src/auth.rs`"));
    assert!(written.contains("[[Makakoo OS]]"));
    assert!(written.contains("Memory hash: `lope:abc123def456`"));

    let sync_recorded = fs::read_to_string(&sync_log).unwrap();
    assert_eq!(
        sync_recorded.trim(),
        format!("sync --file {}", journal_path.display())
    );
}

#[test]
fn fr30_auto_memory_gated_and_written() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = HashMap::new();
    env.insert(
        ENV_HOME.to_string(),
        dir.path().to_string_lossy().to_string(),
    );

    // Disabled by default.
    let err = write_auto_memory("auth-lesson", "Always parameterize SQL.", Some(&env)).unwrap_err();
    assert!(matches!(err, MakakooBridgeError::AutoMemoryDisabled(_)));

    // Enabled with LOPE_BRAIN_AUTOMEMORY=1.
    env.insert(ENV_AUTOMEMORY.to_string(), "1".to_string());
    let path = write_auto_memory("auth lesson!", "Always parameterize SQL.", Some(&env)).unwrap();
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), "lope-auth-lesson.md");
    assert_eq!(fs::read_to_string(&path).unwrap(), "Always parameterize SQL.\n");

    let det = detect_makakoo(Some(&env));
    assert_eq!(det.home.as_deref(), Some(dir.path()));
}
