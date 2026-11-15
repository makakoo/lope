//! Shared sprint domain vocabulary (`models.py`, Sprint S7).

pub use lope_providers::models::{PhaseVerdict, ValidatorResult, VerdictStatus};
use regex::Regex;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

pub trait VerdictStatusExt {
    fn from_token(s: &str) -> Option<VerdictStatus>;
    fn is_terminal(self) -> bool;
}

impl VerdictStatusExt for VerdictStatus {
    fn from_token(s: &str) -> Option<Self> {
        VerdictStatus::parse(s.trim())
    }

    fn is_terminal(self) -> bool {
        matches!(
            self,
            VerdictStatus::Pass | VerdictStatus::Fail | VerdictStatus::Inconclusive
        )
    }
}

pub trait PhaseVerdictExt {
    fn is_pass(&self) -> bool;
    fn needs_retry(&self) -> bool;
}

impl PhaseVerdictExt for PhaseVerdict {
    fn is_pass(&self) -> bool {
        self.status == VerdictStatus::Pass
    }

    fn needs_retry(&self) -> bool {
        self.status == VerdictStatus::NeedsFix
    }
}

pub fn new_validator_result(validator_name: impl Into<String>, verdict: PhaseVerdict) -> ValidatorResult {
    ValidatorResult {
        validator_name: validator_name.into(),
        verdict,
        raw_response: String::new(),
        error: String::new(),
        flag_error_hint: String::new(),
        repair_attempted: false,
        repair_status: None,
        repaired_response: String::new(),
        initial_parse_status: None,
        parse_error_category: None,
    }
}

#[derive(Debug, Clone)]
pub struct Phase {
    pub index: usize,
    pub name: String,
    pub goal: String,
    pub criteria: Vec<String>,
    pub artifacts: Vec<String>,
    pub checks: Vec<String>,
    pub verdict: Option<PhaseVerdict>,
}

impl Phase {
    pub fn is_validated(&self) -> bool {
        self.verdict.is_some()
    }

    pub fn is_pass(&self) -> bool {
        self.verdict.as_ref().is_some_and(|v| v.is_pass())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DomainConfig {
    pub role: &'static str,
    pub artifact_label: &'static str,
    pub check_label: &'static str,
    pub review_task: &'static str,
}

pub fn domain_config(domain: &str) -> DomainConfig {
    match domain {
        "business" => DomainConfig {
            role: "senior operations lead",
            artifact_label: "Deliverables",
            check_label: "Success Metrics",
            review_task: "Review listed deliverables. Check each criterion against evidence. Find gaps in timeline, budget, targeting, or success metrics.",
        },
        "research" => DomainConfig {
            role: "principal researcher",
            artifact_label: "Artifacts",
            check_label: "Validation Criteria",
            review_task: "Review methodology and data sources. Check each criterion against research standards. Find gaps in sampling, validity, ethics, or analysis plan.",
        },
        _ => DomainConfig {
            role: "senior staff engineer",
            artifact_label: "Files",
            check_label: "Tests",
            review_task: "Read listed files. Check each criterion against real code. Find bugs, regressions, broken invariants.",
        },
    }
}

#[derive(Debug, Clone)]
pub struct SprintDoc {
    pub slug: String,
    pub title: String,
    pub origin: String,
    pub domain: String,
    pub phases: Vec<Phase>,
    pub path: Option<String>,
}

impl SprintDoc {
    pub fn domain_config(&self) -> DomainConfig {
        domain_config(&self.domain)
    }

    pub fn get_phase(&self, index: usize) -> Option<&Phase> {
        self.phases.iter().find(|p| p.index == index)
    }

    pub fn get_phase_mut(&mut self, index: usize) -> Option<&mut Phase> {
        self.phases.iter_mut().find(|p| p.index == index)
    }

    pub fn to_markdown(&self) -> String {
        let dc = self.domain_config();
        let mut lines = Vec::new();
        lines.push(format!("# {}", self.title));
        lines.push(String::new());
        if self.domain != "engineering" {
            lines.push(format!("**Domain:** {}", self.domain));
            lines.push(String::new());
        }
        if !self.origin.is_empty() {
            lines.push("## Origin".to_string());
            lines.push(String::new());
            lines.push(self.origin.clone());
            lines.push(String::new());
        }
        lines.push("## Phases".to_string());
        lines.push(String::new());
        let mut sorted_phases = self.phases.clone();
        sorted_phases.sort_by_key(|p| p.index);
        for phase in sorted_phases {
            lines.push(format!("### Phase {}: {}", phase.index, phase.name));
            lines.push(String::new());
            lines.push(format!("**Goal:** {}", phase.goal));
            lines.push(String::new());
            if !phase.criteria.is_empty() {
                lines.push("**Criteria:**".to_string());
                for c in &phase.criteria {
                    lines.push(format!("- {c}"));
                }
                lines.push(String::new());
            }
            if !phase.artifacts.is_empty() {
                lines.push(format!("**{}:**", dc.artifact_label));
                for f in &phase.artifacts {
                    lines.push(format!("- {f}"));
                }
                lines.push(String::new());
            }
            if !phase.checks.is_empty() {
                lines.push(format!("**{}:**", dc.check_label));
                for t in &phase.checks {
                    lines.push(format!("- {t}"));
                }
                lines.push(String::new());
            }
        }
        format!("{}\n", lines.join("\n").trim_end())
    }

    pub fn from_markdown(text: &str, path: Option<&str>) -> Self {
        static PHASE_RE: OnceLock<Regex> = OnceLock::new();
        let phase_re =
            PHASE_RE.get_or_init(|| Regex::new(r"^###\s+Phase\s+(\d+)\s*:\s*(.+?)\s*$").unwrap());

        let mut title = String::new();
        let mut origin_lines: Vec<String> = Vec::new();
        let mut phases: Vec<Phase> = Vec::new();
        let mut current_idx: Option<usize> = None;
        let mut current_list: Option<&'static str> = None;
        let mut in_origin = false;

        for raw_line in text.lines() {
            let line = raw_line.trim_end();
            if line.starts_with("# ") && title.is_empty() {
                title = line[2..].trim().to_string();
                continue;
            }
            if line.starts_with("## Origin") {
                in_origin = true;
                current_idx = None;
                current_list = None;
                continue;
            }
            if let Some(cap) = phase_re.captures(line) {
                in_origin = false;
                current_list = None;
                let index: usize = cap.get(1).unwrap().as_str().parse().unwrap_or(1);
                let name = cap.get(2).unwrap().as_str().trim().to_string();
                phases.push(Phase {
                    index,
                    name,
                    goal: String::new(),
                    criteria: Vec::new(),
                    artifacts: Vec::new(),
                    checks: Vec::new(),
                    verdict: None,
                });
                current_idx = Some(phases.len() - 1);
                continue;
            }
            if line.starts_with("## ") {
                in_origin = false;
                current_list = None;
                continue;
            }
            if in_origin {
                if !line.is_empty() {
                    origin_lines.push(line.to_string());
                }
                continue;
            }
            let Some(idx) = current_idx else {
                continue;
            };
            if let Some(rest) = line.strip_prefix("**Goal:**") {
                phases[idx].goal = rest.trim().to_string();
                current_list = None;
                continue;
            }
            if line.starts_with("**Criteria:**") {
                current_list = Some("criteria");
                continue;
            }
            if ["Files", "Deliverables", "Artifacts"]
                .iter()
                .any(|lbl| line.starts_with(&format!("**{lbl}:**")))
            {
                current_list = Some("artifacts");
                continue;
            }
            if ["Tests", "Success Metrics", "Validation Criteria", "Checks"]
                .iter()
                .any(|lbl| line.starts_with(&format!("**{lbl}:**")))
            {
                current_list = Some("checks");
                continue;
            }
            if let Some(rest) = line.strip_prefix("- ") {
                if let Some(list_name) = current_list {
                    let item = rest.trim().trim_matches('`').to_string();
                    match list_name {
                        "criteria" => phases[idx].criteria.push(item),
                        "artifacts" => phases[idx].artifacts.push(item),
                        "checks" => phases[idx].checks.push(item),
                        _ => {}
                    }
                    continue;
                }
            }
            if line.is_empty() {
                current_list = None;
            }
        }

        let mut domain = "engineering".to_string();
        for raw_line in text.lines() {
            let trimmed = raw_line.trim();
            if trimmed.starts_with("**Domain:**") {
                if let Some((_, rhs)) = trimmed.split_once(":**") {
                    domain = rhs.trim().to_ascii_lowercase();
                }
                break;
            }
        }

        phases.sort_by_key(|p| p.index);
        let slug = slug_from_title(&title);
        Self {
            slug,
            title,
            origin: origin_lines.join("\n").trim().to_string(),
            domain,
            phases,
            path: path.map(str::to_string),
        }
    }

    pub fn save(&mut self, path: &Path) -> std::io::Result<String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, self.to_markdown())?;
        let s = path.display().to_string();
        self.path = Some(s.clone());
        Ok(s)
    }
}

pub fn slug_from_title(title: &str) -> String {
    if title.is_empty() {
        return "untitled".to_string();
    }
    let mut t = title.trim().to_ascii_lowercase();
    if let Some(stripped) = t.strip_prefix("sprint-") {
        t = stripped.to_string();
    }
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in t.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed
    }
}

#[derive(Debug, Clone)]
pub struct Round {
    pub number: usize,
    pub proposer: String,
    pub text: String,
    pub verdict: Option<PhaseVerdict>,
}

#[derive(Debug, Clone)]
pub struct EscalationRequired {
    pub reason: String,
    pub phase_index: usize,
    pub phase_name: String,
    pub rounds: Vec<Round>,
    pub last_verdict: Option<PhaseVerdict>,
}

#[derive(Debug, Clone)]
pub struct Proposal {
    pub round_number: usize,
    pub goal: String,
    pub text: String,
    pub prior_feedback: String,
}

#[derive(Debug, Clone)]
pub struct ExecutionReport {
    pub sprint_doc: SprintDoc,
    pub phase_verdicts: Vec<PhaseVerdict>,
    pub ok: bool,
    pub error: String,
    pub total_duration_seconds: f64,
}

impl ExecutionReport {
    pub fn scorecard(&self) -> String {
        let mut lines = vec![format!("Sprint: {}", self.sprint_doc.title)];
        let mut phases = self.sprint_doc.phases.clone();
        phases.sort_by_key(|p| p.index);
        for phase in phases {
            match &phase.verdict {
                None => lines.push(format!("  P{} {}: (not run)", phase.index, phase.name)),
                Some(v) => lines.push(format!(
                    "  P{} {}: {} conf={:.2} {:.0}s",
                    phase.index,
                    phase.name,
                    v.status.as_str(),
                    v.confidence,
                    v.duration_seconds
                )),
            }
        }
        let status = if self.ok { "OK" } else { "ESCALATED" };
        lines.push(format!("Overall: {status}"));
        if !self.error.is_empty() {
            lines.push(format!("Error: {}", self.error));
        }
        lines.join("\n")
    }

    pub fn confidence_average(&self) -> f64 {
        let vs: Vec<f64> = self
            .phase_verdicts
            .iter()
            .filter(|v| v.confidence > 0.0)
            .map(|v| v.confidence)
            .collect();
        if vs.is_empty() {
            0.0
        } else {
            vs.iter().sum::<f64>() / vs.len() as f64
        }
    }

    pub fn count(&self, status: VerdictStatus) -> usize {
        self.phase_verdicts
            .iter()
            .filter(|v| v.status == status)
            .count()
    }
}

pub fn parse_verdict_block(
    text: &str,
    validator_name: &str,
    fallback_duration: f64,
) -> PhaseVerdict {
    static VERDICT_RE: OnceLock<Regex> = OnceLock::new();
    let verdict_re = VERDICT_RE.get_or_init(|| {
        Regex::new(
            r"(?m)^\s*VERDICT:\s*(?P<status>PASS|NEEDS_FIX|FAIL|INFRA_ERROR|INCONCLUSIVE)\s*(?:\(confidence=(?P<conf>[0-9.]+)\s*,\s*(?P<dur>[0-9.]+)s\))?\s*$",
        )
        .unwrap()
    });

    let Some(m) = verdict_re.captures(text) else {
        return PhaseVerdict::new(
            VerdictStatus::InfraError,
            "no VERDICT: block found in validator response",
            fallback_duration,
            validator_name,
        );
    };

    let status_tok = m.name("status").map(|x| x.as_str()).unwrap_or("");
    let Some(status) = VerdictStatus::from_token(status_tok) else {
        return PhaseVerdict::new(
            VerdictStatus::InfraError,
            format!("unknown verdict token: {status_tok:?}"),
            fallback_duration,
            validator_name,
        );
    };

    let confidence = m
        .name("conf")
        .and_then(|x| x.as_str().parse::<f64>().ok())
        .unwrap_or(0.0);
    let duration = m
        .name("dur")
        .and_then(|x| x.as_str().parse::<f64>().ok())
        .unwrap_or(fallback_duration);

    let rationale = extract_section_body(text, "RATIONALE:", &["REQUIRED_FIXES:", "REQUIRED FIXES:", "NICE_TO_HAVE:", "VERDICT:"]);
    let fixes_body = extract_section_body(text, "REQUIRED_FIXES:", &["NICE_TO_HAVE:", "VERDICT:"]);
    let fixes_body = if fixes_body.is_empty() {
        extract_section_body(text, "REQUIRED FIXES:", &["NICE_TO_HAVE:", "VERDICT:"])
    } else {
        fixes_body
    };
    let mut required_fixes = Vec::new();
    for line in fixes_body.lines() {
        let stripped = line.trim();
        if let Some(rest) = stripped.strip_prefix("- ") {
            required_fixes.push(rest.trim().to_string());
        } else if let Some(rest) = stripped.strip_prefix("* ") {
            required_fixes.push(rest.trim().to_string());
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

fn extract_section_body(text: &str, header: &str, stop_headers: &[&str]) -> String {
    let mut capturing = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !capturing {
            if trimmed == header {
                capturing = true;
            }
            continue;
        }
        if stop_headers.iter().any(|h| trimmed.starts_with(h)) {
            break;
        }
        lines.push(line.trim_end());
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprint_doc_roundtrip_and_slug() {
        let md = "# SPRINT-AUTH-MIDDLEWARE\n\n## Origin\n\nAdd JWT auth\n\n## Phases\n\n### Phase 1: scaffold\n\n**Goal:** Create module\n\n**Criteria:**\n- Compiles cleanly\n\n**Files:**\n- `src/auth.rs`\n\n**Tests:**\n- `cargo test`\n";
        let doc = SprintDoc::from_markdown(md, None);
        assert_eq!(doc.slug, "auth-middleware");
        assert_eq!(doc.phases.len(), 1);
        assert_eq!(doc.phases[0].artifacts, vec!["src/auth.rs"]);
        assert!(doc.to_markdown().contains("### Phase 1: scaffold"));
    }

    #[test]
    fn parse_verdict_block_extracts_fields() {
        let text = "VERDICT: NEEDS_FIX (confidence=0.80, 12.5s)\nRATIONALE:\nMissing error check\nREQUIRED_FIXES:\n- Handle EOF\n- Add test\n";
        let v = parse_verdict_block(text, "mock-a", 1.0);
        assert_eq!(v.status, VerdictStatus::NeedsFix);
        assert!((v.confidence - 0.80).abs() < 1e-6);
        assert!((v.duration_seconds - 12.5).abs() < 1e-6);
        assert_eq!(v.rationale, "Missing error check");
        assert_eq!(v.required_fixes, vec!["Handle EOF", "Add test"]);
    }
}
