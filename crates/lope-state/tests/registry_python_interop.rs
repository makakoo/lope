//! A4: Rust and Python list, reap, prune and close each other's run
//! manifests identically (tools/parity/s1_interop.py a4).
#[test]
fn a4_registry_python_interop() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("/usr/bin/python3")
        .arg("-I")
        .arg(root.join("tools/parity/s1_interop.py"))
        .arg("a4")
        .current_dir(&root)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&out.stdout);
    println!("{report}{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "A4 failed:\n{report}");
}
