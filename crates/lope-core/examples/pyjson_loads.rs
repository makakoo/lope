//! Differential-test helper: JSON-encoded input strings, one per line; prints
//! `OK <compact json>` or `ERR <python-style message>`.
use lope_core::pyjson::{dumps, loads_py, Opts};
use std::io::BufRead;
fn main() {
    for line in std::io::stdin().lock().lines() {
        let doc: String = serde_json::from_str(&line.unwrap()).unwrap();
        match loads_py(&doc) {
            Ok(v) => println!("OK {}", dumps(&v, &Opts::default())),
            Err(e) => println!("ERR {e}"),
        }
    }
}
