//! `lope/verdict_repair.py`: Deterministic repair for validator responses that omit the VERDICT block.

pub use lope_providers::models::{PhaseVerdict, VerdictStatus};
use lope_providers::pyre::{is_match, re, search, splitlines};
use std::collections::HashMap;
use std::sync::OnceLock;

pub const ENV_REPAIR_TIMEOUT: &str = "LOPE_REPAIR_TIMEOUT";
pub const DEFAULT_REPAIR_TIMEOUT: f64 = 30.0;
pub const PARSER_VERSION: &str = "verdict-repair/1";

const ALLOWED_SECTIONS: &[&str] = &["RATIONALE", "REQUIRED_FIXES", "REQUIRED FIXES", "NICE_TO_HAVE"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorCategory {
    MissingVerdictBlock,
    UnknownEnum,
    Transport,
    Timeout,
    ProcessExit,
    OutputLimit,
}

impl ParseErrorCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            ParseErrorCategory::MissingVerdictBlock => "missing-verdict-block",
            ParseErrorCategory::UnknownEnum => "unknown-enum",
            ParseErrorCategory::Transport => "transport",
            ParseErrorCategory::Timeout => "timeout",
            ParseErrorCategory::ProcessExit => "process-exit",
            ParseErrorCategory::OutputLimit => "output-limit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairStatus {
    Accepted,
    RejectedProse,
    RejectedInvalid,
    RejectedTimeout,
    RejectedUnsupported,
    RejectedProcess,
    RejectedNoOriginal,
}

impl RepairStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RepairStatus::Accepted => "accepted",
            RepairStatus::RejectedProse => "rejected-prose",
            RepairStatus::RejectedInvalid => "rejected-invalid",
            RepairStatus::RejectedTimeout => "rejected-timeout",
            RepairStatus::RejectedUnsupported => "rejected-unsupported",
            RepairStatus::RejectedProcess => "rejected-process",
            RepairStatus::RejectedNoOriginal => "rejected-no-original",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepairOutcome {
    pub status: RepairStatus,
    pub verdict: Option<PhaseVerdict>,
    pub reason: String,
}

impl RepairOutcome {
    pub fn accepted(&self) -> bool {
        self.status == RepairStatus::Accepted
    }
}

pub fn repair_timeout(env: Option<&HashMap<String, String>>) -> f64 {
    let raw = match env {
        Some(m) => m.get(ENV_REPAIR_TIMEOUT).cloned(),
        None => std::env::var(ENV_REPAIR_TIMEOUT).ok(),
    };
    let Some(s) = raw else { return DEFAULT_REPAIR_TIMEOUT };
    match s.trim().parse::<f64>() {
        Ok(v) if v > 0.0 => v,
        _ => DEFAULT_REPAIR_TIMEOUT,
    }
}

pub fn is_repairable(category: Option<ParseErrorCategory>) -> bool {
    category == Some(ParseErrorCategory::MissingVerdictBlock)
}

pub fn build_repair_prompt(original_response: &str, _validator_name: &str) -> Result<String, String> {
    if original_response.trim().is_empty() {
        return Err(
            "repair requires the original response; extraction without it would let the validator fabricate a verdict"
                .into(),
        );
    }
    Ok(format!(
        "Below is a code-review response you produced. It is missing the required VERDICT block.\n\n\
         Your task is pure extraction. Read the response and express the conclusion it already contains. \
         Do NOT perform new analysis, do NOT re-review anything, and do NOT reach a conclusion the text does not \
         support. If the response does not clearly reach a conclusion, answer INCONCLUSIVE.\n\n\
         --- BEGIN RESPONSE ---\n\
         {}\n\
         --- END RESPONSE ---\n\n\
         Output exactly this format and nothing else:\n\n\
         VERDICT: <PASS|NEEDS_FIX|FAIL|INCONCLUSIVE>\n\
         RATIONALE:\n\
         <one or two sentences drawn from the response above>\n\
         REQUIRED_FIXES:\n\
         - <one line per fix named in the response; omit this section if none>\n\n\
         No preamble, no explanation, no code fences, no closing remarks.",
        original_response.trim()
    ))
}

/// `models.py:parse_verdict_block`
pub fn parse_verdict_block(text: &str, validator_name: &str, fallback_duration: f64) -> PhaseVerdict {
    static VERDICT_RE: OnceLock<fancy_regex::Regex> = OnceLock::new();
    static RATIONALE_RE: OnceLock<fancy_regex::Regex> = OnceLock::new();
    static FIXES_RE: OnceLock<fancy_regex::Regex> = OnceLock::new();

    let v_re = VERDICT_RE.get_or_init(|| {
        re(r"(?m)^\s*VERDICT:\s*(?P<status>PASS|NEEDS_FIX|FAIL|INFRA_ERROR|INCONCLUSIVE)\s*(?:\(confidence=(?P<conf>[0-9.]+)\s*,\s*(?P<dur>[0-9.]+)s\))?\s*$")
    });
    let r_re = RATIONALE_RE.get_or_init(|| {
        re(r"(?ms)^\s*RATIONALE:\s*$\n(?P<body>(?:.*\n)*?)(?=^\s*(?:REQUIRED_FIXES|NICE_TO_HAVE|VERDICT)|\z)")
    });
    let f_re = FIXES_RE.get_or_init(|| {
        re(r"(?ms)^\s*REQUIRED[_ ]FIXES:\s*$\n(?P<body>(?:.*\n)*?)(?=^\s*(?:NICE_TO_HAVE|VERDICT)|\z)")
    });

    let Some(m) = search(v_re, text) else {
        return PhaseVerdict::new(
            VerdictStatus::InfraError,
            "no VERDICT: block found in validator response",
            fallback_duration,
            validator_name,
        );
    };
    let status_str = m.name("status").map(|c| c.as_str()).unwrap_or("");
    let Some(status) = VerdictStatus::parse(status_str) else {
        return PhaseVerdict::new(
            VerdictStatus::InfraError,
            format!("unknown verdict token: {status_str:?}"),
            fallback_duration,
            validator_name,
        );
    };
    let confidence = m.name("conf").and_then(|c| c.as_str().parse::<f64>().ok()).unwrap_or(0.0);
    let duration = m.name("dur").and_then(|c| c.as_str().parse::<f64>().ok()).unwrap_or(fallback_duration);

    let rationale = search(r_re, text)
        .and_then(|c| c.name("body"))
        .map(|b| {
            splitlines(b.as_str())
                .into_iter()
                .map(|l| l.trim_end())
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        })
        .unwrap_or_default();

    let mut required_fixes = Vec::new();
    if let Some(fm) = search(f_re, text).and_then(|c| c.name("body")) {
        for line in splitlines(fm.as_str()) {
            let s = line.trim();
            if let Some(rest) = s.strip_prefix("- ").or_else(|| s.strip_prefix("* ")) {
                required_fixes.push(rest.trim().to_string());
            }
        }
    }

    PhaseVerdict {
        status,
        confidence,
        rationale,
        required_fixes,
        nice_to_have: Vec::new(),
        duration_seconds: duration,
        validator_name: validator_name.to_string(),
        stage: None,
        evidence_gate_triggered: false,
    }
}

pub fn evaluate_repair_reply(
    text: &str,
    validator_name: &str,
    fallback_duration: f64,
) -> RepairOutcome {
    static VERDICT_LINE_RE: OnceLock<fancy_regex::Regex> = OnceLock::new();
    static SECTION_RE: OnceLock<fancy_regex::Regex> = OnceLock::new();
    static FENCE_RE: OnceLock<fancy_regex::Regex> = OnceLock::new();

    if text.trim().is_empty() {
        return RepairOutcome {
            status: RepairStatus::RejectedInvalid,
            verdict: None,
            reason: "empty reply".into(),
        };
    }

    let vl_re = VERDICT_LINE_RE.get_or_init(|| {
        re(r"^\s*VERDICT:\s*(?P<status>[A-Z_]+)\s*(?:\(confidence=(?P<conf>[0-9.]+)\s*,\s*(?P<dur>[0-9.]+)s\))?\s*$")
    });
    let sec_re = SECTION_RE.get_or_init(|| re(r"^\s*(?P<name>[A-Z_ ]+):\s*$"));
    let fen_re = FENCE_RE.get_or_init(|| re(r"^\s*```[a-zA-Z0-9_-]*\s*$"));

    let mut lines: Vec<&str> = splitlines(text.trim());
    if lines.len() >= 2 && is_match(fen_re, lines[0]) && is_match(fen_re, lines[lines.len() - 1]) {
        lines = lines[1..lines.len() - 1].to_vec();
    }

    let verdict_lines: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| is_match(vl_re, line).then_some(i))
        .collect();
    if verdict_lines.is_empty() {
        return RepairOutcome {
            status: RepairStatus::RejectedInvalid,
            verdict: None,
            reason: "no VERDICT line in repair reply".into(),
        };
    }
    if verdict_lines.len() > 1 {
        return RepairOutcome {
            status: RepairStatus::RejectedInvalid,
            verdict: None,
            reason: format!("{} VERDICT lines; expected exactly one", verdict_lines.len()),
        };
    }
    if verdict_lines[0] != 0 {
        return RepairOutcome {
            status: RepairStatus::RejectedProse,
            verdict: None,
            reason: "text precedes the VERDICT line".into(),
        };
    }

    let status_token = search(vl_re, lines[0])
        .and_then(|c| c.name("status"))
        .map(|m| m.as_str())
        .unwrap_or("");
    let Some(parsed_status) = VerdictStatus::parse(status_token) else {
        return RepairOutcome {
            status: RepairStatus::RejectedInvalid,
            verdict: None,
            reason: format!("unknown verdict token: {status_token:?}"),
        };
    };
    if parsed_status == VerdictStatus::InfraError {
        return RepairOutcome {
            status: RepairStatus::RejectedInvalid,
            verdict: None,
            reason: "repair may not return INFRA_ERROR".into(),
        };
    }

    let mut in_section = false;
    for line in &lines[1..] {
        let stripped = line.trim();
        if stripped.is_empty() {
            in_section = false;
            continue;
        }
        if let Some(caps) = search(sec_re, line) {
            let name = caps.name("name").map(|m| m.as_str().trim()).unwrap_or("");
            if !ALLOWED_SECTIONS.contains(&name) {
                return RepairOutcome {
                    status: RepairStatus::RejectedProse,
                    verdict: None,
                    reason: format!("unexpected section: {name:?}"),
                };
            }
            in_section = true;
            continue;
        }
        if !in_section {
            return RepairOutcome {
                status: RepairStatus::RejectedProse,
                verdict: None,
                reason: "prose after VERDICT line outside any allowed section".into(),
            };
        }
    }

    let block_text = format!("{}\n", lines.join("\n"));
    let verdict = parse_verdict_block(&block_text, validator_name, fallback_duration);
    if verdict.status == VerdictStatus::InfraError {
        return RepairOutcome {
            status: RepairStatus::RejectedInvalid,
            verdict: None,
            reason: format!("shared parser rejected the block: {}", verdict.rationale),
        };
    }

    RepairOutcome {
        status: RepairStatus::Accepted,
        verdict: Some(verdict),
        reason: String::new(),
    }
}
