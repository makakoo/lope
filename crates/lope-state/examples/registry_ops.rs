//! A4 helper: drive the Rust registry. LOPE_HOME selects the root.
//!   registry_ops start RUN_ID OWNER_PID [TIMEOUT]
//!   registry_ops call RUN_ID CALL_ID PATH STATE
//!   registry_ops extra RUN_ID            (adds non-Python fields: tolerance probe)
//!   registry_ops close RUN_ID
//!   registry_ops list | reap [--dry-run] | prune [--dry-run]
use lope_state::registry::RunRegistry;
use serde_json::{json, Map, Value};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let r = RunRegistry::open_default().unwrap();
    let out: Value = match a[0].as_str() {
        "start" => {
            let pid: i64 = a[2].parse().unwrap();
            r.start_run("review", Some(&a[1]), a.get(3).map(|t| t.parse().unwrap()), Some(pid)).unwrap()
        }
        "call" => {
            let mut c = Map::new();
            c.insert("call_id".into(), json!(a[2]));
            c.insert("validator".into(), json!("mock-a"));
            c.insert("state".into(), json!(a[4]));
            c.insert("owned_paths".into(), json!([a[3]]));
            r.register_call(&a[1], c).unwrap()
        }
        "extra" => r
            .update(&a[1], |m| {
                m.insert("super_lope".into(), json!({"diag_path": "x.diag", "cleanup_budget_s": 2.0}));
                Ok(())
            })
            .unwrap(),
        "close" => r.close_run(&a[1], "completed", "").unwrap(),
        "list" => Value::Array(r.list_active(true)),
        "reap" | "prune" => Value::Array(r.reap_all(a.get(1).map(|s| s == "--dry-run").unwrap_or(false)).unwrap()),
        other => panic!("unknown op {other}"),
    };
    println!("{}", lope_core::pyjson::dumps(&out, &lope_core::pyjson::Opts::compact_sorted()));
}
