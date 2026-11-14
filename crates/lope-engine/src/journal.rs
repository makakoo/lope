//! Append-only JSONL heal journal (`journal.py`, Sprint S7, `FR35`).

use lope_core::pyjson::{self, Opts};
use serde_json::{Map, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn journal_path() -> PathBuf {
    let home = std::env::var("LOPE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let h = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(h).join(".lope")
        });
    home.join("journal.jsonl")
}

pub fn append_event(event: &str, payload: Vec<(&str, Value)>) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut map = Map::new();
    map.insert("timestamp".to_string(), serde_json::json!(ts));
    map.insert("event".to_string(), Value::String(event.to_string()));
    for (k, v) in payload {
        map.insert(k.to_string(), v);
    }
    let path = journal_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", pyjson::dumps(&Value::Object(map), &Opts::default_dumps()));
    }
}

pub fn read_recent(limit: usize) -> Vec<Value> {
    let path = journal_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = raw.lines().collect();
    let start = lines.len().saturating_sub(limit);
    let mut entries = Vec::new();
    for line in &lines[start..] {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
            entries.push(v);
        }
    }
    entries
}
