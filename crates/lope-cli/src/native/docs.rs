//! Native handler for `lope docs` (`cli.py`, Sprint S8, `FR39`).

use crate::native::Native;
use std::fs;
use std::path::PathBuf;

const EMBEDDED_REFERENCE: &str = include_str!("../../assets/reference.md");

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn find_reference_md() -> Option<String> {
    if let Some(path) = non_empty_env("LOPE_DOCS_PATH") {
        if let Ok(text) = fs::read_to_string(path) {
            return Some(text);
        }
    }
    if let Some(src) = non_empty_env("LOPE_SOURCE_DIR") {
        let p = PathBuf::from(src).join("docs/reference.md");
        if let Ok(text) = fs::read_to_string(p) {
            return Some(text);
        }
    }
    None
}

pub fn run() -> Native {
    let text = find_reference_md().unwrap_or_else(|| EMBEDDED_REFERENCE.to_string());
    // Match Python's `print(ref.read_text(encoding="utf-8"))`: single trailing newline.
    println!("{text}");
    Native::Exit(0)
}
