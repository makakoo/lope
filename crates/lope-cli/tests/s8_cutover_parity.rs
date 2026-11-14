//! S8 acceptance & parity tests: FR37 (`install`), FR38 (`update`/`upgrade` delegated), FR39 (`docs`).

use std::process::Command;

#[test]
fn s8_docs_and_install_all_8_hosts_and_update_delegation() {
    let lope = env!("CARGO_BIN_EXE_lope");

    // 1. FR39: `lope docs` outputs complete reference.md natively
    let docs_out = Command::new(lope)
        .env("LOPE_PYTHON", "/nonexistent/python")
        .arg("docs")
        .output()
        .unwrap();
    assert!(docs_out.status.success(), "lope docs failed");
    let docs_txt = String::from_utf8_lossy(&docs_out.stdout);
    assert!(docs_txt.len() > 10_000, "docs output unexpectedly short");
    assert!(docs_txt.contains("lope"), "docs output missing lope reference");

    // 2. FR37: `lope install --host <h>` produces artifacts in a fresh HOME for all 8 hosts
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    for host in [
        "claude", "codex", "gemini", "opencode", "cursor", "vibe", "qwen", "pi",
    ] {
        let out = Command::new(lope)
            .env("HOME", home)
            .env("LOPE_SOURCE_DIR", "/Users/sebastian/.lope")
            .args(["install", "--host", host])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "lope install --host {host} failed:\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert!(home.join(".claude/skills/lope").exists());
    assert!(home.join(".agents/skills/lope").exists() || home.join(".codex").exists());
    assert!(home.join(".gemini/skills/lope").exists() || home.join(".gemini/commands").exists());

    // 3. FR38 (OQ-2): `update` and `upgrade` remain delegated to Python
    for verb in ["update", "upgrade"] {
        let out = Command::new(lope)
            .env("LOPE_PYTHON", "/nonexistent/python")
            .env("LOPE_ROUTER_TRACE", "1")
            .arg(verb)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(127));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("delegate (NoCoverage)"), "{verb}: {err}");
    }
}
