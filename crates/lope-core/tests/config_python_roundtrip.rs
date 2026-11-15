//! A3: Rust config load+save is byte-identical to Python's save(); Python
//! loads every Rust-written config; rejections agree (tools/parity/s1_interop.py a3).
#[test]
fn a3_config_python_roundtrip() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(root.join("tools/parity/s1_interop.py"))
        .arg("a3")
        .current_dir(&root)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    println!("{report}{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "A3 failed:\n{report}");
}
