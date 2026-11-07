//! A6: a process that leaves the group but keeps the stdout pipe open makes
//! the call UNVERIFIED (output_drain_timeout) with metadata, and never hangs.
mod common;
use lope_runtime::call::{run_subprocess_group, CallError, Request};

#[test]
fn a6_escaped_pipe_holder_is_unverified_not_a_hang() {
    common::setup();
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("escaped");
    let p = common::script(
        dir.path(),
        "escape",
        "/usr/bin/perl -MPOSIX -e 'if (fork()==0) { POSIX::setsid(); open(F, \">\", $ARGV[0]); print F $$; close F; sleep 30; exit 0 } print \"answer\\n\"; exit 0' \"$1\"\n",
    );
    let req = Request::new(vec![p.display().to_string(), pidfile.display().to_string()], 20.0);
    let t = std::time::Instant::now();
    let r = run_subprocess_group(&req, None);
    let took = t.elapsed().as_secs_f64();
    let escaped = common::read_pid(&pidfile);
    unsafe { libc::kill(escaped, libc::SIGKILL) };
    match r {
        Err(CallError::Unverified(c)) => {
            assert_eq!(c.reason, "provider exited but output pipes did not close after bounded drain");
            assert_eq!(c.message(), c.reason);
            assert!(c.cleanup_result == "clean" || c.cleanup_result.starts_with("clean"), "{}", c.cleanup_result);
            assert!(!c.stdout_complete);
            assert!(!c.stdout_truncated);
            assert_eq!(c.returncode, serde_json::json!(0));
            assert_eq!(c.stdout, "answer\n", "output kept for LOPE_RECOVER_UNVERIFIED_OUTPUT");
        }
        other => panic!("expected UNVERIFIED, got {other:?}"),
    }
    eprintln!("A6 took {took:.3}s");
    assert!(took < 5.0, "must not hang ({took:.3}s)");
}
