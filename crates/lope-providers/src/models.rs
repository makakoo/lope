//! `models.py`: verdict primitives.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictStatus {
    Pass,
    NeedsFix,
    Fail,
    InfraError,
    Inconclusive,
}

impl VerdictStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            VerdictStatus::Pass => "PASS",
            VerdictStatus::NeedsFix => "NEEDS_FIX",
            VerdictStatus::Fail => "FAIL",
            VerdictStatus::InfraError => "INFRA_ERROR",
            VerdictStatus::Inconclusive => "INCONCLUSIVE",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "PASS" => VerdictStatus::Pass,
            "NEEDS_FIX" => VerdictStatus::NeedsFix,
            "FAIL" => VerdictStatus::Fail,
            "INFRA_ERROR" => VerdictStatus::InfraError,
            "INCONCLUSIVE" => VerdictStatus::Inconclusive,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhaseVerdict {
    pub status: VerdictStatus,
    pub confidence: f64,
    pub rationale: String,
    pub required_fixes: Vec<String>,
    pub nice_to_have: Vec<String>,
    pub duration_seconds: f64,
    pub validator_name: String,
    pub stage: Option<String>,
    pub evidence_gate_triggered: bool,
}

impl PhaseVerdict {
    pub fn new(status: VerdictStatus, rationale: impl Into<String>, duration: f64, validator: &str) -> Self {
        PhaseVerdict {
            status,
            confidence: 0.0,
            rationale: rationale.into(),
            required_fixes: vec![],
            nice_to_have: vec![],
            duration_seconds: duration,
            validator_name: validator.to_string(),
            stage: None,
            evidence_gate_triggered: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatorResult {
    pub validator_name: String,
    pub verdict: PhaseVerdict,
    pub raw_response: String,
    pub error: String,
    pub flag_error_hint: String,
    pub repair_attempted: bool,
    pub repair_status: Option<String>,
    pub repaired_response: String,
    pub initial_parse_status: Option<String>,
    pub parse_error_category: Option<String>,
}

impl ValidatorResult {
    pub fn ok(&self) -> bool {
        self.error.is_empty() && !matches!(self.verdict.status, VerdictStatus::InfraError | VerdictStatus::Inconclusive)
    }
}
