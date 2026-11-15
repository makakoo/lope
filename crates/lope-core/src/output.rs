//! `output.py` helpers.

use crate::pyjson::{self, Opts};
use crate::redaction::redact_value;
use serde_json::Value;

/// `print_json(payload)` text (without the trailing newline print adds):
/// dicts are redacted, lists have their dict items redacted, then
/// `json.dumps(indent=2, sort_keys=True)`.
pub fn json_text(payload: &Value) -> String {
    let red = match payload {
        Value::Object(_) => redact_value(payload),
        Value::Array(items) => {
            Value::Array(items.iter().map(|x| if x.is_object() { redact_value(x) } else { x.clone() }).collect())
        }
        other => other.clone(),
    };
    pyjson::dumps(&red, &Opts::pretty_sorted())
}

/// Python `format(value, "<width")` for the scalar JSON values Lope prints
/// with f-string padding (strings and numbers).
pub fn ljust(v: &Value, width: usize) -> String {
    let s = crate::config::py_str(v);
    let n = s.chars().count();
    if n >= width {
        s
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

pub fn ljust_str(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - n))
    }
}

/// `dict.get(key, default)` rendered with `str()`.
pub fn get_str(v: &Value, key: &str, default: &str) -> String {
    match v.get(key) {
        Some(x) => crate::config::py_str(x),
        None => default.to_string(),
    }
}
