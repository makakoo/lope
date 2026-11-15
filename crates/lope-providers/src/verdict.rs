//! The `---VERDICT---…---END---` parser (`validators.py:parse_opencode_verdict`),
//! JSON-first with the YAML-ish fallback, confidence gate and evidence gate.

pub use crate::models::{PhaseVerdict, ValidatorResult, VerdictStatus};
pub use parse_verdict as parse_opencode_verdict;

pub fn infra_error(
    validator_name: &str,
    error_msg: &str,
    duration: f64,
) -> ValidatorResult {
    ValidatorResult {
        validator_name: validator_name.to_string(),
        verdict: PhaseVerdict::new(VerdictStatus::InfraError, error_msg, duration, validator_name),
        raw_response: String::new(),
        error: error_msg.to_string(),
        flag_error_hint: String::new(),
        repair_attempted: false,
        repair_status: None,
        repaired_response: String::new(),
        initial_parse_status: None,
        parse_error_category: None,
    }
}
use crate::pyre::{is_match, re, search, splitlines};
use fancy_regex::Regex;
use lope_core::pyjson;
use serde_json::Value;
use std::sync::OnceLock;

pub const MIN_CONFIDENCE_FOR_PASS: f64 = 0.7;

struct P {
    block: Regex,
    json_obj: Regex,
    fence: Regex,
    status: Regex,
    confidence: Regex,
    rationale: Regex,
    fixes: Regex,
    nice: Regex,
    file_line: Regex,
    shell_cmd: Regex,
    code_fence: Regex,
    evidence: Vec<Regex>,
}

fn p() -> &'static P {
    static P_: OnceLock<P> = OnceLock::new();
    P_.get_or_init(|| P {
        block: re(r"(?s)---VERDICT---\s*\n(.+?)\n---END---"),
        json_obj: re(r"(?s)\{.*\}"),
        fence: re(r"(?s)```(?:json)?\s*\n(\{.*?\})\s*\n```"),
        status: re(r"(?im)^\s*status:\s*(\w+)"),
        confidence: re(r"(?im)^\s*confidence:\s*([0-9.]+)"),
        rationale: re(r"(?ims)^\s*rationale:\s*(.+?)(?=^\s*(?:required_fixes|nice_to_have|status|confidence):|\z)"),
        fixes: re(
            r"(?im)^\s*required_fixes:\s*$\n(?P<body>(?:.*(?:\n|\z))*?)(?=^\s*(?:nice_to_have|status|confidence):|\z)",
        ),
        nice: re(
            r"(?im)^\s*nice_to_have:\s*$\n(?P<body>(?:.*(?:\n|\z))*?)(?=^\s*(?:required_fixes|status|confidence):|\z)",
        ),
        file_line: re(r"(?:[A-Za-z0-9_./-]+/)?[A-Za-z0-9_.-]+\.[A-Za-z0-9]+:\d+"),
        shell_cmd: re(r"(?:^|\n)[ \t]*[\$>#][ \t]+\S"),
        code_fence: re(r"```[\s\S]*?```"),
        evidence: vec![
            re(r"(?i)\btest(?:s)?\s+passed\b"),
            re(r"(?i)\ball\s+tests?\s+pass(?:ed)?\b"),
            re(r"(?i)\bverified\b"),
            re(r"(?i)\bconfirmed\b"),
            re(r"(?i)\bexit\s+(?:code\s+)?0\b"),
            re(r"(?i)\breturn\s*code\s+0\b"),
            re(r"(?i)\brun\s+\d+\s+tests?\b"),
            re(r"\btest_\w+\b"),
        ],
    })
}

/// `validate_verdict_dict`: Ok(()) or the schema error text.
pub fn validate_verdict_dict(d: &Value) -> Result<(), String> {
    let Value::Object(m) = d else { return Err("VERDICT must be an object".into()) };
    for f in ["status", "confidence", "rationale"] {
        if !m.contains_key(f) {
            return Err(format!("VERDICT missing required field: '{f}'"));
        }
    }
    let Value::String(status) = &m["status"] else { return Err("VERDICT.status must be string".into()) };
    if !matches!(status.to_uppercase().as_str(), "PASS" | "NEEDS_FIX" | "FAIL") {
        return Err("VERDICT.status must be PASS|NEEDS_FIX|FAIL".into());
    }
    let conf = match &m["confidence"] {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        _ => return Err("VERDICT.confidence must be number".into()),
    };
    if !(0.0..=1.0).contains(&conf) {
        return Err("VERDICT.confidence must be in [0.0, 1.0]".into());
    }
    let Value::String(r) = &m["rationale"] else { return Err("VERDICT.rationale must be string".into()) };
    if r.trim().is_empty() {
        return Err("VERDICT.rationale must not be empty".into());
    }
    for key in ["required_fixes", "nice_to_have"] {
        if let Some(v) = m.get(key) {
            let Value::Array(items) = v else { return Err(format!("VERDICT.{key} must be array")) };
            if items.iter().any(|i| !i.is_string()) {
                return Err(format!("VERDICT.{key}[i] must be string"));
            }
        }
    }
    Ok(())
}

fn try_parse_json_verdict(body: &str) -> Option<Value> {
    let stripped = body.trim();
    if stripped.is_empty() {
        return None;
    }
    let mut cands: Vec<String> = Vec::new();
    if stripped.starts_with('{') && stripped.ends_with('}') {
        cands.push(stripped.to_string());
    }
    if let Some(c) = search(&p().fence, body) {
        cands.push(c.get(1).unwrap().as_str().to_string());
    }
    if let Some(c) = search(&p().json_obj, body) {
        cands.push(c.get(0).unwrap().as_str().to_string());
    }
    for c in cands {
        if let Ok(v) = pyjson::loads(&c) {
            if validate_verdict_dict(&v).is_ok() {
                return Some(v);
            }
        }
    }
    None
}

fn parse_bullet_list(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    for line in splitlines(body) {
        let s = line.trim();
        if let Some(rest) = s.strip_prefix("- ") {
            let item = rest.trim();
            if !item.is_empty() && !item.to_lowercase().contains("empty") {
                items.push(item.to_string());
            }
        } else if let Some(rest) = s.strip_prefix("* ") {
            let item = rest.trim();
            if !item.is_empty() {
                items.push(item.to_string());
            }
        }
    }
    items
}

pub fn evidence_present(text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    let p = p();
    is_match(&p.code_fence, text)
        || is_match(&p.file_line, text)
        || is_match(&p.shell_cmd, text)
        || p.evidence.iter().any(|r| is_match(r, text))
}

fn apply_evidence_gate(
    status: VerdictStatus,
    rationale: &str,
    raw: &str,
    fixes: Vec<String>,
) -> (VerdictStatus, Vec<String>, bool) {
    if std::env::var("LOPE_EVIDENCE_GATE").map(|v| v.to_lowercase() == "off").unwrap_or(false) {
        return (status, fixes, false);
    }
    if status != VerdictStatus::Pass || evidence_present(rationale) || evidence_present(raw) {
        return (status, fixes, false);
    }
    let mut f = fixes;
    f.insert(
        0,
        "provide verification evidence in rationale (file:line reference, command output, test result, or fenced code block) — no PASS without evidence".into(),
    );
    (VerdictStatus::NeedsFix, f, true)
}

/// Python `f"{x:.2f}"` (round-half-even on the exact binary value).
pub fn fmt2(x: f64) -> String {
    format!("{x:.2}")
}

/// `parse_opencode_verdict(text, validator_name, fallback_duration)`.
pub fn parse_verdict(text: &str, validator_name: &str, duration: f64) -> PhaseVerdict {
    let infra = |msg: String| PhaseVerdict::new(VerdictStatus::InfraError, msg, duration, validator_name);
    if text.is_empty() {
        return infra("opencode response was empty".into());
    }
    let Some(block) = search(&p().block, text).map(|c| c.get(1).unwrap().as_str().to_string()) else {
        return infra("no ---VERDICT---...---END--- block found in opencode response".into());
    };
    let gate_conf = |status: &mut VerdictStatus, fixes: &mut Vec<String>, conf: f64| {
        if *status == VerdictStatus::Pass && conf < MIN_CONFIDENCE_FOR_PASS {
            *status = VerdictStatus::NeedsFix;
            if fixes.is_empty() {
                fixes.push(format!("confidence {} below threshold {MIN_CONFIDENCE_FOR_PASS}", fmt2(conf)));
            }
        }
    };
    if let Some(j) = try_parse_json_verdict(&block) {
        let mut status = VerdictStatus::parse(&j["status"].as_str().unwrap().to_uppercase()).unwrap();
        let confidence = j["confidence"].as_f64().unwrap_or(0.0);
        let rationale = j["rationale"].as_str().unwrap().trim().to_string();
        let keep = |k: &str| -> Vec<String> {
            j.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .filter(|s| !s.trim().is_empty() && !s.to_lowercase().contains("empty"))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut fixes = keep("required_fixes");
        let nice = keep("nice_to_have");
        gate_conf(&mut status, &mut fixes, confidence);
        let (status, fixes, ev) = apply_evidence_gate(status, &rationale, text, fixes);
        return PhaseVerdict {
            status,
            confidence,
            rationale,
            required_fixes: fixes,
            nice_to_have: nice,
            duration_seconds: duration,
            validator_name: validator_name.into(),
            stage: None,
            evidence_gate_triggered: ev,
        };
    }
    let status_raw = search(&p().status, &block).map(|c| c.get(1).unwrap().as_str().to_uppercase()).unwrap_or_default();
    let Some(mut status) = VerdictStatus::parse(&status_raw)
        .filter(|s| matches!(s, VerdictStatus::Pass | VerdictStatus::NeedsFix | VerdictStatus::Fail))
    else {
        return infra(format!("unknown status token: {}", lope_state::registry::py_repr_str(&status_raw)));
    };
    let confidence =
        search(&p().confidence, &block).and_then(|c| py_float_str(c.get(1).unwrap().as_str())).unwrap_or(0.0);
    let rationale = search(&p().rationale, &block)
        .map(|c| {
            splitlines(c.get(1).unwrap().as_str())
                .iter()
                .map(|l| l.trim_end())
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    let mut fixes =
        search(&p().fixes, &block).map(|c| parse_bullet_list(c.name("body").unwrap().as_str())).unwrap_or_default();
    let nice =
        search(&p().nice, &block).map(|c| parse_bullet_list(c.name("body").unwrap().as_str())).unwrap_or_default();
    gate_conf(&mut status, &mut fixes, confidence);
    let (status, fixes, ev) = apply_evidence_gate(status, &rationale, text, fixes);
    PhaseVerdict {
        status,
        confidence,
        rationale,
        required_fixes: fixes,
        nice_to_have: nice,
        duration_seconds: duration,
        validator_name: validator_name.into(),
        stage: None,
        evidence_gate_triggered: ev,
    }
}

/// Python `float("1.2.3")` raises; `[0-9.]+` can produce such strings.
fn py_float_str(s: &str) -> Option<f64> {
    if s.matches('.').count() > 1 || s == "." {
        return None;
    }
    s.parse().ok()
}
