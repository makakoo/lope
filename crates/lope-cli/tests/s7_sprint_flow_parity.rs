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
    assert_eq!(
        untrusted.status.code(),
        Some(3),
        "G7: untrusted gate must fail closed with exit 3"
    );

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
    assert!(
        audit_out.status.success(),
        "audit exits 0 on valid sprint doc even when phases are unrun"
    );
    let audit_txt = String::from_utf8_lossy(&audit_out.stdout);
    assert!(audit_txt.contains("Sprint: SPRINT-T"));
    assert!(audit_txt.contains("Overall: OK"));
}

#[test]
fn s7_execute_manual_eof_exits_3() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    let lope = env!("CARGO_BIN_EXE_lope");
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock-val");
    fs::write(&mock, "#!/bin/sh\ncat >/dev/null\necho 'VERDICT: PASS'\n").unwrap();
    fs::set_permissions(&mock, fs::Permissions::from_mode(0o755)).unwrap();
    let lope_home = tmp.path().join("home");
    fs::create_dir_all(&lope_home).unwrap();
    fs::write(
        lope_home.join("config.json"),
        format!(
            r#"{{"version":1,"validators":["mock"],"primary":"mock","timeout":30,"parallel":false,"providers":[{{"name":"mock","type":"subprocess","command":["{}"],"stdin":true,"timeout":30}}]}}"#,
            mock.display()
        ),
    )
    .unwrap();
    let sprint_md = tmp.path().join("SPRINT.md");
    fs::write(
        &sprint_md,
        "# SPRINT-M\n\n## Origin\nM\n\n## Phases\n\n### Phase 1: p1\n**Goal:** G\n",
    )
    .unwrap();

    let out = Command::new(lope)
        .current_dir(tmp.path())
        .env("LOPE_HOME", &lope_home)
        .args(["execute", sprint_md.to_str().unwrap(), "--manual"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "manual EOF must exit 3; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("--manual mode requires an interactive stdin"),
        "missing EOF message: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn s7_codex_writer_gets_workspace_write_while_validator_stays_readonly() {
    use std::os::unix::fs::PermissionsExt;

    let lope = env!("CARGO_BIN_EXE_lope");
    let tmp = tempfile::tempdir().unwrap();
    let root_dir = fs::canonicalize(tmp.path()).unwrap();
    let bin_dir = root_dir.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let log_path = root_dir.join("codex.log");
    let fake_codex = bin_dir.join("codex");
    fs::write(
        &fake_codex,
        format!(
            "#!/bin/sh\ncat >/dev/null\necho \"ARGS:$* DEPTH:${{LOPE_IMPLEMENTATION_DEPTH:-unset}}\" >> \"{}\"\ncat <<'EOF'\n---VERDICT---\nstatus: PASS\nconfidence: 0.95\nrationale: verified in src/lib.rs:10 (cargo test passed)\n---END---\nEOF\n",
            log_path.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o755)).unwrap();

    let lope_home = root_dir.join("home");
    fs::create_dir_all(&lope_home).unwrap();
    fs::write(
        lope_home.join("config.json"),
        r#"{"version":1,"validators":["codex"],"primary":"codex","timeout":30,"parallel":false,"providers":[]}"#,
    )
    .unwrap();

    let sprint_md = root_dir.join("SPRINT.md");
    fs::write(
        &sprint_md,
        "# SPRINT-C\n\n## Origin\nC\n\n## Phases\n\n### Phase 1: p1\n**Goal:** G\n",
    )
    .unwrap();

    let path_env = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin_dir.display());
    let out = Command::new(lope)
        .current_dir(&root_dir)
        .env("LOPE_HOME", &lope_home)
        .env("LOPE_SELF_HEAL", "0")
        .env("PATH", path_env)
        .env_remove("LOPE_IMPLEMENTATION_DEPTH")
        .args([
            "implement",
            sprint_md.to_str().unwrap(),
            "--agents",
            "codex",
            "--escalate-to",
            "codex",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "implement with fake codex failed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let log = fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert!(
        lines.len() >= 2,
        "expected writer + validator codex calls, got:\n{log}"
    );
    assert!(
        lines[0].contains("-s workspace-write") && lines[0].contains("DEPTH:1"),
        "writer call must pass workspace-write and DEPTH:1, got: {}",
        lines[0]
    );
    assert!(
        lines[1..].iter().all(|l| l.contains("-s read-only") && l.contains("DEPTH:unset")),
        "validator calls must stay read-only without depth increment, got:\n{log}"
    );
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

#[test]
fn s7_self_heal_cli_retry_end_to_end() {
    use std::os::unix::fs::PermissionsExt;

    let lope = env!("CARGO_BIN_EXE_lope");
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let lope_home = root.join("home");
    fs::create_dir_all(&lope_home).unwrap();

    let broken_bin = root.join("broken-cli");
    fs::write(
        &broken_bin,
        r#"#!/bin/sh
if [ "$1" = "--help" ]; then
  echo "Usage: broken-cli --ask <prompt>"
  exit 0
fi
if [ "$1" = "--bad-flag" ]; then
  echo "error: unrecognized argument '--bad-flag'" >&2
  exit 2
fi
if [ "$1" = "--ask" ]; then
  case "$2" in
    *"Reply with the single word OK"*) echo "OK" ;;
    *) printf -- "Implemented.\n---VERDICT---\nstatus: PASS\nconfidence: 0.95\nrationale: checked src/lib.rs:1 (tests pass)\n---END---\n" ;;
  esac
  exit 0
fi
exit 1
"#,
    )
    .unwrap();
    fs::set_permissions(&broken_bin, fs::Permissions::from_mode(0o755)).unwrap();

    let reviewer_bin = root.join("rev-mock");
    fs::write(
        &reviewer_bin,
        format!(
            r#"#!/bin/sh
if printf '%s' "$*" | grep -q "argv_template"; then
  printf '```json\n{{"argv_template":["{}","--ask","{{prompt}}"],"stdin_mode":"none","stdout_parser":"plaintext","confidence":0.95,"rationale":"use --ask"}}\n```\n'
else
  printf -- "---VERDICT---\nstatus: PASS\nconfidence: 0.95\nrationale: verified src/lib.rs:1 (tests passed)\n---END---\n"
fi
"#,
            broken_bin.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&reviewer_bin, fs::Permissions::from_mode(0o755)).unwrap();

    fs::write(
        lope_home.join("config.json"),
        format!(
            r#"{{"version":1,"validators":["broken","mock-rev"],"primary":"broken","timeout":30,"parallel":false,"providers":[{{"name":"broken","type":"subprocess","command":["{}","--bad-flag","{{prompt}}"],"timeout":30}},{{"name":"mock-rev","type":"subprocess","command":["{}","{{prompt}}"],"timeout":30}}]}}"#,
            broken_bin.display(),
            reviewer_bin.display()
        ),
    )
    .unwrap();

    let sprint_md = root.join("SPRINT.md");
    fs::write(
        &sprint_md,
        "# SPRINT-HEAL\n\n## Origin\nHeal\n\n## Phases\n\n### Phase 1: fix\n**Goal:** Repair\n",
    )
    .unwrap();

    let out = Command::new(lope)
        .current_dir(&root)
        .env("LOPE_HOME", &lope_home)
        .env("LOPE_SELF_HEAL", "1")
        .env("LOPE_EVIDENCE_GATE", "off")
        .env("LOPE_MINIMALITY", "off")
        .args(["execute", sprint_md.to_str().unwrap()])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "lope execute with LOPE_SELF_HEAL=1 failed:\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        stdout.contains("self-heal succeeded"),
        "expected self-heal retry banner in stdout:\n{stdout}"
    );
    let cfg_txt = fs::read_to_string(lope_home.join("config.json")).unwrap();
    assert!(
        cfg_txt.contains("learned_adapters") && cfg_txt.contains("\"broken\""),
        "config.json should persist learned_adapters.broken:\n{cfg_txt}"
    );
    let journal_txt = fs::read_to_string(lope_home.join("journal.jsonl")).unwrap();
    assert!(
        journal_txt.contains("heal_success"),
        "journal.jsonl should log heal_success:\n{journal_txt}"
    );
}
