//! A2 (Spec 03 §4): for the S0 delegation corpus, `lope` (Rust router →
//! exec Python) must be indistinguishable from direct Python Lope: stdout,
//! exit code, normalised stderr and written state. Runs the golden-run
//! harness (tools/parity/golden_run.py) against the live Python checkout
//! ($LOPE_HOME or ~/.lope) with offline mock validators. Slow: every case
//! starts Python Lope twice.

use std::path::Path;
use std::process::Command;

#[test]
fn a2_delegation_corpus_matches_direct_python() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let harness = root.join("tools/parity/golden_run.py");
    let corpus = root.join("tools/parity/corpus/s0-delegation.json");
    let out = Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(&harness)
        .args(["--lope", env!("CARGO_BIN_EXE_lope")])
        .arg("--corpus")
        .arg(&corpus)
        .output()
        .expect("run golden_run.py with /usr/bin/python3");
    let report = String::from_utf8_lossy(&out.stdout);
    println!("{report}");
    eprintln!("{}", String::from_utf8_lossy(&out.stderr));
    assert!(
        out.status.success(),
        "A2 delegation corpus has mismatches:\n{report}"
    );
}
