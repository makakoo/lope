//! Delegation to Python Lope (FR5, Spec 01 §5 step 4).
//!
//! Equivalent to the current `/usr/local/bin/lope` shim:
//! `PYTHONPATH="${LOPE_HOME:-$HOME/.lope}" exec python3 -m lope "$@"`.
//! `exec` replaces this process, so env, cwd, stdio, signals and the exit
//! status are Python's by construction.

use lope_core::{exit, paths};
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

pub const SYSTEM_PYTHON: &str = "/usr/bin/python3";

/// Interpreter: `LOPE_PYTHON`, then `/usr/bin/python3`, then `python3` on PATH.
pub fn interpreter() -> OsString {
    interpreter_from(
        std::env::var_os("LOPE_PYTHON"),
        Path::new(SYSTEM_PYTHON).is_file(),
    )
}

pub fn interpreter_from(lope_python: Option<OsString>, system_python_exists: bool) -> OsString {
    match lope_python.filter(|v| !v.is_empty()) {
        Some(p) => p,
        None if system_python_exists => OsString::from(SYSTEM_PYTHON),
        None => OsString::from("python3"),
    }
}

/// The command that delegation execs (also used by tests to spawn instead).
pub fn command(args: &[OsString]) -> Command {
    let mut cmd = Command::new(interpreter());
    cmd.arg("-m")
        .arg("lope")
        .args(args)
        .env("PYTHONPATH", paths::lope_home());
    cmd
}

/// Replace this process with Python Lope. Returns only on failure, as an exit
/// code following the shell convention (127 not found, 126 not executable).
pub fn exec(args: &[OsString]) -> i32 {
    let mut cmd = command(args);
    // Rust ignores SIGPIPE at startup and ignored signals survive exec; give
    // Python the default disposition, as a shell would.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let err = cmd.exec();
    let program = cmd.get_program().to_string_lossy().into_owned();
    eprintln!("lope: cannot run Python Lope ({program} -m lope): {err}");
    match err.kind() {
        std::io::ErrorKind::NotFound => exit::NOT_FOUND,
        _ => exit::CANNOT_EXECUTE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreter_resolution() {
        assert_eq!(interpreter_from(Some("/opt/py".into()), true), "/opt/py");
        assert_eq!(interpreter_from(Some("".into()), true), SYSTEM_PYTHON);
        assert_eq!(interpreter_from(None, true), SYSTEM_PYTHON);
        assert_eq!(interpreter_from(None, false), "python3");
    }
}
