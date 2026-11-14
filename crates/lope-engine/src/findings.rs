//! Port of `lope/findings.py`: structured findings parser, deduplication,
//! consensus scoring, and Markdown consensus rendering.

use lope_core::pyjson;
use lope_core::redaction::redact_text;
use lope_memory::ConsensusFindingInput;
use lope_providers::pyre::{head, splitlines};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub const EVIDENCE_LIMIT: usize = 240;

pub fn severity_rank(sev: &str) -> i32 {
    match sev {
        "critical" => 4,
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

pub fn normalize_severity_opt(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "blocker" | "critical" | "severe" | "fatal" => Some("critical"),
        "high" | "error" | "major" | "important" => Some("high"),
        "warning" | "warn" | "medium" | "moderate" => Some("medium"),
        "low" | "minor" | "nit" | "nitpick" => Some("low"),
        "info" | "informational" | "note" | "notice" | "trivial" => Some("info"),
        _ => None,
    }
}

pub fn normalize_severity(value: Option<&str>) -> String {
    let Some(v) = value else {
        return "info".into();
    };
    normalize_severity_opt(v).unwrap_or("info").to_string()
}

pub fn canonical_category_opt(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "security" | "sec" | "auth" => Some("security"),
        "correctness" | "bug" | "logic" => Some("correctness"),
        "perf" | "performance" | "speed" => Some("perf"),
        "tests" | "test" | "testing" => Some("tests"),
        "ux" | "design" => Some("ux"),
        "docs" | "doc" | "documentation" => Some("docs"),
        "ops" | "operations" | "ci" | "infra" => Some("ops"),
        _ => None,
    }
}

pub fn normalize_category(value: Option<&str>) -> Option<String> {
    let v = value?.trim().to_ascii_lowercase();
    if v.is_empty() {
        return None;
    }
    Some(
        canonical_category_opt(&v)
            .map(String::from)
            .unwrap_or(v),
    )
}

pub fn severity_family(sev: &str) -> &'static str {
    match sev {
        "critical" | "high" => "high",
        "medium" => "medium",
        _ => "low",
    }
}

pub fn clamp_confidence(v: f64) -> f64 {
    if v.is_nan() {
        0.5
    } else {
        v.clamp(0.0, 1.0)
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsensusLevel {
    Confirmed,
    Likely,
    NeedsVerification,
    Disputed,
    Unverified,
}

impl ConsensusLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConsensusLevel::Confirmed => "confirmed",
            ConsensusLevel::Likely => "likely",
            ConsensusLevel::NeedsVerification => "needs-verification",
            ConsensusLevel::Disputed => "disputed",
            ConsensusLevel::Unverified => "unverified",
        }
    }

    pub fn heading(&self) -> &'static str {
        match self {
            ConsensusLevel::Confirmed => "CONFIRMED",
            ConsensusLevel::Likely => "LIKELY",
            ConsensusLevel::NeedsVerification => "NEEDS VERIFICATION",
            ConsensusLevel::Disputed => "DISPUTED",
            ConsensusLevel::Unverified => "UNVERIFIED",
        }
    }
}

pub const LEVEL_ORDER: [ConsensusLevel; 5] = [
    ConsensusLevel::Confirmed,
    ConsensusLevel::Likely,
    ConsensusLevel::NeedsVerification,
    ConsensusLevel::Disputed,
    ConsensusLevel::Unverified,
];

pub fn normalize_message(message: &str) -> String {
    if message.is_empty() {
        return String::new();
    }
    let lower = message.to_lowercase();
    let mut cleaned = String::with_capacity(lower.len());
    let mut prev_space = true;
    for ch in lower.trim().chars() {
        let is_word = ch.is_alphanumeric() || ch == '_';
        if is_word {
            cleaned.push(ch);
            prev_space = false;
        } else if !prev_space {
            cleaned.push(' ');
            prev_space = true;
        }
    }
    cleaned.trim().to_string()
}

pub fn canonical_key(file: Option<&str>, line: Option<i64>, message: &str) -> String {
    let file_part = file.unwrap_or("").trim().to_lowercase();
    let line_part = line.map(|l| l.to_string()).unwrap_or_default();
    format!("{file_part}|{line_part}|{}", normalize_message(message))
}

pub fn finding_hash(file: Option<&str>, line: Option<i64>, message: &str) -> String {
    let key = canonical_key(file, line, message);
    lope_core::sha256_hex(key.as_bytes())[..16].to_string()
}

/// Exact port of Python `difflib.SequenceMatcher(None, a, b).ratio()`.
pub fn sequence_similarity(a: &str, b: &str) -> f64 {
    let na: Vec<char> = normalize_message(a).chars().collect();
    let nb: Vec<char> = normalize_message(b).chars().collect();
    let la = na.len();
    let lb = nb.len();
    if la + lb == 0 {
        return 1.0;
    }
    if la == 0 || lb == 0 {
        return 0.0;
    }

    // Build b2j index with Python 3.2+ autojunk heuristic (len >= 200 and > 1% frequency).
    let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
    for (i, &ch) in nb.iter().enumerate() {
        b2j.entry(ch).or_default().push(i);
    }
    if lb >= 200 {
        let ntest = lb / 100 + 1;
        b2j.retain(|_, idxs| idxs.len() <= ntest);
    }

    let find_longest = |alo: usize, ahi: usize, blo: usize, bhi: usize| -> (usize, usize, usize) {
        let mut besti = alo;
        let mut bestj = blo;
        let mut bestsize = 0usize;
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut newj2len: HashMap<usize, usize> = HashMap::new();
            if let Some(indices) = b2j.get(&na[i]) {
                for &j in indices {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j2len.get(&j.wrapping_sub(1)).copied().unwrap_or(0) + 1;
                    newj2len.insert(j, k);
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = newj2len;
        }
        while besti > alo && bestj > blo && na[besti - 1] == nb[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && na[besti + bestsize] == nb[bestj + bestsize]
        {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    };

    let mut queue = vec![(0usize, la, 0usize, lb)];
    let mut matches = 0usize;
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = find_longest(alo, ahi, blo, bhi);
        if k > 0 {
            matches += k;
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    (2.0 * matches as f64) / ((la + lb) as f64)
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub message: String,
    pub validator: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub end_line: Option<i64>,
    pub severity: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub evidence: String,
    pub raw: String,
}

impl Finding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        message: String,
        validator: String,
        file: Option<String>,
        line: Option<i64>,
        end_line: Option<i64>,
        severity: &str,
        category: Option<&str>,
        confidence: f64,
        evidence: String,
        raw: String,
    ) -> Self {
        let msg = redact_text(&message).trim().to_string();
        let val = {
            let s = validator.trim();
            if s.is_empty() {
                "unknown".to_string()
            } else {
                s.to_string()
            }
        };
        let sev = normalize_severity(Some(severity));
        let cat = normalize_category(category);
        let conf = clamp_confidence(confidence);
        let ev = redact_text(&evidence).trim().to_string();
        let rw = redact_text(&raw).trim().to_string();
        let fl = file.and_then(|f| {
            let t = f.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        });
        Finding {
            message: msg,
            validator: val,
            file: fl,
            line,
            end_line,
            severity: sev,
            category: cat,
            confidence: conf,
            evidence: ev,
            raw: rw,
        }
    }

    pub fn hash(&self) -> String {
        finding_hash(self.file.as_deref(), self.line, &self.message)
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "message": self.message,
            "validator": self.validator,
            "file": self.file,
            "line": self.line,
            "end_line": self.end_line,
            "severity": self.severity,
            "category": self.category,
            "confidence": round3(self.confidence),
            "evidence": self.evidence,
            "raw": self.raw,
        })
    }
}

#[derive(Debug, Clone)]
pub struct MergedFinding {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub end_line: Option<i64>,
    pub severity: String,
    pub category: Option<String>,
    pub detected_by: Vec<String>,
    pub evidence: Map<String, Value>,
    pub confidences: Vec<f64>,
    pub raw_messages: Vec<String>,
}

impl MergedFinding {
    pub fn confidence_max(&self) -> f64 {
        self.confidences
            .iter()
            .copied()
            .fold(0.0_f64, f64::max)
    }

    pub fn confidence_avg(&self) -> f64 {
        if self.confidences.is_empty() {
            0.0
        } else {
            self.confidences.iter().sum::<f64>() / (self.confidences.len() as f64)
        }
    }

    pub fn hash(&self) -> String {
        finding_hash(self.file.as_deref(), self.line, &self.message)
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "message": self.message,
            "file": self.file,
            "line": self.line,
            "end_line": self.end_line,
            "severity": self.severity,
            "category": self.category,
            "detected_by": self.detected_by,
            "evidence": self.evidence,
            "confidence_max": round3(self.confidence_max()),
            "confidence_avg": round3(self.confidence_avg()),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ConsensusFinding {
    pub message: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub end_line: Option<i64>,
    pub severity: String,
    pub category: Option<String>,
    pub detected_by: Vec<String>,
    pub evidence: Map<String, Value>,
    pub confidence_max: f64,
    pub confidence_avg: f64,
    pub agreement_count: usize,
    pub total_validators: usize,
    pub agreement_ratio: f64,
    pub consensus_score: f64,
    pub consensus_level: ConsensusLevel,
    pub dissenting: Vec<String>,
}

impl ConsensusFinding {
    pub fn hash(&self) -> String {
        finding_hash(self.file.as_deref(), self.line, &self.message)
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "message": self.message,
            "file": self.file,
            "line": self.line,
            "end_line": self.end_line,
            "severity": self.severity,
            "category": self.category,
            "detected_by": self.detected_by,
            "evidence": self.evidence,
            "confidence_max": round3(self.confidence_max),
            "confidence_avg": round3(self.confidence_avg),
            "agreement_count": self.agreement_count,
            "total_validators": self.total_validators,
            "agreement_ratio": round3(self.agreement_ratio),
            "consensus_score": round3(self.consensus_score),
            "consensus_level": self.consensus_level.as_str(),
            "dissenting": self.dissenting,
        })
    }

    pub fn to_memory_input(&self) -> ConsensusFindingInput {
        ConsensusFindingInput {
            hash: self.hash(),
            message: self.message.clone(),
            file: self.file.clone(),
            line: self.line,
            end_line: self.end_line,
            severity: self.severity.clone(),
            category: self.category.clone(),
            confidence_max: self.confidence_max,
            consensus_score: self.consensus_score,
            consensus_level: self.consensus_level.as_str().to_string(),
            agreement_count: self.agreement_count as i64,
            total_validators: self.total_validators as i64,
            detected_by: self.detected_by.clone(),
            evidence: self.evidence.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FindingParseResult {
    pub findings: Vec<Finding>,
    pub method: String,
    pub parsed_count: usize,
    pub fallback_used: bool,
    pub raw_input: String,
}

struct Rx {
    bullet: Regex,
    bracket_sev: Regex,
    prefix_colon: Regex,
    prefix_space: Regex,
    loc_start: Regex,
    conf_tail: Regex,
}

fn rx() -> &'static Rx {
    static RX: OnceLock<Rx> = OnceLock::new();
    RX.get_or_init(|| Rx {
        bullet: Regex::new(r"^([\-\*•]\s+)").unwrap(),
        bracket_sev: Regex::new(r"^\[(?P<sev>[A-Za-z]+)\]\s+(?P<rest>.+)$").unwrap(),
        prefix_colon: Regex::new(r"^(?P<word>[A-Za-z][A-Za-z\-_]*)\s*:\s+(?P<rest>.+)$").unwrap(),
        prefix_space: Regex::new(r"^(?P<word>[A-Za-z][A-Za-z\-_]*)\s+(?P<rest>.+)$").unwrap(),
        loc_start: Regex::new(
            r"^(?P<file>(?:[\w\-]+/)*[\w\-]+\.[A-Za-z][A-Za-z0-9_]*)(?::(?P<line>\d+))?(?::(?P<end>\d+))?(?P<sep>\s*[—–\-:]\s*|\s+)",
        )
        .unwrap(),
        conf_tail: Regex::new(r"(?i)\(\s*confidence\s*[:=]\s*(?P<conf>[0-9]*\.?[0-9]+)\s*\)\s*$")
            .unwrap(),
    })
}

pub fn parse_findings(
    text: &str,
    validator_name: &str,
    source_file: Option<&str>,
) -> FindingParseResult {
    if text.trim().is_empty() {
        return FindingParseResult {
            findings: vec![],
            method: "empty".into(),
            parsed_count: 0,
            fallback_used: false,
            raw_input: String::new(),
        };
    }

    let redacted = redact_text(text);
    let validator = {
        let s = validator_name.trim();
        if s.is_empty() {
            "unknown"
        } else {
            s
        }
    };

    let json_findings = try_parse_json(&redacted, validator, source_file);
    if !json_findings.is_empty() {
        let n = json_findings.len();
        return FindingParseResult {
            findings: json_findings,
            method: "json".into(),
            parsed_count: n,
            fallback_used: false,
            raw_input: redacted,
        };
    }

    let mut structured = Vec::new();
    let mut fallback = Vec::new();

    for line in splitlines(&redacted) {
        if line.trim().is_empty() {
            continue;
        }
        if let Some((f, kind)) = parse_line(&line, validator, source_file) {
            if kind == "structured" {
                structured.push(f);
            } else {
                fallback.push(f);
            }
        }
    }

    let has_s = !structured.is_empty();
    let has_f = !fallback.is_empty();
    let mut findings = structured;
    findings.extend(fallback);
    if findings.is_empty() {
        return FindingParseResult {
            findings: vec![],
            method: "empty".into(),
            parsed_count: 0,
            fallback_used: false,
            raw_input: redacted,
        };
    }

    let method = match (has_s, has_f) {
        (true, true) => "mixed",
        (true, false) => "structured",
        _ => "fallback",
    };
    let n = findings.len();
    FindingParseResult {
        findings,
        method: method.into(),
        parsed_count: n,
        fallback_used: has_f,
        raw_input: redacted,
    }
}

fn try_parse_json(text: &str, validator: &str, source_file: Option<&str>) -> Vec<Finding> {
    let stripped = text.trim();
    let mut candidates: Vec<Value> = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(stripped) {
        candidates.push(v);
    } else {
        for (idx, ch) in stripped.char_indices() {
            if ch != '[' && ch != '{' {
                continue;
            }
            let mut de = serde_json::Deserializer::from_str(&stripped[idx..]).into_iter::<Value>();
            if let Some(Ok(v)) = de.next() {
                candidates.push(v);
                break;
            }
        }
    }

    for cand in candidates {
        let items: Vec<&Value> = match &cand {
            Value::Array(arr) => arr.iter().collect(),
            Value::Object(map) => ["findings", "results", "issues"]
                .iter()
                .find_map(|k| map.get(*k).and_then(Value::as_array))
                .map(|a| a.iter().collect())
                .unwrap_or_default(),
            _ => vec![],
        };
        if items.is_empty() {
            continue;
        }
        let findings: Vec<Finding> = items
            .into_iter()
            .filter_map(|item| finding_from_dict(item, validator, source_file))
            .collect();
        if !findings.is_empty() {
            return findings;
        }
    }
    vec![]
}

fn coerce_int(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    if let Some(f) = v.as_f64() {
        return Some(f as i64);
    }
    if let Some(s) = v.as_str() {
        return s.trim().parse::<i64>().ok();
    }
    None
}

fn coerce_float(v: Option<&Value>, default: f64) -> f64 {
    let Some(v) = v else { return default };
    if let Some(f) = v.as_f64() {
        return f;
    }
    if let Some(i) = v.as_i64() {
        return i as f64;
    }
    if let Some(s) = v.as_str() {
        return s.trim().parse::<f64>().unwrap_or(default);
    }
    default
}

fn non_empty_str<'a>(map: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    for k in keys {
        if let Some(s) = map.get(*k).and_then(Value::as_str) {
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

fn finding_from_dict(item: &Value, validator: &str, source_file: Option<&str>) -> Option<Finding> {
    let obj = item.as_object()?;
    let message = non_empty_str(obj, &["message", "title", "description", "text"])?;
    let file = non_empty_str(obj, &["file", "path"])
        .or(source_file)
        .map(String::from);
    let line = coerce_int(obj.get("line"))
        .or_else(|| coerce_int(obj.get("start_line")))
        .or_else(|| coerce_int(obj.get("startLine")));
    let end_line = coerce_int(obj.get("end_line")).or_else(|| coerce_int(obj.get("endLine")));
    let severity = non_empty_str(obj, &["severity", "level"]).unwrap_or("info");
    let category = non_empty_str(obj, &["category", "rule", "kind"]);
    let confidence = if obj.get("confidence").is_some_and(|v| !v.is_null()) {
        coerce_float(obj.get("confidence"), 0.5)
    } else {
        coerce_float(obj.get("score"), 0.5)
    };
    let evidence = non_empty_str(obj, &["evidence", "snippet"]).unwrap_or("");
    let raw = pyjson::dumps(
        item,
        &pyjson::Opts {
            indent: None,
            sort_keys: true,
            separators: None,
            ensure_ascii: false,
        },
    );
    Some(Finding::new(
        message.to_string(),
        validator.to_string(),
        file,
        line,
        end_line,
        severity,
        category,
        confidence,
        evidence.to_string(),
        raw,
    ))
}

fn parse_line(
    line: &str,
    validator: &str,
    source_file: Option<&str>,
) -> Option<(Finding, &'static str)> {
    let stripped = line.trim();
    if stripped.is_empty() {
        return None;
    }
    let r = rx();
    let mut body = stripped;
    let mut is_bullet = false;
    if let Some(m) = r.bullet.find(stripped) {
        is_bullet = true;
        body = &stripped[m.end()..];
    }

    if let Some(caps) = r.bracket_sev.captures(body) {
        let sev_word = caps.name("sev").unwrap().as_str().to_ascii_lowercase();
        if normalize_severity_opt(&sev_word).is_some() {
            if let Some(f) = build_finding(
                &sev_word,
                None,
                caps.name("rest").unwrap().as_str(),
                validator,
                stripped,
                source_file,
                false,
                true,
            ) {
                return Some((f, "structured"));
            }
        }
    }

    if let Some(caps) = r.prefix_colon.captures(body) {
        let word = caps.name("word").unwrap().as_str().to_ascii_lowercase();
        let rest = caps.name("rest").unwrap().as_str();
        if normalize_severity_opt(&word).is_some() {
            if let Some(f) =
                build_finding(&word, None, rest, validator, stripped, source_file, false, true)
            {
                return Some((f, "structured"));
            }
        } else if canonical_category_opt(&word).is_some() {
            if let Some(f) = build_finding(
                "info",
                Some(&word),
                rest,
                validator,
                stripped,
                source_file,
                false,
                true,
            ) {
                return Some((f, "structured"));
            }
        }
    }

    if let Some(caps) = r.prefix_space.captures(body) {
        let word = caps.name("word").unwrap().as_str().to_ascii_lowercase();
        let rest = caps.name("rest").unwrap().as_str();
        if normalize_severity_opt(&word).is_some() {
            if let Some(f) =
                build_finding(&word, None, rest, validator, stripped, source_file, true, false)
            {
                return Some((f, "structured"));
            }
        } else if canonical_category_opt(&word).is_some() {
            if let Some(f) = build_finding(
                "info",
                Some(&word),
                rest,
                validator,
                stripped,
                source_file,
                true,
                false,
            ) {
                return Some((f, "structured"));
            }
        }
    }

    if is_bullet {
        if let Some(f) =
            build_finding("info", None, body, validator, stripped, source_file, false, true)
        {
            return Some((f, "fallback"));
        }
    }

    None
}

#[allow(clippy::too_many_arguments)]
fn build_finding(
    severity: &str,
    category: Option<&str>,
    rest_in: &str,
    validator: &str,
    raw: &str,
    source_file: Option<&str>,
    require_location: bool,
    require_message: bool,
) -> Option<Finding> {
    let mut rest = rest_in.trim().to_string();
    if rest.is_empty() {
        return None;
    }

    let r = rx();
    let mut confidence: Option<f64> = None;
    if let Some(m) = r.conf_tail.captures(&rest) {
        confidence = m
            .name("conf")
            .and_then(|c| c.as_str().parse::<f64>().ok());
        let start = m.get(0).unwrap().start();
        rest = rest[..start].trim_end().to_string();
        if rest.ends_with(',') || rest.ends_with(';') {
            rest.pop();
            rest = rest.trim_end().to_string();
        }
    }

    let mut file: Option<String> = None;
    let mut line: Option<i64> = None;
    let mut end_line: Option<i64> = None;
    let mut message = rest.clone();

    if let Some(caps) = r.loc_start.captures(&rest) {
        file = caps.name("file").map(|m| m.as_str().to_string());
        line = caps
            .name("line")
            .and_then(|m| m.as_str().parse::<i64>().ok());
        end_line = caps
            .name("end")
            .and_then(|m| m.as_str().parse::<i64>().ok());
        let end_idx = caps.get(0).unwrap().end();
        message = rest[end_idx..].trim().to_string();
        if message.is_empty() {
            message = rest.trim().to_string();
            file = None;
            line = None;
            end_line = None;
        }
    }

    if require_location && file.is_none() {
        return None;
    }
    if require_message && message.is_empty() {
        return None;
    }

    if file.is_none() {
        if let Some(sf) = source_file {
            if !sf.is_empty() {
                file = Some(sf.to_string());
            }
        }
    }

    let evidence = head(raw, EVIDENCE_LIMIT).to_string();

    Some(Finding::new(
        message,
        validator.to_string(),
        file,
        line,
        end_line,
        severity,
        category,
        confidence.unwrap_or(0.5),
        evidence,
        raw.to_string(),
    ))
}

pub fn merge_findings(findings: &[Finding], similarity_threshold: f64) -> Vec<MergedFinding> {
    let mut merged: Vec<MergedFinding> = Vec::new();
    for f in findings {
        if let Some(idx) = find_merge_target(&merged, f, similarity_threshold) {
            merge_into(&mut merged[idx], f);
        } else {
            merged.push(initialize_merged(f));
        }
    }
    merged.sort_by(|a, b| {
        (-severity_rank(&a.severity))
            .cmp(&(-severity_rank(&b.severity)))
            .then_with(|| a.file.as_deref().unwrap_or("").cmp(b.file.as_deref().unwrap_or("")))
            .then_with(|| {
                a.line
                    .unwrap_or(1_000_000_000)
                    .cmp(&b.line.unwrap_or(1_000_000_000))
            })
            .then_with(|| a.message.cmp(&b.message))
    });
    merged
}

fn initialize_merged(f: &Finding) -> MergedFinding {
    let src = if !f.evidence.is_empty() {
        &f.evidence
    } else if !f.raw.is_empty() {
        &f.raw
    } else {
        &f.message
    };
    let snippet = head(src, EVIDENCE_LIMIT).to_string();
    let mut ev = Map::new();
    ev.insert(f.validator.clone(), Value::String(snippet));
    MergedFinding {
        message: f.message.clone(),
        file: f.file.clone(),
        line: f.line,
        end_line: f.end_line,
        severity: f.severity.clone(),
        category: f.category.clone(),
        detected_by: vec![f.validator.clone()],
        evidence: ev,
        confidences: vec![f.confidence],
        raw_messages: vec![f.message.clone()],
    }
}

fn merge_into(target: &mut MergedFinding, f: &Finding) {
    let is_new = !target.detected_by.contains(&f.validator);
    if is_new {
        target.detected_by.push(f.validator.clone());
    }
    target.confidences.push(f.confidence);
    target.raw_messages.push(f.message.clone());

    if severity_rank(&f.severity) > severity_rank(&target.severity) {
        target.severity = f.severity.clone();
        target.message = f.message.clone();
    }
    if target.category.is_none() && f.category.is_some() {
        target.category = f.category.clone();
    }
    if target.file.is_none() && f.file.is_some() {
        target.file = f.file.clone();
    }
    if target.line.is_none() && f.line.is_some() {
        target.line = f.line;
    }
    if target.end_line.is_none() && f.end_line.is_some() {
        target.end_line = f.end_line;
    }

    let src = if !f.evidence.is_empty() {
        &f.evidence
    } else if !f.raw.is_empty() {
        &f.raw
    } else {
        &f.message
    };
    let snippet = head(src, EVIDENCE_LIMIT).to_string();
    if is_new {
        target
            .evidence
            .insert(f.validator.clone(), Value::String(snippet));
    } else {
        let existing_len = target
            .evidence
            .get(&f.validator)
            .and_then(Value::as_str)
            .map(|s| s.chars().count())
            .unwrap_or(0);
        if snippet.chars().count() > existing_len {
            target
                .evidence
                .insert(f.validator.clone(), Value::String(snippet));
        }
    }
}

fn find_merge_target(
    merged: &[MergedFinding],
    finding: &Finding,
    similarity_threshold: f64,
) -> Option<usize> {
    let norm_msg = normalize_message(&finding.message);
    for (idx, existing) in merged.iter().enumerate() {
        // Rule 1: exact normalized key
        if finding.file.as_deref().unwrap_or("") == existing.file.as_deref().unwrap_or("")
            && finding.line == existing.line
            && !norm_msg.is_empty()
            && norm_msg == normalize_message(&existing.message)
        {
            return Some(idx);
        }
        // Rule 2: same file, line within ±2, paraphrase similarity >= 0.78
        if let (Some(ff), Some(ef), Some(fl), Some(el)) = (
            finding.file.as_deref(),
            existing.file.as_deref(),
            finding.line,
            existing.line,
        ) {
            if !ff.is_empty()
                && ff == ef
                && (fl - el).abs() <= 2
                && sequence_similarity(&finding.message, &existing.message) >= 0.78
            {
                return Some(idx);
            }
        }
    }

    // Rule 3: message-only similarity within same severity family
    let cross_file_threshold = similarity_threshold.max(0.90);
    for (idx, existing) in merged.iter().enumerate() {
        let f_file = finding.file.as_deref().filter(|s| !s.is_empty());
        let e_file = existing.file.as_deref().filter(|s| !s.is_empty());
        if let (Some(ff), Some(ef)) = (f_file, e_file) {
            if ff != ef {
                if sequence_similarity(&finding.message, &existing.message) >= cross_file_threshold
                    && severity_family(&finding.severity) == severity_family(&existing.severity)
                {
                    return Some(idx);
                }
                continue;
            }
        }
        if sequence_similarity(&finding.message, &existing.message) >= similarity_threshold
            && severity_family(&finding.severity) == severity_family(&existing.severity)
        {
            return Some(idx);
        }
    }
    None
}

pub fn score_consensus(
    merged: &[MergedFinding],
    all_validator_names: &[String],
) -> Vec<ConsensusFinding> {
    let mut roster: Vec<String> = Vec::new();
    for n in all_validator_names {
        if !roster.contains(n) {
            roster.push(n.clone());
        }
    }
    let total = roster.len().max(1);
    let mut results = Vec::with_capacity(merged.len());

    for entry in merged {
        let agreement_count = entry.detected_by.len();
        let agreement_ratio = (agreement_count as f64) / (total as f64);
        let confidence_max = entry.confidence_max();
        let consensus_score = agreement_ratio * confidence_max;
        let level = classify_consensus_level(agreement_ratio, confidence_max, &entry.severity);
        let det_set: HashSet<&str> = entry.detected_by.iter().map(String::as_str).collect();
        let mut dissenting: Vec<String> = roster
            .iter()
            .filter(|n| !det_set.contains(n.as_str()))
            .cloned()
            .collect();
        dissenting.sort();
        dissenting.dedup();

        results.push(ConsensusFinding {
            message: entry.message.clone(),
            file: entry.file.clone(),
            line: entry.line,
            end_line: entry.end_line,
            severity: entry.severity.clone(),
            category: entry.category.clone(),
            detected_by: entry.detected_by.clone(),
            evidence: entry.evidence.clone(),
            confidence_max,
            confidence_avg: entry.confidence_avg(),
            agreement_count,
            total_validators: total,
            agreement_ratio,
            consensus_score,
            consensus_level: level,
            dissenting,
        });
    }

    results.sort_by(|a, b| {
        (-severity_rank(&a.severity))
            .cmp(&(-severity_rank(&b.severity)))
            .then_with(|| {
                b.consensus_score
                    .partial_cmp(&a.consensus_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.file.as_deref().unwrap_or("").cmp(b.file.as_deref().unwrap_or("")))
            .then_with(|| {
                a.line
                    .unwrap_or(1_000_000_000)
                    .cmp(&b.line.unwrap_or(1_000_000_000))
            })
            .then_with(|| a.message.cmp(&b.message))
    });
    results
}

fn classify_consensus_level(
    agreement_ratio: f64,
    confidence: f64,
    severity: &str,
) -> ConsensusLevel {
    if agreement_ratio >= 0.60 && confidence >= 0.65 {
        return ConsensusLevel::Confirmed;
    }
    if agreement_ratio >= 0.40 && confidence >= 0.55 {
        return ConsensusLevel::Likely;
    }
    if agreement_ratio >= 0.20 || matches!(severity, "critical" | "high") {
        return ConsensusLevel::NeedsVerification;
    }
    ConsensusLevel::Unverified
}

pub fn format_consensus_markdown(scored: &[ConsensusFinding], title: Option<&str>) -> String {
    if scored.is_empty() {
        return "No consensus findings.\n".into();
    }
    let mut lines: Vec<String> = Vec::new();
    if let Some(t) = title {
        let tr = t.trim();
        if !tr.is_empty() {
            lines.push(format!("# {tr}"));
            lines.push(String::new());
        }
    }

    for level in LEVEL_ORDER {
        let items: Vec<&ConsensusFinding> = scored
            .iter()
            .filter(|f| f.consensus_level == level)
            .collect();
        if items.is_empty() {
            continue;
        }
        lines.push(format!("## {}", level.heading()));
        for item in items {
            let mut location = String::new();
            if let Some(f) = &item.file {
                if !f.is_empty() {
                    location.push(' ');
                    location.push_str(f);
                    if let Some(ln) = item.line {
                        location.push_str(&format!(":{ln}"));
                    }
                }
            }
            let sev = item.severity.to_ascii_uppercase();
            lines.push(
                format!("- [{sev}]{location} {}", item.message)
                    .trim_end()
                    .to_string(),
            );
            let by = if item.detected_by.is_empty() {
                "—".to_string()
            } else {
                item.detected_by.join(", ")
            };
            lines.push(format!(
                "  consensus: {:.2} · agreement: {}/{} · detected_by: {by}",
                item.consensus_score, item.agreement_count, item.total_validators
            ));
            if !item.evidence.is_empty() {
                lines.push("  evidence:".into());
                let mut pairs: Vec<(&String, &Value)> = item.evidence.iter().collect();
                pairs.sort_by(|a, b| a.0.cmp(b.0));
                for (validator, quote) in pairs {
                    let q = quote.as_str().unwrap_or("");
                    let snippet = redact_text(q).trim().to_string();
                    if snippet.is_empty() {
                        continue;
                    }
                    let first_line = splitlines(&snippet).into_iter().next().unwrap_or_default();
                    lines.push(format!("    {validator}: {}", head(&first_line, 160)));
                }
            }
        }
        lines.push(String::new());
    }

    let rendered = format!("{}\n", lines.join("\n").trim_end());
    redact_text(&rendered)
}
