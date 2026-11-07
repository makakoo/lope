//! `diagnostics.py`: bounded, credential-redacted subprocess diagnostics.

use crate::pyre::{re, tail};
use fancy_regex::Regex;
use std::sync::OnceLock;

fn pats() -> &'static [(Regex, &'static str); 8] {
    static P: OnceLock<[(Regex, &'static str); 8]> = OnceLock::new();
    P.get_or_init(|| {
        [
            // Python's non-MULTILINE `$` also matches before a final "\n".
            (re(r"-----BEGIN [^-]*PRIVATE KEY-----[\s\S]*?(?:-----END [^-]*PRIVATE KEY-----|(?=\n?\z))"), "[REDACTED PRIVATE KEY]"),
            (re(r"<think>[\s\S]*?(?:</think>|(?=\n?\z))"), "[REDACTED REASONING]"),
            (re(r#"(?im)^.*(?:"thinking_delta"|"reasoning_content"|"thinking"\s*:).*$"#), "[REDACTED REASONING]"),
            (re(r#"(?i)\b(Bearer|Basic)\s+[^\s"'<>]+"#), "${1} [REDACTED]"),
            (
                re(r#"(?ix)(["']?(?:api[_-]?key|access[_-]?token|refresh[_-]?token|password|secret)["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,;}]+)"#),
                "${1}[REDACTED]",
            ),
            (re(r"\b(?:sk-[A-Za-z0-9_-]{8,}|gh[pousr]_[A-Za-z0-9_]{8,}|xox[baprs]-[A-Za-z0-9-]{8,})\b"), "[REDACTED]"),
            (re(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b"), "[REDACTED JWT]"),
            (re(r"([a-zA-Z][a-zA-Z0-9+.-]*://)[^\s/@]+:[^\s/@]+@"), "${1}[REDACTED]@"),
        ]
    })
}

pub fn redact_diagnostic(text: &str) -> String {
    let mut t = text.to_string();
    for (r, rep) in pats().iter() {
        t = r.replace_all(&t, *rep).into_owned();
    }
    t
}

pub fn subprocess_diagnostics(stdout: &str, stderr: &str, limit: usize) -> String {
    let mut details = Vec::new();
    for (name, v) in [("stdout", stdout), ("stderr", stderr)] {
        let red = redact_diagnostic(v);
        let text = crate::pyre::strip(&red);
        if !text.is_empty() {
            let shown = if crate::pyre::char_len(text) > limit {
                format!("...[truncated]...{}", tail(text, limit))
            } else {
                text.to_string()
            };
            details.push(format!("{name}: {shown}"));
        }
    }
    details.join("; ")
}

pub fn diagnostic_suffix(stdout: &str, stderr: &str) -> String {
    let d = subprocess_diagnostics(stdout, stderr, 500);
    if d.is_empty() {
        d
    } else {
        format!("; {d}")
    }
}
