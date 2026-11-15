//! S3 golden runs: native `team list|enable|disable|remove` vs Python
//! (tools/parity/corpus/s3-team.json; config.json bytes compared).
#[test]
fn s3_team_matches_python() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(root.join("tools/parity/golden_run.py"))
        .args(["--lope", env!("CARGO_BIN_EXE_lope")])
        .arg("--corpus")
        .arg(root.join("tools/parity/corpus/s3-team.json"))
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    println!("{report}{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "S3 team parity mismatches:\n{report}");
}
