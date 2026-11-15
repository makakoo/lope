//! `journal.py`: append-only `$LOPE_HOME/journal.jsonl` (heal events).

use crate::pyjson::{self, Opts};
use serde_json::{Map, Value};
use std::io::Write;
use std::path::PathBuf;

pub fn journal_path() -> PathBuf {
    crate::paths::lope_home_get_default().join("journal.jsonl")
}

/// `append_event(event, payload)`: best effort, never fails the caller.
pub fn append_event(event: &str, payload: &Map<String, Value>) {
    let mut record = Map::new();
    record.insert(
        "timestamp".into(),
        serde_json::Number::from_f64(crate::time::now()).map(Value::Number).unwrap_or(Value::Null),
    );
    record.insert("event".into(), Value::from(event));
    for (k, v) in payload {
        record.insert(k.clone(), v.clone());
    }
    let line = pyjson::dumps(&Value::Object(record), &Opts::default()) + "\n";
    let path = journal_path();
    let _ = (|| -> std::io::Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
        f.write_all(line.as_bytes())
    })();
}

/// `read_recent(limit)`: last `limit` lines, malformed ones skipped.
pub fn read_recent(limit: usize) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(journal_path()) else {
        return vec![];
    };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..]
        .iter()
        .filter_map(|l| {
            let t = l.trim();
            if t.is_empty() {
                None
            } else {
                pyjson::loads(t).ok()
            }
        })
        .collect()
}
