//! A5: a lingering same-group child is killed and the call is `ok` within
//! 1 s of leader exit, 50 times in a row, with zero false UNVERIFIED.
mod common;
use lope_runtime::call::{run_subprocess_group, Request};

#[test]
fn a5_lingering_same_group_child_is_reaped_and_call_is_ok_50x() {
    common::setup();
    let dir = tempfile::tempdir().unwrap();
    // The background sleep inherits stdout and stays in the leader's group.
    let p = common::script(dir.path(), "provider", "echo answer\nsleep 30 &\necho \"$!\" > \"$1\"\nexit 0\n");
    let mut durations = Vec::new();
    let mut unverified = 0;
    for i in 0..50 {
        let pidfile = dir.path().join(format!("bg{i}"));
        let req = Request::new(vec![p.display().to_string(), pidfile.display().to_string()], 20.0);
        let t = std::time::Instant::now();
        match run_subprocess_group(&req, None) {
            Ok(c) => {
                assert_eq!(c.returncode, 0);
                assert_eq!(c.stdout, "answer\n");
                assert!(c.cleanup_result == "clean" || c.cleanup_result.starts_with("clean:"), "{}", c.cleanup_result);
            }
            Err(lope_runtime::call::CallError::Unverified(_)) => unverified += 1,
            Err(e) => panic!("run {i}: {e:?}"),
        }
        durations.push(t.elapsed().as_secs_f64());
        let bg = common::read_pid(&pidfile);
        assert!(!common::alive(bg), "run {i}: lingering child {bg} survived");
    }
    durations.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = durations[24];
    let p95 = durations[47];
    eprintln!("A5 wall per call: p50={p50:.3}s p95={p95:.3}s max={:.3}s unverified={unverified}", durations[49]);
    assert_eq!(unverified, 0, "false UNVERIFIED results");
    assert!(p95 < 1.0, "p95 {p95:.3}s: call should finish within 1 s of leader exit");
}

#[test]
fn ordinary_outcomes() {
    common::setup();
    let dir = tempfile::tempdir().unwrap();
    // stdin delivery + exit code + markers in env
    let p = common::script(
        dir.path(),
        "echo",
        "cat\necho \"run=$LOPE_RUN_ID call=$LOPE_CALL_ID marker=${#LOPE_PROCESS_MARKER}\" >&2\nexit 3\n",
    );
    let mut req = Request::new(vec![p.display().to_string()], 10.0);
    req.input_text = Some("hello\n".into());
    let c = run_subprocess_group(&req, None).unwrap();
    assert_eq!(c.returncode, 3);
    assert_eq!(c.stdout, "hello\n");
    assert!(c.stderr.contains("marker=32"), "{}", c.stderr);
    // output limit
    let big = common::script(dir.path(), "big", "head -c 5000 /dev/zero | tr '\\0' x\n");
    let mut req = Request::new(vec![big.display().to_string()], 10.0);
    req.stdout_limit = 1000;
    match run_subprocess_group(&req, None) {
        Err(lope_runtime::call::CallError::OutputLimit { stream, limit, stdout, .. }) => {
            assert_eq!((stream.as_str(), limit), ("stdout", 1000));
            assert!(stdout.contains("[output truncated; head and tail preserved]"));
        }
        other => panic!("{other:?}"),
    }
    // missing executable
    let req = Request::new(vec!["/nonexistent/provider".into()], 5.0);
    match run_subprocess_group(&req, None) {
        Err(lope_runtime::call::CallError::FileNotFound(r)) => {
            assert_eq!(r, "FileNotFoundError: [Errno 2] No such file or directory: '/nonexistent/provider'")
        }
        other => panic!("{other:?}"),
    }
    // env override removal and argv policy
    let req = Request::new(vec!["/bin/echo".into(), "x".repeat(130 * 1024)], 5.0);
    assert!(matches!(run_subprocess_group(&req, None), Err(lope_runtime::call::CallError::InputLimit(_))));
}

#[test]
fn verbose_stderr_truncates_without_killing_and_enforces_hard_limit() {
    common::setup();
    let dir = tempfile::tempdir().unwrap();
    // >512 KiB stderr must truncate in BoundedStream without aborting a clean run.
    let noisy =
        common::script(dir.path(), "noisy_ok", "head -c 655360 /dev/zero | tr '\\0' s >&2\necho 'FINAL_ANSWER'\n");
    let req = Request::new(vec![noisy.display().to_string()], 10.0);
    let c = run_subprocess_group(&req, None).unwrap();
    assert_eq!(c.returncode, 0);
    assert_eq!(c.stdout, "FINAL_ANSWER\n");
    assert!(c.stderr.len() <= lope_runtime::call::DEFAULT_STDERR_LIMIT);
    assert!(c.stderr.contains("[output truncated; head and tail preserved]"));

    // Runaway stderr past stderr_hard_limit still terminates cleanly as OutputLimit.
    let flood = common::script(dir.path(), "stderr_flood", "head -c 16384 /dev/zero | tr '\\0' e >&2\n");
    let mut req = Request::new(vec![flood.display().to_string()], 10.0);
    req.stderr_limit = 1000;
    req.stderr_hard_limit = 4000;
    match run_subprocess_group(&req, None) {
        Err(lope_runtime::call::CallError::OutputLimit { stream, limit, stderr, .. }) => {
            assert_eq!((stream.as_str(), limit), ("stderr", 4000));
            assert!(stderr.len() <= 1000);
            assert!(stderr.contains("[output truncated; head and tail preserved]"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn parent_death_kills_the_provider_group() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pid");
    let p = common::script(dir.path(), "slow", "echo $$ > \"$1\"\nsleep 60\n");
    let spec =
        serde_json::json!({"command": [p.display().to_string(), pidfile.display().to_string()], "timeout": 60.0});
    let mut sup = std::process::Command::new(env!("CARGO_BIN_EXE_lope-supervisor"))
        .arg(dir.path().join("result.json"))
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = sup.stdin.take().unwrap();
    writeln!(stdin, "{spec}").unwrap();
    let provider = common::read_pid(&pidfile);
    assert!(common::alive(provider));
    drop(stdin); // owner "dies": control pipe EOF
    let status = sup.wait().unwrap();
    assert!(status.success());
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!common::alive(provider), "provider survived parent death");
    assert!(!dir.path().join("result.json").exists(), "no result is written after parent death");
}
