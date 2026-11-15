//! S6 golden runs: native `deliberate` vs direct Python Lope
//! (`tools/parity/corpus/s6-deliberate.json`; A14).
#[test]
fn s6_deliberate_matches_python() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(root.join("tools/parity/golden_run.py"))
        .args(["--lope", env!("CARGO_BIN_EXE_lope")])
        .arg("--corpus")
        .arg(root.join("tools/parity/corpus/s6-deliberate.json"))
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    println!("{report}{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "S6 parity mismatches:\n{report}");
}
