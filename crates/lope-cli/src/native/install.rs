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
        if !src.trim().is_empty() {
            let p = PathBuf::from(src);
            if has_install_script(&p) {
                return Some(p);
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dirs = Vec::new();
        if let Some(d) = exe.parent() {
            dirs.push(d.to_path_buf());
        }
        if let Ok(canon) = std::fs::canonicalize(&exe) {
            if let Some(d) = canon.parent() {
                if !dirs.iter().any(|existing| existing == d) {
                    dirs.push(d.to_path_buf());
                }
            }
        }
        for dir in dirs {
            for cand in [dir.clone(), dir.join("../share/lope"), dir.join("../../.."), dir.join("../../../..")] {
                if has_install_script(&cand) {
                    return Some(cand);
                }
            }
        }
    }
    let home_assets = lope_core::paths::lope_home();
    if has_install_script(&home_assets) {
        return Some(home_assets);
    }
    None
}

pub fn run(m: &ArgMatches) -> Native {
    let host = scope::os_str(m, "--host").unwrap_or_else(|| "all".to_string());
    let Some(assets) = find_assets_root() else {
        eprintln!("lope install: cannot locate Lope skills/install assets (set LOPE_SOURCE_DIR)");
        return Native::Exit(1);
    };
    let script = assets.join("install");
    let mut cmd = Command::new("bash");
    cmd.arg(&script);
    if host != "all" {
        cmd.args(["--host", &host]);
    }
    cmd.env("LOPE_ENGINE_ROOT", &assets);
    if let Ok(exe) = std::env::current_exe() {
        cmd.env("LOPE_BIN", exe);
    }
    if std::env::var_os("LOPE_PACKAGED_INSTALL").is_none() {
        cmd.env("LOPE_PACKAGED_INSTALL", "1");
    }
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
