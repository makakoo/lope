//! Secret redaction (`redaction.py`), replacement shapes byte-identical.
//!
//! Python's PEM and header patterns use backreferences, which the `regex`
//! crate does not support; they are implemented by hand with the same
//! leftmost-first semantics.

use regex::Regex;
use serde_json::{Map, Value};
use std::sync::OnceLock;

struct Patterns {
    pem_begin: Regex,
    sk: Regex,
    gh: Regex,
    bearer: Regex,
    header: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        pem_begin: Regex::new(r"-----BEGIN ([A-Z0-9 ]*PRIVATE KEY)-----").unwrap(),
        sk: Regex::new(r"\bsk-[A-Za-z0-9_\-]{8,}\b").unwrap(),
        gh: Regex::new(r"\bgh[pousr]_[A-Za-z0-9_]{12,}\b").unwrap(),
        bearer: Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+\-/=]{10,}\b").unwrap(),
        // (['"]?)TOKEN\3  ==  "TOKEN" | 'TOKEN' | TOKEN  (tried in that order)
        header: Regex::new(
            r#"(?i)\b(api[-_ ]?key|x-api-key|authorization|token|access_token|secret)(\s*[:=]\s*)(?:"([A-Za-z0-9._~+\-/=]{16,})"|'([A-Za-z0-9._~+\-/=]{16,})'|([A-Za-z0-9._~+\-/=]{16,}))"#,
        )
        .unwrap(),
    })
}

/// `-----BEGIN (X)-----.*?-----END \1-----` with DOTALL: non-overlapping,
/// leftmost first. A BEGIN without a matching END does not match; scanning
/// resumes one character after it, as the backtracking engine would.
fn redact_pem(text: &str) -> String {
    let p = patterns();
    let mut out = String::with_capacity(text.len());
    let mut emitted = 0;
    let mut search = 0;
    while search <= text.len() {
        let Some(c) = p.pem_begin.captures_at(text, search) else {
            break;
        };
        let whole = c.get(0).unwrap();
        let family = c.get(1).unwrap().as_str();
        let end_marker = format!("-----END {family}-----");
        match text[whole.end()..].find(&end_marker) {
            Some(i) => {
                out.push_str(&text[emitted..whole.start()]);
                out.push_str(&format!("-----BEGIN {family}-----\n<redacted>\n-----END {family}-----"));
                emitted = whole.end() + i + end_marker.len();
                search = emitted;
            }
            None => {
                search = whole.start() + 1;
                while search < text.len() && !text.is_char_boundary(search) {
                    search += 1;
                }
            }
        }
    }
    out.push_str(&text[emitted..]);
    out
}

pub fn redact_text(text: &str) -> String {
    let p = patterns();
    let t = redact_pem(text);
    let t = p.sk.replace_all(&t, "sk-<redacted>");
    let t = p.gh.replace_all(&t, "ghp_<redacted>");
    let t = p.bearer.replace_all(&t, "Bearer <redacted>");
    let t = p.header.replace_all(&t, |c: &regex::Captures| {
        let quote = if c.get(3).is_some() {
            "\""
        } else if c.get(4).is_some() {
            "'"
        } else {
            ""
        };
        format!("{}{}{quote}<redacted>{quote}", &c[1], &c[2])
    });
    t.into_owned()
}

/// `redact_mapping` / `_redact_value`: redact every string leaf.
pub fn redact_value(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(redact_text(s)),
        Value::Array(a) => Value::Array(a.iter().map(redact_value).collect()),
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, val) in m {
                out.insert(k.clone(), redact_value(val));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        assert_eq!(redact_text("key sk-abcdefgh123 x"), "key sk-<redacted> x");
        assert_eq!(redact_text("ghp_abcdefghijkl1"), "ghp_<redacted>");
        assert_eq!(redact_text("Authorization: Bearer abcdefghijklmnop"), "Authorization: Bearer <redacted>");
        assert_eq!(redact_text("api_key=\"ABCDEFGHIJKLMNOPQRST\""), "api_key=\"<redacted>\"");
        assert_eq!(redact_text("token: 'ABCDEFGHIJKLMNOPQRST'"), "token: '<redacted>'");
        assert_eq!(redact_text("X-API-Key: ABCDEFGHIJKLMNOPQRST"), "X-API-Key: <redacted>");
        let pem = "a\n-----BEGIN RSA PRIVATE KEY-----\nMIIx\n-----END RSA PRIVATE KEY-----\nb";
        assert_eq!(
            redact_text(pem),
            "a\n-----BEGIN RSA PRIVATE KEY-----\n<redacted>\n-----END RSA PRIVATE KEY-----\nb"
        );
        assert_eq!(redact_text("plain prose stays"), "plain prose stays");
    }
}
