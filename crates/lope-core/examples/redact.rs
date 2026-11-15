//! Differential-test helper: one JSON string per line in, redacted JSON string out.
use std::io::BufRead;
fn main() {
    for line in std::io::stdin().lock().lines() {
        let s: String = serde_json::from_str(&line.unwrap()).unwrap();
        println!("{}", serde_json::to_string(&lope_core::redaction::redact_text(&s)).unwrap());
    }
}
