//! S7 acceptance & parity tests: A11 (G7, G8), A15 (negotiate/execute/implement/audit), A16 (flow).

use std::fs;
use std::process::Command;

#[test]
fn s7_cli_surface_and_a11_a15_a16() {
    let lope = env!("CARGO_BIN_EXE_lope");

    // 1. A16: flow list + init + validate + render -T dot + run (script-only)
    let tmp = tempfile::tempdir().unwrap();
    let list_out = Command::new(lope).args(["flow", "list"]).output().unwrap();
    assert!(list_out.status.success());
    let list_txt = String::from_utf8_lossy(&list_out.stdout);
    assert!(list_txt.contains("consensus"));
    assert!(list_txt.contains("judge-loop"));
    assert!(list_txt.contains("review-gate"));

    let dot_path = tmp.path().join("demo.dot");
    fs::write(
        &dot_path,
        r#"digraph demo {
  Start [type="start"]
  Gate [type="script", cmd="echo hello-flow"]
  Exit [type="exit"]
  Start -> Gate -> Exit
}
"#,
    )
    .unwrap();

    let val_out = Command::new(lope)
        .args(["flow", "validate", dot_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(val_out.status.success());

    let ren_out = Command::new(lope)
        .args(["flow", "render", dot_path.to_str().unwrap(), "-T", "dot"])
        .output()
        .unwrap();
    assert!(ren_out.status.success());
    assert!(String::from_utf8_lossy(&ren_out.stdout).contains("digraph demo"));

    // 2. A11 (G7/G8): check without trust fails closed in non-TTY, with --trust passes & saves baseline
    let rules_dir = tmp.path().join(".lope");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(
        rules_dir.join("rules.json"),
        r#"{"gates":[{"name":"num","cmd":"echo score=10","type":"regex_number","regex":"score=(\\d+)","required":true}]}"#,
    )
    .unwrap();

    let untrusted = Command::new(lope)
        .current_dir(tmp.path())
        .env("LOPE_HOME", tmp.path().join("home"))
        .args(["check"])
        .output()
        .unwrap();
    assert!(!untrusted.status.success(), "G7: untrusted gate must fail closed");

    let trusted = Command::new(lope)
        .current_dir(tmp.path())
        .env("LOPE_HOME", tmp.path().join("home"))
        .args(["gate", "save", "--trust", "--json"])
        .output()
        .unwrap();
    assert!(trusted.status.success());

    let check_after = Command::new(lope)
        .current_dir(tmp.path())
        .env("LOPE_HOME", tmp.path().join("home"))
        .args(["gate", "check", "--json"])
        .output()
        .unwrap();
    assert!(check_after.status.success());

    // 3. A15: audit on incomplete SPRINT.md reports cleanly
    let sprint_md = tmp.path().join("SPRINT.md");
    fs::write(
        &sprint_md,
        "# SPRINT-T\n\n## Origin\nT\n\n## Phases\n\n### Phase 1: p1\n**Goal:** G\n",
    )
    .unwrap();
    let audit_out = Command::new(lope)
        .current_dir(tmp.path())
        .env("LOPE_HOME", tmp.path().join("home"))
        .args(["audit", sprint_md.to_str().unwrap(), "--no-journal"])
        .output()
        .unwrap();
    assert!(!audit_out.status.success());
    assert!(String::from_utf8_lossy(&audit_out.stdout).contains("Sprint: SPRINT-T"));
}

#[test]
fn s7_golden_parity_matches_python() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(root.join("tools/parity/golden_run.py"))
        .args(["--lope", env!("CARGO_BIN_EXE_lope")])
        .arg("--corpus")
        .arg(root.join("tools/parity/corpus/s7-sprint-flow.json"))
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    println!("{report}{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "S7 parity mismatches:\n{report}");
}
