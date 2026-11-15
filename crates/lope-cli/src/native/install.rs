//! Native handler for `lope install` (`cli.py`, Sprint S8, `FR37`).

use crate::native::{scope, Native};
use clap::ArgMatches;
use std::path::{Path, PathBuf};
use std::process::Command;

fn has_install_script(dir: &Path) -> bool {
    dir.join("install").is_file() && dir.join("skills").is_dir()
}

pub fn find_assets_root() -> Option<PathBuf> {
    if let Ok(src) = std::env::var("LOPE_SOURCE_DIR") {
        let p = PathBuf::from(src);
        if has_install_script(&p) {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [
                dir.to_path_buf(),
                dir.join("../share/lope"),
                dir.join("../../.."),
            ] {
                if has_install_script(&cand) {
                    return Some(cand);
                }
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join(".lope");
        if has_install_script(&p) {
            return Some(p);
        }
    }
    let fallback = PathBuf::from("/Users/sebastian/.lope");
    if has_install_script(&fallback) {
        return Some(fallback);
    }
    None
}

pub fn run(m: &ArgMatches) -> Native {
    let host = scope::os_str(m, "--host").unwrap_or_else(|| "all".to_string());
    let Some(assets) = find_assets_root() else {
        eprintln!("lope install: cannot locate Lope skills/install assets");
        return Native::Exit(1);
    };
    let script = assets.join("install");
    let mut cmd = Command::new("bash");
    cmd.arg(&script);
    if host != "all" {
        cmd.args(["--host", &host]);
    }
    cmd.env("LOPE_ENGINE_ROOT", &assets);
    cmd.env("LOPE_PACKAGED_INSTALL", "0");
    let py = std::env::var("LOPE_PYTHON").unwrap_or_else(|_| "python3".to_string());
    cmd.env("LOPE_PYTHON", py);
    match cmd.status() {
        Ok(st) => Native::Exit(st.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("Install failed: cannot execute {}: {e}", script.display());
            Native::Exit(1)
        }
    }
}
