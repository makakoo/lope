#![allow(dead_code)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn setup() {
    std::env::set_var("LOPE_SUPERVISOR_EXE", env!("CARGO_BIN_EXE_lope-supervisor"));
}

pub fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

pub fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

pub fn read_pid(path: &Path) -> i32 {
    for _ in 0..1500 {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(p) = s.trim().parse() {
                return p;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("no pid in {}", path.display());
}
