//! S1 golden runs: native `status` / `jobs *` vs direct Python Lope
//! (tools/parity/corpus/s1-native.json; routing asserted per case).
#[test]
fn s1_native_status_and_jobs_match_python() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(root.join("tools/parity/golden_run.py"))
        .args(["--lope", env!("CARGO_BIN_EXE_lope")])
        .arg("--corpus")
        .arg(root.join("tools/parity/corpus/s1-native.json"))
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    println!("{report}{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "S1 parity mismatches:\n{report}");
}
