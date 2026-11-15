//! S2 measurement: post-exit cleanup latency (leader exit → group confirmed
//! empty) for a provider that leaves a same-group background child, the D1
//! pattern. Prints the distribution and how many calls Python's 0.7 s budget
//! would have failed. Usage: cleanup_latency SUPERVISOR_EXE [N]
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sup = &a[1];
    let n: usize = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(100);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("provider");
    std::fs::write(&p, "#!/bin/sh\necho answer\nsleep 30 &\nsleep 30 &\nexit 0\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut xs = Vec::new();
    let mut outcomes = std::collections::BTreeMap::<String, usize>::new();
    for i in 0..n {
        let res = dir.path().join(format!("r{i}.json"));
        let spec = serde_json::json!({"command": [p.display().to_string()], "timeout": 30.0});
        let mut child = std::process::Command::new(sup).arg(&res).stdin(std::process::Stdio::piped()).spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        writeln!(stdin, "{spec}").unwrap();
        child.wait().unwrap();
        drop(stdin);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&res).unwrap()).unwrap();
        *outcomes
            .entry(format!("{}/{}", v["outcome"].as_str().unwrap(), v["cleanup_result"].as_str().unwrap()))
            .or_default() += 1;
        xs.push(v["cleanup_seconds"].as_f64().unwrap_or(f64::NAN));
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| xs[((xs.len() - 1) as f64 * p).round() as usize];
    let over = |t: f64| xs.iter().filter(|x| **x > t).count();
    let load = std::fs::read_to_string("/dev/null").map(|_| ()).ok();
    let _ = load;
    let mut la = [0f64; 3];
    unsafe { libc::getloadavg(la.as_mut_ptr(), 3) };
    println!("cleanup_latency: n={n} load1={:.1}", la[0]);
    println!(
        "  post-exit cleanup seconds: min={:.4} p50={:.4} p90={:.4} p95={:.4} p99={:.4} max={:.4}",
        xs[0],
        q(0.5),
        q(0.9),
        q(0.95),
        q(0.99),
        xs[n - 1]
    );
    println!("  calls over 0.7 s (Python's budget, would be UNVERIFIED): {}", over(0.7));
    println!("  calls over 2.0 s (Lope v2 default budget): {}", over(2.0));
    println!("  outcomes: {outcomes:?}");
}
