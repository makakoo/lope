//! `output_validity.py`: is a raw answer substantive?
use serde_json::Value;

fn content_from_envelope(v: &Value) -> String {
    let Value::Object(m) = v else { return String::new() };
    for k in ["response", "answer", "text", "content", "output_text"] {
        if let Some(Value::String(s)) = m.get(k) {
            if !s.trim().is_empty() {
                return s.trim().to_string();
            }
        }
    }
    if let Some(Value::Array(ch)) = m.get("choices") {
        if let Some(Value::Object(first)) = ch.first() {
            if let Some(Value::Object(msg)) = first.get("message") {
                if let Some(Value::String(c)) = msg.get("content") {
                    if !c.trim().is_empty() {
                        return c.trim().to_string();
                    }
                }
            }
        }
    }
    String::new()
}

/// Python truthiness of a JSON value.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `classify_output(raw).substantive`
pub fn substantive(raw: &str) -> bool {
    let text = crate::pyre::strip(raw);
    if text.is_empty() {
        return false;
    }
    if matches!(text.to_uppercase().as_str(), "CLEAN" | "PASS" | "OK" | "NO FINDINGS") {
        return true;
    }
    if let Ok(env) = lope_core::pyjson::loads(text) {
        if env.is_object() {
            if !content_from_envelope(&env).is_empty() {
                return true;
            }
            return false;
        }
    }
    let lowered = text.to_lowercase();
    !((lowered.starts_with("<tool_use") || lowered.starts_with("<tool_call") || lowered.starts_with("tool_calls:"))
        && !lowered.contains("verdict"))
}
