//! A7: a timeout writes a 0600 redacted `.diag` sidecar outside the run's
//! work dir, the manifest carries no forbidden keys, and Python Lope's
//! `jobs list` still parses the run.
mod common;
use lope_runtime::call::{run_subprocess_group, CallContext, CallError, Request};
use lope_state::registry::RunRegistry;
use std::os::unix::fs::PermissionsExt;

#[test]
fn a7_timeout_diag_sidecar_and_python_reads_the_run() {
    common::setup();
    let dir = tempfile::tempdir().unwrap();
    let home = std::fs::canonicalize(dir.path()).unwrap().join("lope-home");
    let reg = RunRegistry::open(home.join("runs")).unwrap();
    reg.start_run("ask", Some("run-a7"), Some(600.0), None).unwrap();
    let mut call = serde_json::Map::new();
    call.insert("call_id".into(), serde_json::json!("call-1"));
    call.insert("validator".into(), serde_json::json!("mock"));
    call.insert("state".into(), serde_json::json!("planned"));
    reg.register_call("run-a7", call).unwrap();
    let p = common::script(
        dir.path(),
        "hang",
        "echo 'progress then Authorization: Bearer abcdefghijklmnopqrstuvwxyz' >&2\nsleep 30\n",
    );
    let mut ctx = CallContext {
        registry_root: Some(reg.root.clone()),
        run_id: "run-a7".into(),
        call_id: "call-1".into(),
        ..Default::default()
    };
    let req = Request::new(vec![p.display().to_string()], 3.0);
    let t = std::time::Instant::now();
    let r = run_subprocess_group(&req, Some(&mut ctx));
    let took = t.elapsed().as_secs_f64();
    let diag = match r {
        Err(CallError::Timeout { diag_path: Some(d), .. }) => std::path::PathBuf::from(d),
        other => panic!("{other:?}"),
    };
    assert!(took < 6.0, "timeout took {took:.2}s");
    assert!(diag.starts_with(home.join("runs").join("diag")), "{}", diag.display());
    assert_eq!(std::fs::metadata(&diag).unwrap().permissions().mode() & 0o777, 0o600);
    let text = std::fs::read_to_string(&diag).unwrap();
    assert!(text.contains("Bearer <redacted>") && !text.contains("abcdefghijklmnop"), "{text}");
    let d: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(d["leader_alive_at_deadline"], true);
    assert!(d["stderr_bytes"].as_u64().unwrap() > 0);
    // manifest: finished call, no forbidden keys, diag_path recorded
    let m = reg.load_active("run-a7").unwrap();
    let c = &m["calls"]["call-1"];
    assert_eq!(c["state"], "finished");
    assert_eq!(c["outcome"], "provider_timeout");
    assert!(c["cleanup_result"].as_str().unwrap().starts_with("clean"), "{c}");
    let flat = m.to_string().to_lowercase();
    for k in lope_state::registry::FORBIDDEN_MANIFEST_KEYS {
        assert!(!flat.contains(&format!("\"{k}\":")), "forbidden key {k}");
    }
    // Python reads the run (and the run work dir holds nothing extra)
    let py = std::process::Command::new("/usr/bin/python3")
        .args(["-m", "lope", "jobs", "list", "--json"])
        .env("LOPE_HOME", &home)
        .env(
            "PYTHONPATH",
            std::env::var("A7_LOPE_SRC").unwrap_or_else(|_| format!("{}/.lope", std::env::var("HOME").unwrap())),
        )
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(py.status.success(), "{}", String::from_utf8_lossy(&py.stderr));
    let v: serde_json::Value = serde_json::from_slice(&py.stdout).unwrap();
    assert_eq!(v["count"], 1);
    assert_eq!(v["jobs"][0]["run_id"], "run-a7");
    assert_eq!(v["jobs"][0]["calls"]["call-1"]["outcome"], "provider_timeout");
    let closed = reg.close_run("run-a7", "completed", "").unwrap();
    assert_eq!(closed["action"], "closed", "{closed}");
}
