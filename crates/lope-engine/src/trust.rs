//! Trust boundary for project-defined gate commands (`trust.py`, Sprint S7, `FR34`).

use lope_core::pyjson::{self, Opts};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

const TRUTHY: &[&str] = &["1", "true", "yes", "on"];

fn truthy_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| TRUTHY.contains(&v.trim().to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn lope_home() -> PathBuf {
    if let Ok(h) = std::env::var("LOPE_HOME") {
        if !h.is_empty() {
            return PathBuf::from(h);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".lope")
}

pub fn trust_store_path() -> PathBuf {
    lope_home().join("trusted_gates.json")
}

pub fn gate_command_digest<'a>(cmds: impl IntoIterator<Item = &'a str>) -> String {
    let mut sorted: Vec<&str> = cmds.into_iter().collect();
    sorted.sort_unstable();
    let mut hasher = Sha256::new();
    for cmd in sorted {
        hasher.update(cmd.as_bytes());
        hasher.update(b"\x00");
    }
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn load_store() -> Map<String, Value> {
    let Ok(raw) = fs::read_to_string(trust_store_path()) else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

fn canonical_root(root: &Path) -> String {
    fs::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .display()
        .to_string()
}

pub fn is_trusted(root: &Path, digest: &str) -> bool {
    let store = load_store();
    let key = canonical_root(root);
    match store.get(&key) {
        Some(Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some(digest)),
        _ => false,
    }
}

pub fn record_trust(root: &Path, digest: &str) {
    let mut store = load_store();
    let key = canonical_root(root);
    let mut digests: Vec<Value> = match store.get(&key) {
        Some(Value::Array(arr)) => arr.clone(),
        _ => Vec::new(),
    };
    if !digests.iter().any(|v| v.as_str() == Some(digest)) {
        digests.push(Value::String(digest.to_string()));
    }
    store.insert(key, Value::Array(digests));
    let path = trust_store_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(
        path,
        pyjson::dumps(&Value::Object(store), &Opts::pretty_sorted()),
    );
}

pub fn ensure_gates_trusted(
    specs: &[(&str, &str)],
    cwd: Option<&Path>,
    assume_yes: bool,
) -> bool {
    if specs.is_empty() {
        return true;
    }
    let root_buf = match cwd {
        Some(c) => c.to_path_buf(),
        None => std::env::current_dir().unwrap_or_default(),
    };
    let root = fs::canonicalize(&root_buf).unwrap_or(root_buf);
    let digest = gate_command_digest(specs.iter().map(|(_, cmd)| *cmd));

    if assume_yes || truthy_env("LOPE_TRUST_GATES") || truthy_env("LOPE_YES_RUN_PROJECT_COMMANDS")
    {
        record_trust(&root, &digest);
        return true;
    }

    if is_trusted(&root, &digest) {
        return true;
    }

    eprintln!(
        "\n!  lope is about to run {} project-defined gate command(s)",
        specs.len()
    );
    eprintln!(
        "   from {}/.lope/rules.json — they execute with your privileges:",
        root.display()
    );
    for (name, cmd) in specs {
        eprintln!("     - {name}: {cmd}");
    }
    eprintln!("   Only run gate commands from a repository you trust.\n");

    let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();
    if !interactive {
        eprintln!(
            "   Refusing to run gate commands non-interactively from an untrusted repo.\n   Re-run with --trust, or set LOPE_TRUST_GATES=1, to allow them."
        );
        return false;
    }

    eprint!("   Run these gate commands? [y]es once / [a]lways / [N]o: ");
    let _ = io::stderr().flush();
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    let trimmed = answer.trim().to_ascii_lowercase();
    if trimmed == "a" || trimmed == "always" {
        record_trust(&root, &digest);
        return true;
    }
    if trimmed == "y" || trimmed == "yes" {
        return true;
    }
    eprintln!("   Skipped gate execution (repo not trusted).");
    false
}
