//! `request_plan.py` (DIRECT path). Native verbs run only DIRECT plans; a
//! plan that would CHUNK or REJECT makes the verb delegate to Python before
//! any side effect (semantic chunking needs Python's `ast`).

use crate::budget::InputProfile;
use crate::generic::Validator;
use serde_json::{json, Value};

pub const DEFAULT_DIRECT_PROMPT_BYTES: usize = 96 * 1024;
pub const PROMPT_WRAPPER_RESERVE_BYTES: usize = 4 * 1024;
const ARGV_POLICY_LIMIT: usize = 128 * 1024;

pub struct Transport {
    pub validator: String,
    pub transport: &'static str,
    pub max_prompt_bytes: usize,
}

pub fn transport_for(v: &dyn Validator) -> Transport {
    let t = v.transport();
    let cap = match t {
        "argv" => ARGV_POLICY_LIMIT - PROMPT_WRAPPER_RESERVE_BYTES,
        _ => 1024 * 1024,
    };
    Transport { validator: v.name().to_string(), transport: t, max_prompt_bytes: cap }
}

pub struct Plan {
    pub json: Value,
    pub direct: bool,
    pub planned_calls: usize,
    pub utf8_bytes: usize,
    pub maximum_concurrency: usize,
    pub nominal: f64,
}

pub struct Limits {
    pub max_chunks: i64,
    pub max_calls: i64,
    pub max_input_bytes: i64,
}

/// `plan_request(...)` for the direct case; `direct == false` means Python
/// would chunk or reject.
#[allow(clippy::too_many_arguments)]
pub fn plan_direct(
    text: &str,
    mode: &str,
    validators: &[std::sync::Arc<dyn Validator>],
    policy: &str,
    lim: &Limits,
    per_call_timeout: f64,
    parallel: bool,
    extra_calls: usize,
) -> Plan {
    let prof = InputProfile::from_text(text);
    let ts: Vec<Transport> = validators.iter().map(|v| transport_for(v.as_ref())).collect();
    let n = ts.len();
    let direct_limit = ts.iter().map(|t| t.max_prompt_bytes.max(4)).fold(DEFAULT_DIRECT_PROMPT_BYTES, usize::min);
    let conc = if parallel { n } else { n.min(1) };
    let calls = n + extra_calls;
    let nominal = (if parallel && n > 0 { per_call_timeout } else { per_call_timeout * n as f64 })
        + extra_calls as f64 * per_call_timeout;
    let forecast = prof.utf8_bytes * n;
    let fnum = |x: f64| serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null);
    let direct = policy != "chunk"
        && prof.utf8_bytes <= direct_limit
        && (calls as i64) <= lim.max_calls
        && (forecast as i64) <= lim.max_input_bytes;
    let json = json!({
        "schema_version": 1,
        "mode": mode,
        "action": "direct",
        "reason": format!("input fits direct byte ceiling {direct_limit}"),
        "policy": policy,
        "input": {
            "utf8_bytes": prof.utf8_bytes, "lines": prof.lines, "estimated_tokens": prof.estimated_tokens,
            "estimated_tokens_label": "approximate (ceil UTF-8 bytes / 3)",
        },
        "transports": ts.iter().map(|t| json!({
            "validator": t.validator, "transport": t.transport, "max_prompt_bytes": t.max_prompt_bytes,
            "direct_safe": prof.utf8_bytes <= DEFAULT_DIRECT_PROMPT_BYTES.min(t.max_prompt_bytes),
        })).collect::<Vec<_>>(),
        "chunks": [],
        "required_chunks": 1,
        "planned_calls": calls,
        "required_calls": calls,
        "maximum_concurrency": conc,
        // int arithmetic in Python when the per-call timeout is an int
        "nominal_wall_ceiling_seconds": if nominal.fract() == 0.0 && nominal.abs() < 1e15 { json!(nominal as i64) } else { fnum(nominal) },
        "forecast_input_bytes": forecast,
        "limits": {"max_chunks": lim.max_chunks, "max_calls": lim.max_calls, "max_input_bytes": lim.max_input_bytes},
        "mitigation": null,
    });
    Plan { json, direct, planned_calls: calls, utf8_bytes: prof.utf8_bytes, maximum_concurrency: conc, nominal }
}
