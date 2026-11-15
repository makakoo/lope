//! Port of `lope/sarif.py`: minimal SARIF v2.1.0 emitter for Lope consensus reviews.

use crate::findings::ConsensusFinding;
use lope_core::pyjson;
use lope_core::redaction::redact_text;
use serde_json::{json, Map, Value};

pub const SARIF_SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";
pub const SARIF_VERSION: &str = "2.1.0";
pub const TOOL_INFORMATION_URI: &str = "https://github.com/traylinx/lope";

pub fn severity_to_sarif_level(severity: &str) -> &'static str {
    match severity.to_ascii_lowercase().as_str() {
        "critical" | "high" => "error",
        "medium" => "warning",
        _ => "note",
    }
}

pub fn rule_id_for(finding: &ConsensusFinding) -> String {
    let cat = finding
        .category
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("uncategorized")
        .to_ascii_lowercase();
    let sev = {
        let s = finding.severity.trim().to_ascii_lowercase();
        if s.is_empty() {
            "info".to_string()
        } else {
            s
        }
    };
    format!("lope.{cat}.{sev}")
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn build_message(finding: &ConsensusFinding) -> String {
    let mut bits: Vec<String> = Vec::new();
    let m = finding.message.trim();
    if !m.is_empty() {
        bits.push(m.to_string());
    }
    bits.push(format!(
        "consensus {:.2} · agreement {}/{}",
        finding.consensus_score, finding.agreement_count, finding.total_validators
    ));
    if !finding.detected_by.is_empty() {
        bits.push(format!("detected_by: {}", finding.detected_by.join(", ")));
    }
    redact_text(&bits.join(" · "))
}

fn build_location(finding: &ConsensusFinding) -> Option<Value> {
    let f = finding.file.as_deref().filter(|s| !s.is_empty())?;
    let mut physical = Map::new();
    physical.insert("artifactLocation".into(), json!({"uri": f}));
    if let Some(ln) = finding.line {
        let mut region = Map::new();
        region.insert("startLine".into(), json!(ln));
        if let Some(el) = finding.end_line {
            region.insert("endLine".into(), json!(el));
        }
        physical.insert("region".into(), Value::Object(region));
    }
    Some(json!({"physicalLocation": Value::Object(physical)}))
}

pub fn finding_to_result(finding: &ConsensusFinding) -> Value {
    let mut result = Map::new();
    result.insert("ruleId".into(), json!(rule_id_for(finding)));
    result.insert(
        "level".into(),
        json!(severity_to_sarif_level(&finding.severity)),
    );
    result.insert("message".into(), json!({"text": build_message(finding)}));
    result.insert(
        "properties".into(),
        json!({
            "detected_by": finding.detected_by,
            "consensus_score": round3(finding.consensus_score),
            "agreement_ratio": round3(finding.agreement_ratio),
            "agreement_count": finding.agreement_count,
            "total_validators": finding.total_validators,
            "confidence": round3(finding.confidence_max),
            "severity": finding.severity,
            "category": finding.category,
            "consensus_level": finding.consensus_level.as_str(),
            "dissenting": finding.dissenting,
        }),
    );
    if let Some(loc) = build_location(finding) {
        result.insert("locations".into(), json!([loc]));
    }
    Value::Object(result)
}

pub fn build_rules(findings: &[ConsensusFinding]) -> Vec<Value> {
    let mut seen: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    for f in findings {
        let rid = rule_id_for(f);
        seen.entry(rid.clone()).or_insert_with(|| {
            json!({
                "id": rid,
                "name": rid,
                "shortDescription": {"text": rid},
                "defaultConfiguration": {"level": severity_to_sarif_level(&f.severity)},
                "properties": {
                    "severity": f.severity,
                    "category": f.category,
                },
            })
        });
    }
    seen.into_values().collect()
}

pub fn build_sarif(findings: &[ConsensusFinding], tool_version: Option<&str>) -> Value {
    let version = tool_version.unwrap_or(lope_core::COMPAT_VERSION);
    let rules = build_rules(findings);
    let results: Vec<Value> = findings.iter().map(finding_to_result).collect();
    json!({
        "$schema": SARIF_SCHEMA,
        "version": SARIF_VERSION,
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": "lope",
                        "informationUri": TOOL_INFORMATION_URI,
                        "version": version,
                        "rules": rules,
                    }
                },
                "results": results,
            }
        ],
    })
}

pub fn dumps(findings: &[ConsensusFinding], tool_version: Option<&str>) -> String {
    let doc = build_sarif(findings, tool_version);
    pyjson::dumps(
        &doc,
        &pyjson::Opts {
            indent: Some(2),
            sort_keys: true,
            separators: None,
            ensure_ascii: true,
        },
    )
}
