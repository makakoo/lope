//! Binary-level router behaviour (FR2, FR4, FR5, FR23). Hermetic: Python is
//! replaced by a fake interpreter via LOPE_PYTHON, except where noted.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const LOPE: &str = env!("CARGO_BIN_EXE_lope");

fn lope(args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(LOPE);
    cmd.args(args);
    for k in ["LOPE_IMPLEMENTATION_DEPTH", "LOPE_FORCE_PYTHON", "LOPE_FORCE_NATIVE", "LOPE_PYTHON", "LOPE_ROUTER_TRACE"]
    {
        cmd.env_remove(k);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().expect("run lope")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A fake interpreter that records how it was invoked and exits 42.
fn fake_python(dir: &Path) -> PathBuf {
    let p = dir.join("fake-python");
    fs::write(
        &p,
        "#!/bin/sh\nprintf 'PYTHONPATH=%s\\n' \"$PYTHONPATH\"\nfor a in \"$@\"; do printf 'ARG=%s\\n' \"$a\"; done\nexit 42\n",
    )
    .unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// Glimmer's doctor regex `/v?([0-9]+\.[0-9]+(?:\.[0-9]+)?)/`, first match.
fn first_version(s: &str) -> Option<String> {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if !b[i].is_ascii_digit() || (i > 0 && b[i - 1].is_ascii_digit()) {
            continue;
        }
        let rest = &s[i..];
        let mut parts = Vec::new();
        let mut cur = String::new();
        for c in rest.chars() {
            if c.is_ascii_digit() {
                cur.push(c);
            } else if c == '.' && !cur.is_empty() && parts.len() < 2 {
                parts.push(std::mem::take(&mut cur));
            } else {
                break;
            }
        }
        if !cur.is_empty() {
            parts.push(cur);
        }
        if parts.len() >= 2 {
            return Some(parts.join("."));
        }
    }
    None
}

#[test]
fn a1_version_forms_are_native_and_glimmer_parsable() {
    for form in [&["version"][..], &["--version"], &["-V"]] {
        let o = lope(form, &[("LOPE_PYTHON", "/nonexistent/python")]);
        assert_eq!(o.status.code(), Some(0), "{form:?}: {}", stderr(&o));
        assert_eq!(first_version(&stdout(&o)).as_deref(), Some(env!("CARGO_PKG_VERSION")), "{form:?}");
        assert_eq!(stdout(&o), format!("lope {} (super-lope; compat 0.17.3)\n", env!("CARGO_PKG_VERSION")));
        assert!(o.stderr.is_empty());
    }
}

#[test]
fn fr4_nesting_guard_matches_python_byte_for_byte() {
    for depth in ["1", "x", ""] {
        for args in [&["version"][..], &["status"], &[]] {
            let o = lope(args, &[("LOPE_IMPLEMENTATION_DEPTH", depth), ("LOPE_PYTHON", "/nonexistent/python")]);
            assert_eq!(o.status.code(), Some(2));
            assert_eq!(stderr(&o), "error: nested Lope orchestration is disabled inside an implementation writer\n");
            assert!(o.stdout.is_empty());
        }
    }
}

#[test]
fn fr5_delegation_execs_python_with_normalised_argv() {
    let dir = tempfile::tempdir().unwrap();
    let py = fake_python(dir.path());
    let home = dir.path().join("lh");
    let env = [("LOPE_PYTHON", py.to_str().unwrap()), ("LOPE_HOME", home.to_str().unwrap())];
    let cases: &[(&[&str], &[&str])] = &[
        (
            &["review", "notes.md", "--consensus", "quorum", "--format", "markdown", "--brain-log"],
            &["review", "notes.md", "--consensus", "--format", "markdown", "--brain-log"],
        ),
        (
            &["negotiate", "g", "--domain", "engineering", "--rounds", "3"],
            &["negotiate", "g", "--domain", "engineering", "--max-rounds", "3"],
        ),
        (&["docs"], &["docs"]),
        (&[], &[]),
        (&["version", "--help"], &["version", "--help"]),
        (&["review", "--consensus", "quorum", "--brain-log"], &["review", "--consensus", "quorum", "--brain-log"]),
        (&["ask", "a b\tc", "", "--", "-x"], &["ask", "a b\tc", "", "--", "-x"]),
    ];
    for (input, expected) in cases {
        let o = lope(input, &env);
        assert_eq!(o.status.code(), Some(42), "{input:?}: exit status is passed through");
        let mut want = format!("PYTHONPATH={}\nARG=-m\nARG=lope\n", home.display());
        for a in *expected {
            want.push_str(&format!("ARG={a}\n"));
        }
        assert_eq!(stdout(&o), want, "{input:?}");
    }
}

#[test]
fn fr5_force_flags_and_exec_failure() {
    let dir = tempfile::tempdir().unwrap();
    let py = fake_python(dir.path());
    let py = py.to_str().unwrap();
    // FORCE_PYTHON: even `version` is delegated.
    let o = lope(&["--version"], &[("LOPE_PYTHON", py), ("LOPE_FORCE_PYTHON", "1")]);
    assert_eq!(o.status.code(), Some(42));
    assert!(stdout(&o).ends_with("ARG=lope\nARG=version\n"), "{}", stdout(&o));
    // FORCE_NATIVE: uncovered invocations exit 2 without running Python.
    let o = lope(&["docs"], &[("LOPE_PYTHON", py), ("LOPE_FORCE_NATIVE", "1")]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    assert!(stderr(&o).contains("not implemented natively yet"), "{}", stderr(&o));
    let o = lope(&["version"], &[("LOPE_FORCE_NATIVE", "yes")]);
    assert_eq!(o.status.code(), Some(0));
    // Missing interpreter: shell convention 127.
    let o = lope(&["docs"], &[("LOPE_PYTHON", "/nonexistent/python")]);
    assert_eq!(o.status.code(), Some(127));
    assert!(stderr(&o).starts_with("lope: cannot run Python Lope"), "{}", stderr(&o));
}

#[test]
fn fr23_oq1_modes_exit_2_before_delegation() {
    let o = lope(&["review", "x.md", "--consensus", "unanimous"], &[("LOPE_PYTHON", "/nonexistent/python")]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).starts_with("lope review: error: --consensus unanimous is not supported"), "{}", stderr(&o));
}

#[test]
fn router_trace() {
    let o = lope(
        &["review", "n.md", "--consensus", "quorum", "--brain-log"],
        &[("LOPE_PYTHON", "/nonexistent/python"), ("LOPE_ROUTER_TRACE", "1")],
    );
    assert!(
        stderr(&o).starts_with("lope-router: delegate (NoCoverage) /nonexistent/python -m lope review n.md --consensus --brain-log [normalised: consensus-quorum×1]"),
        "{}",
        stderr(&o)
    );
    let o = lope(
        &["review", "n.md", "--consensus", "quorum"],
        &[("LOPE_ROUTER_TRACE", "1")],
    );
    assert!(
        stderr(&o).starts_with("lope-router: native review [normalised: consensus-quorum×1]\n"),
        "{}",
        stderr(&o)
    );
    let o = lope(&["-V"], &[("LOPE_ROUTER_TRACE", "1")]);
    assert_eq!(stderr(&o), "lope-router: native version [normalised: version-flag]\n");
}

#[test]
fn delegated_exit_status_and_signals_are_pythons() {
    // Real exec: the child *is* this process, so a signal death is visible.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("killer");
    fs::write(&p, "#!/bin/sh\nkill -TERM $$\n").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    let o = lope(&["docs"], &[("LOPE_PYTHON", p.to_str().unwrap())]);
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(o.status.signal(), Some(15));
}

#[test]
fn lope_binary_doubles_as_the_supervisor() {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    let dir = tempfile::tempdir().unwrap();
    let res = dir.path().join("r.json");
    let mut child =
        Command::new(LOPE).arg0("lope-supervisor").arg(&res).stdin(std::process::Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin, r#"{{"command":["/bin/echo","hi"],"timeout":10.0}}"#).unwrap();
    assert!(child.wait().unwrap().success());
    drop(stdin);
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&res).unwrap()).unwrap();
    assert_eq!(v["outcome"], "ok");
    assert_eq!(v["stdout"], "hi\n");
}
