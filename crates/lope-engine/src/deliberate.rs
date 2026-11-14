//! Council-style deliberation (`deliberation.py`, Sprint S6, `FR31`, `A14`).
//!
//! `lope deliberate` is a structured adversarial-reasoning verb. Nothing in this
//! module modifies source files, touches git, or runs commands beyond the
//! injected `generate` callback.

use lope_core::pyjson::{self, Opts};
use lope_core::redaction::redact_text;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub const DEPTHS: &[&str] = &["quick", "standard", "deep"];
pub const DEFAULT_DEPTH: &str = "standard";

pub const HUMAN_QUESTION_MODES: &[&str] = &["never", "blocking", "always"];
pub const DEFAULT_HUMAN_QUESTIONS: &str = "never";

pub const MINORITY_HIGH_SEVERITY: &[&str] = &["high", "critical", "blocker"];
pub const MAX_SCENARIO_BYTES: usize = 48 * 1024;
pub const MAX_TURN_BYTES: usize = 32 * 1024;
pub const MAX_PEER_BLOCK_BYTES: usize = 64 * 1024;
pub const DEFAULT_STAGE_WORKERS: usize = 5;

const BOUNDARY_MARKER: &str = "\n...[bounded council material; head and tail retained]...\n";
pub const SCENARIO_BLOCK_HEADER: &str = "<<< Scenario >>>";
pub const SCENARIO_BLOCK_FOOTER: &str = "<<< End scenario >>>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliberationError {
    /// `DeliberationInconclusive(RuntimeError)` — exits 1 in CLI.
    Inconclusive(String),
    /// `ValueError` or `KeyError` — exits 2 in CLI.
    Value(String),
    /// Uncaught runtime error during synthesis or IO.
    Runtime(String),
}

impl std::fmt::Display for DeliberationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeliberationError::Inconclusive(m)
            | DeliberationError::Value(m)
            | DeliberationError::Runtime(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for DeliberationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub sections: &'static [&'static str],
    pub rubric: &'static [&'static str],
    pub position_intro: &'static str,
    pub critique_intro: &'static str,
    pub revision_intro: &'static str,
    pub synthesis_intro: &'static str,
    pub rubric_intro: &'static str,
}

impl TemplateSpec {
    pub fn required_section_block(&self) -> String {
        self.sections
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn rubric_block(&self) -> String {
        self.rubric
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CouncilTurn {
    pub stage: String,
    pub validator: String,
    pub text: String,
    pub label: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RubricVerdict {
    pub validator: String,
    pub label: Option<String>,
    pub status: String,
    pub severity: String,
    pub text: String,
    pub objections: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DeliberationRun {
    pub template: TemplateSpec,
    pub scenario: String,
    pub validators: Vec<String>,
    pub primary: String,
    pub depth: String,
    pub anonymous: bool,
    pub turns: Vec<CouncilTurn>,
    pub synthesis: String,
    pub rubric: Vec<RubricVerdict>,
    pub minority_report: String,
    pub decision_log: String,
    pub output_dir: Option<PathBuf>,
    pub started_at: String,
    pub finished_at: String,
}

impl DeliberationRun {
    pub fn to_dict(&self) -> Value {
        let scenario_preview: String = self.scenario.chars().take(240).collect();
        let rubric_arr: Vec<Value> = self
            .rubric
            .iter()
            .map(|r| {
                json!({
                    "validator": r.validator,
                    "label": r.label,
                    "status": r.status,
                    "severity": r.severity,
                    "objections": r.objections,
                })
            })
            .collect();
        let turns_arr: Vec<Value> = self
            .turns
            .iter()
            .map(|t| {
                let preview: String = t.text.chars().take(240).collect();
                json!({
                    "stage": t.stage,
                    "validator": t.validator,
                    "label": t.label,
                    "text_preview": preview,
                })
            })
            .collect();
        json!({
            "template": self.template.name,
            "scenario_preview": scenario_preview,
            "validators": self.validators,
            "primary": self.primary,
            "depth": self.depth,
            "anonymous": self.anonymous,
            "synthesis_present": !self.synthesis.trim().is_empty(),
            "rubric": rubric_arr,
            "minority_report_present": !self.minority_report.trim().is_empty(),
            "turns": turns_arr,
            "started_at": self.started_at,
            "finished_at": self.finished_at,
            "output_dir": self.output_dir.as_ref().map(|p| p.display().to_string()),
        })
    }
}

pub static TEMPLATES: &[TemplateSpec] = &[
    TemplateSpec {
        name: "adr",
        title: "Architecture Decision Record",
        sections: &[
            "Context",
            "Decision",
            "Consequences",
            "Alternatives Considered",
        ],
        rubric: &[
            "Decision is stated explicitly with one-line rationale.",
            "At least two alternatives are documented with their trade-offs.",
            "Consequences include both positive and negative outcomes.",
            "Assumptions are explicit, not implied.",
        ],
        position_intro: "You are drafting an Architecture Decision Record (ADR). Read the scenario and write your independent position. Cover: the underlying problem, the decision you recommend, the alternatives you considered, and the trade-offs of each.",
        critique_intro: "You are critiquing peer ADR drafts. Identify gaps, hidden assumptions, missing alternatives, and unstated consequences. Be direct. Cite specific lines or claims.",
        revision_intro: "You are revising your ADR position based on the critiques below. Fold in valid corrections, defend positions you still stand by, and explicitly note any objections you reject.",
        synthesis_intro: "You are synthesizing the council's final ADR using the rubric below. Produce a single coherent record with the exact section headings listed.",
        rubric_intro: "Score the synthesized ADR against the rubric. Reply with one line of the form ``VERDICT: PASS`` or ``VERDICT: NEEDS_FIX``, followed by a one-line ``SEVERITY: low|medium|high``, then bullet objections (one per line, prefix ``- ``).",
    },
    TemplateSpec {
        name: "prd",
        title: "Product Requirements Document",
        sections: &[
            "Problem",
            "Users and Use Cases",
            "Goals and Non-Goals",
            "Solution Sketch",
            "Acceptance Criteria",
            "Risks",
        ],
        rubric: &[
            "Problem is grounded in a specific user pain, not a feature wish.",
            "Users / use cases are concrete and bounded.",
            "Non-goals are explicit and ruthless.",
            "Acceptance criteria are testable.",
            "At least one major risk is identified.",
        ],
        position_intro: "You are drafting a Product Requirements Document. Read the scenario and write your independent take. Be opinionated about what we should build, who it's for, and what it explicitly won't do.",
        critique_intro: "You are critiquing peer PRD drafts. Look for vague problems, feature creep, untestable acceptance criteria, and missing non-goals. Quote the specific text you take issue with.",
        revision_intro: "Revise your PRD based on the critiques. Tighten scope, sharpen the problem statement, and answer concrete objections.",
        synthesis_intro: "Synthesize the council's PRD using the rubric below. Use the exact section headings listed. Be terse — a PRD is a contract, not a story.",
        rubric_intro: "Score the synthesized PRD against the rubric. Reply with ``VERDICT: PASS`` or ``VERDICT: NEEDS_FIX``, then ``SEVERITY: low|medium|high``, then bullet objections.",
    },
    TemplateSpec {
        name: "rfc",
        title: "Request for Comments",
        sections: &[
            "Summary",
            "Motivation",
            "Detailed Design",
            "Drawbacks",
            "Rationale and Alternatives",
            "Open Questions",
        ],
        rubric: &[
            "Motivation explains why the change is worth doing now.",
            "Detailed design is concrete enough to implement from.",
            "Drawbacks are honest, not perfunctory.",
            "Open Questions section is non-empty.",
        ],
        position_intro: "You are writing your independent position for an RFC. Outline the change you propose, the reasoning, and any open questions you cannot yet answer.",
        critique_intro: "Critique peer RFC drafts. Hunt for hand-waved details, missing alternatives, and incomplete drawbacks. Be specific.",
        revision_intro: "Revise your RFC position based on the critiques.",
        synthesis_intro: "Synthesize the council's final RFC using the rubric and section list below.",
        rubric_intro: "Score the synthesized RFC. ``VERDICT:`` then ``SEVERITY:`` then bullet objections.",
    },
    TemplateSpec {
        name: "build-vs-buy",
        title: "Build vs Buy Analysis",
        sections: &[
            "Decision Statement",
            "Requirements",
            "Build Option",
            "Buy Options",
            "Total Cost of Ownership",
            "Recommendation",
        ],
        rubric: &[
            "At least one build option and at least two buy options are evaluated.",
            "TCO covers a 3-year horizon, not a launch-day estimate.",
            "Recommendation is unambiguous.",
            "Lock-in / exit risk is acknowledged for buy options.",
        ],
        position_intro: "You are writing your independent recommendation on whether to build or buy this capability. Compare options on a 3-year horizon. Be opinionated.",
        critique_intro: "Critique peer build-vs-buy analyses. Press for missing TCO components, ignored vendors, and weakly-defended assumptions.",
        revision_intro: "Revise your build-vs-buy position based on the critiques.",
        synthesis_intro: "Synthesize the council's final build-vs-buy decision using the rubric and sections below. State the recommendation in the first sentence.",
        rubric_intro: "Score the synthesized build-vs-buy artifact. Reply with VERDICT, SEVERITY, and bullet objections.",
    },
    TemplateSpec {
        name: "migration-plan",
        title: "Migration Plan",
        sections: &[
            "Migration Goal",
            "Source and Target State",
            "Phases and Sequence",
            "Rollback Strategy",
            "Validation Gates",
            "Risks",
        ],
        rubric: &[
            "Phases are sequenced and each one is independently shippable or revertible.",
            "Rollback strategy is explicit, not implied.",
            "Validation gates are observable, not subjective.",
            "At least one risk is severe enough to require mitigation.",
        ],
        position_intro: "You are writing your independent migration plan. Outline the source and target state, the phase sequence, the rollback strategy at each phase, and the gates that prove each phase succeeded.",
        critique_intro: "Critique peer migration plans. Find phase ordering errors, missing rollback paths, and validation gates that are not actually observable.",
        revision_intro: "Revise your migration plan based on the critiques.",
        synthesis_intro: "Synthesize the council's final migration plan using the rubric and sections below.",
        rubric_intro: "Score the synthesized migration plan. Reply with VERDICT, SEVERITY, and bullet objections.",
    },
    TemplateSpec {
        name: "incident-review",
        title: "Incident Review",
        sections: &[
            "Incident Summary",
            "Timeline",
            "Root Cause",
            "Contributing Factors",
            "Action Items",
            "Lessons Learned",
        ],
        rubric: &[
            "Root cause is a specific mechanism, not a category.",
            "Action items are owned and have a deadline.",
            "Lessons learned are non-blamey and structural.",
            "Timeline is in UTC and includes detection + mitigation times.",
        ],
        position_intro: "You are writing your independent incident review. Reconstruct the timeline, identify the root cause, list contributing factors, and propose owned action items.",
        critique_intro: "Critique peer incident reviews. Push back on category-level root causes, owner-less action items, and blamey framing.",
        revision_intro: "Revise your incident review based on the critiques.",
        synthesis_intro: "Synthesize the council's final incident review using the rubric and sections below.",
        rubric_intro: "Score the synthesized incident review. Reply with VERDICT, SEVERITY, and bullet objections.",
    },
];

pub fn list_templates() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = TEMPLATES.iter().map(|t| t.name).collect();
    names.sort_unstable();
    names
}

pub fn get_template(name: &str) -> Result<TemplateSpec, DeliberationError> {
    let key = name.trim().to_ascii_lowercase();
    for spec in TEMPLATES {
        if spec.name == key {
            return Ok(spec.clone());
        }
    }
    Err(DeliberationError::Value(format!(
        "Unknown deliberation template: {name:?}. Available: {}",
        list_templates().join(", ")
    )))
}

pub fn anon_label(index: usize) -> String {
    if index < 26 {
        return format!("Response {}", (b'A' + index as u8) as char);
    }
    let first = (index / 26) - 1;
    let second = index % 26;
    format!(
        "Response {}{}",
        (b'A' + (first % 26) as u8) as char,
        (b'A' + second as u8) as char
    )
}

pub fn build_label_map(validators: &[String]) -> BTreeMap<String, String> {
    validators
        .iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), anon_label(i)))
        .collect()
}

fn label_for(name: &str, label_map: &BTreeMap<String, String>, anonymous: bool) -> String {
    if anonymous {
        label_map
            .get(name)
            .cloned()
            .unwrap_or_else(|| "Response ?".to_string())
    } else {
        name.to_string()
    }
}

pub fn wrap_scenario(scenario: &str) -> Result<String, DeliberationError> {
    let redacted = redact_text(scenario);
    let trimmed = redacted.trim();
    let body = if trimmed.is_empty() {
        "(empty scenario)"
    } else {
        trimmed
    };
    if body.len() > MAX_SCENARIO_BYTES {
        return Err(DeliberationError::Value(format!(
            "deliberation scenario exceeds {MAX_SCENARIO_BYTES} UTF-8 bytes; provide a smaller lossless scenario brief"
        )));
    }
    Ok(format!(
        "{SCENARIO_BLOCK_HEADER}\n{body}\n{SCENARIO_BLOCK_FOOTER}\n"
    ))
}

pub fn bounded_text(text: &str, limit: usize) -> String {
    let redacted = redact_text(text);
    let raw = redacted.as_bytes();
    if raw.len() <= limit {
        return redacted;
    }
    let marker_bytes = &BOUNDARY_MARKER.as_bytes()[..BOUNDARY_MARKER.len().min(limit)];
    let keep = limit.saturating_sub(marker_bytes.len());
    let head_len = keep / 2;
    let tail_len = keep - head_len;
    let mut head = &raw[..head_len];
    while !head.is_empty() && std::str::from_utf8(head).is_err() {
        head = &head[..head.len() - 1];
    }
    let head_text = std::str::from_utf8(head).unwrap_or("");

    let mut tail = if tail_len > 0 {
        &raw[raw.len() - tail_len..]
    } else {
        &[][..]
    };
    while !tail.is_empty() && std::str::from_utf8(tail).is_err() {
        tail = &tail[1..];
    }
    let tail_text = std::str::from_utf8(tail).unwrap_or("");
    let marker_text = String::from_utf8_lossy(marker_bytes);
    format!("{head_text}{marker_text}{tail_text}")
}

pub fn peer_block(
    turns: &[CouncilTurn],
    label_map: &BTreeMap<String, String>,
    anonymous: bool,
    exclude: Option<&str>,
) -> String {
    let mut chunks = Vec::new();
    for turn in turns {
        if exclude == Some(turn.validator.as_str()) {
            continue;
        }
        if turn.text.trim().is_empty() {
            continue;
        }
        let label = label_for(&turn.validator, label_map, anonymous);
        let bounded = bounded_text(&turn.text, MAX_TURN_BYTES);
        chunks.push(format!("[{label}]\n{}\n", bounded.trim_end()));
    }
    if chunks.is_empty() {
        return "(no peer outputs available)".to_string();
    }
    let joined = chunks.join("\n");
    bounded_text(joined.trim_end(), MAX_PEER_BLOCK_BYTES)
}

pub fn build_position_prompt(
    template: &TemplateSpec,
    scenario: &str,
) -> Result<String, DeliberationError> {
    Ok(format!(
        "{}\n\n{}\nRequired sections in your final ADR / PRD / RFC are listed below; your initial position can be looser, but cover them at least in outline:\n{}\n\nBe opinionated. Cite assumptions explicitly. Mark anything you are uncertain about with `Inference:` so the council can challenge it.",
        template.position_intro,
        wrap_scenario(scenario)?,
        template.required_section_block()
    ))
}

pub fn build_critique_prompt(
    template: &TemplateSpec,
    scenario: &str,
    peer_block_text: &str,
) -> Result<String, DeliberationError> {
    Ok(format!(
        "{}\n\n{}\n<<< Peer positions (anonymized) >>>\n{}\n<<< End peer positions >>>\n\nCritique each peer position by its label. Be specific: quote the claim and explain why it is weak, missing, or wrong. Do not speculate about which model wrote which response — labels are stripped on purpose.",
        template.critique_intro,
        wrap_scenario(scenario)?,
        peer_block_text
    ))
}

pub fn build_revision_prompt(
    template: &TemplateSpec,
    scenario: &str,
    own_position: &str,
    peer_critiques: &str,
) -> Result<String, DeliberationError> {
    Ok(format!(
        "{}\n\n{}\n<<< Your prior position >>>\n{}\n<<< End your prior position >>>\n\n<<< Critiques you received (anonymized) >>>\n{}\n<<< End critiques >>>\n\nProduce a revised version of your position. Where you agree, incorporate the critique. Where you disagree, defend your stance with one explicit sentence.",
        template.revision_intro,
        wrap_scenario(scenario)?,
        bounded_text(own_position, MAX_TURN_BYTES).trim_end(),
        bounded_text(peer_critiques, MAX_PEER_BLOCK_BYTES)
    ))
}

pub fn build_synthesis_prompt(
    template: &TemplateSpec,
    scenario: &str,
    revisions_block: &str,
) -> Result<String, DeliberationError> {
    Ok(format!(
        "{}\n\n{}\n<<< Council revised positions (anonymized) >>>\n{}\n<<< End council revised positions >>>\n\nRequired section headings (use them verbatim, in this order):\n{}\n\nRubric you will be scored against:\n{}\n\nProduce the final document. Do not invent positions that no council member raised; if you must extrapolate, prefix with `Inference:`.",
        template.synthesis_intro,
        wrap_scenario(scenario)?,
        bounded_text(revisions_block, MAX_PEER_BLOCK_BYTES),
        template.required_section_block(),
        template.rubric_block()
    ))
}

pub fn build_rubric_prompt(template: &TemplateSpec, synthesis_text: &str) -> String {
    format!(
        "{}\n\n<<< Synthesis to score >>>\n{}\n<<< End synthesis >>>\n\nRubric:\n{}\n\nReply format (exact):\nVERDICT: PASS|NEEDS_FIX\nSEVERITY: low|medium|high\n- objection 1\n- objection 2\nUse no other prose.",
        template.rubric_intro,
        bounded_text(synthesis_text, MAX_TURN_BYTES).trim_end(),
        template.rubric_block()
    )
}

pub fn parse_rubric_response(text: &str) -> (String, String, Vec<String>) {
    static VERDICT_RE: OnceLock<Regex> = OnceLock::new();
    static SEVERITY_RE: OnceLock<Regex> = OnceLock::new();
    static OBJECTION_RE: OnceLock<Regex> = OnceLock::new();

    let verdict_re = VERDICT_RE
        .get_or_init(|| Regex::new(r"(?im)^\s*VERDICT\s*:\s*(?P<v>PASS|NEEDS_FIX)\b").unwrap());
    let severity_re = SEVERITY_RE.get_or_init(|| {
        Regex::new(r"(?im)^\s*SEVERITY\s*:\s*(?P<s>low|medium|high|critical|blocker)\b").unwrap()
    });
    let objection_re = OBJECTION_RE.get_or_init(|| Regex::new(r"(?m)^\s*-\s+(?P<o>.+)$").unwrap());

    let redacted = redact_text(text);
    let status = match verdict_re.captures(&redacted) {
        Some(c) => {
            let v = c.name("v").unwrap().as_str().to_ascii_uppercase();
            if v == "NEEDS_FIX" || v == "NEEDS-FIX" {
                "NEEDS_FIX".to_string()
            } else {
                v
            }
        }
        None => "NEEDS_FIX".to_string(),
    };

    let severity = match severity_re.captures(&redacted) {
        Some(c) => {
            let s = c.name("s").unwrap().as_str().to_ascii_lowercase();
            if s == "critical" || s == "blocker" {
                "high".to_string()
            } else {
                s
            }
        }
        None => {
            if status == "NEEDS_FIX" {
                "medium".to_string()
            } else {
                "low".to_string()
            }
        }
    };

    let objections: Vec<String> = objection_re
        .captures_iter(&redacted)
        .filter_map(|c| {
            let s = c.name("o")?.as_str().trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
        .collect();

    (status, severity, objections)
}

fn require_contributors(
    turns: &[CouncilTurn],
    expected: usize,
    stage: &str,
) -> Result<(), DeliberationError> {
    let required = expected / 2 + 1;
    let contributors = turns
        .iter()
        .filter(|t| !t.text.trim().is_empty() && !t.metadata.contains_key("error"))
        .count();
    if contributors < required {
        return Err(DeliberationError::Inconclusive(format!(
            "inconclusive: {stage} contributor quorum {contributors}/{required} not met from {expected} validators"
        )));
    }
    Ok(())
}

fn format_stage_error(err: &str) -> String {
    let prefixed = if err.starts_with("RuntimeError: ")
        || err.starts_with("ValueError: ")
        || err.starts_with("KeyError: ")
    {
        err.to_string()
    } else {
        format!("RuntimeError: {err}")
    };
    prefixed.chars().take(300).collect()
}

fn generate_stage<F, P>(
    validators: &[String],
    prompt_for: P,
    generate: &F,
    timeout: u64,
    parallel: bool,
    max_workers: usize,
) -> BTreeMap<String, (String, String)>
where
    F: Fn(&str, &str, u64) -> Result<String, String> + Sync,
    P: Fn(&str) -> String + Sync,
{
    let invoke = |name: &str| -> (String, String) {
        let prompt = prompt_for(name);
        match generate(name, &prompt, timeout) {
            Ok(raw) => {
                let cleaned = redact_text(&raw).trim().to_string();
                (bounded_text(&cleaned, MAX_TURN_BYTES), String::new())
            }
            Err(e) => (String::new(), format_stage_error(&e)),
        }
    };

    if !parallel || validators.len() <= 1 {
        let mut out = BTreeMap::new();
        for name in validators {
            out.insert(name.clone(), invoke(name));
        }
        return out;
    }

    let workers = validators.len().min(max_workers.max(1));
    let queue = Arc::new(Mutex::new(
        validators.iter().cloned().collect::<std::collections::VecDeque<String>>(),
    ));
    let results: Arc<Mutex<BTreeMap<String, (String, String)>>> =
        Arc::new(Mutex::new(BTreeMap::new()));

    std::thread::scope(|s| {
        for _ in 0..workers {
            let q = Arc::clone(&queue);
            let r = Arc::clone(&results);
            s.spawn(move || loop {
                let next = q.lock().unwrap().pop_front();
                let Some(name) = next else { break };
                let res = invoke(&name);
                r.lock().unwrap().insert(name, res);
            });
        }
    });

    let mut final_map = BTreeMap::new();
    let guard = results.lock().unwrap();
    for name in validators {
        let val = guard
            .get(name)
            .cloned()
            .unwrap_or_else(|| (String::new(), "not scheduled".to_string()));
        final_map.insert(name.clone(), val);
    }
    final_map
}

pub struct DeliberationOptions<'a> {
    pub template: TemplateSpec,
    pub scenario: &'a str,
    pub validators: &'a [String],
    pub primary: Option<&'a str>,
    pub depth: &'a str,
    pub timeout: u64,
    pub anonymous: bool,
    pub output_dir: Option<&'a Path>,
    pub parallel: bool,
    pub max_workers: usize,
}

pub fn run_deliberation<F>(
    opts: DeliberationOptions<'_>,
    generate: F,
) -> Result<DeliberationRun, DeliberationError>
where
    F: Fn(&str, &str, u64) -> Result<String, String> + Sync,
{
    if !DEPTHS.contains(&opts.depth) {
        return Err(DeliberationError::Value(format!(
            "depth must be one of ('quick', 'standard', 'deep'), got {:?}",
            opts.depth
        )));
    }
    if opts.validators.is_empty() {
        return Err(DeliberationError::Value(
            "run_deliberation requires at least one validator".to_string(),
        ));
    }

    let started_at = utc_iso_seconds();
    let primary_name = opts
        .primary
        .filter(|s| !s.is_empty())
        .unwrap_or(&opts.validators[0])
        .to_string();
    if !opts.validators.iter().any(|v| v == &primary_name) {
        return Err(DeliberationError::Value(format!(
            "primary {primary_name:?} is not in validators {:?}",
            opts.validators
        )));
    }

    let label_map = build_label_map(opts.validators);
    let mut turns: Vec<CouncilTurn> = Vec::new();

    // --- Phase 2: independent positions ---
    let position_prompt = build_position_prompt(&opts.template, opts.scenario)?;
    let position_results = generate_stage(
        opts.validators,
        |_name| position_prompt.clone(),
        &generate,
        opts.timeout,
        opts.parallel,
        opts.max_workers,
    );
    let mut positions = Vec::new();
    for name in opts.validators {
        let (text, error) = position_results.get(name).cloned().unwrap_or_default();
        let mut metadata = BTreeMap::new();
        if !error.is_empty() {
            metadata.insert("error".to_string(), error);
        }
        positions.push(CouncilTurn {
            stage: "position".to_string(),
            validator: name.clone(),
            label: label_map.get(name).cloned(),
            text,
            metadata,
        });
    }
    turns.extend(positions.clone());
    require_contributors(&positions, opts.validators.len(), "position")?;

    let mut revisions: Vec<CouncilTurn> = positions.clone();

    if opts.depth == "standard" || opts.depth == "deep" {
        // --- Phase 3: anonymized critique ---
        let mut critique_prompts = BTreeMap::new();
        for name in opts.validators {
            let pb = peer_block(&positions, &label_map, opts.anonymous, Some(name));
            critique_prompts.insert(
                name.clone(),
                build_critique_prompt(&opts.template, opts.scenario, &pb)?,
            );
        }
        let critique_results = generate_stage(
            opts.validators,
            |name| critique_prompts.get(name).cloned().unwrap_or_default(),
            &generate,
            opts.timeout,
            opts.parallel,
            opts.max_workers,
        );
        let mut critiques = Vec::new();
        for name in opts.validators {
            let (text, error) = critique_results.get(name).cloned().unwrap_or_default();
            let mut metadata = BTreeMap::new();
            if !error.is_empty() {
                metadata.insert("error".to_string(), error);
            }
            critiques.push(CouncilTurn {
                stage: "critique".to_string(),
                validator: name.clone(),
                label: label_map.get(name).cloned(),
                text,
                metadata,
            });
        }
        turns.extend(critiques.clone());
        require_contributors(&critiques, opts.validators.len(), "critique")?;

        // --- Phase 4: revisions ---
        let mut revision_prompts = BTreeMap::new();
        for name in opts.validators {
            let own_position = positions
                .iter()
                .find(|p| &p.validator == name)
                .map(|p| p.text.as_str())
                .unwrap_or("");
            let peer_critiques: Vec<CouncilTurn> = critiques
                .iter()
                .filter(|c| &c.validator != name)
                .cloned()
                .collect();
            let received = peer_block(&peer_critiques, &label_map, opts.anonymous, None);
            revision_prompts.insert(
                name.clone(),
                build_revision_prompt(&opts.template, opts.scenario, own_position, &received)?,
            );
        }
        let revision_results = generate_stage(
            opts.validators,
            |name| revision_prompts.get(name).cloned().unwrap_or_default(),
            &generate,
            opts.timeout,
            opts.parallel,
            opts.max_workers,
        );
        revisions = Vec::new();
        for name in opts.validators {
            let (text, error) = revision_results.get(name).cloned().unwrap_or_default();
            let mut metadata = BTreeMap::new();
            if !error.is_empty() {
                metadata.insert("error".to_string(), error);
            }
            revisions.push(CouncilTurn {
                stage: "revision".to_string(),
                validator: name.clone(),
                label: label_map.get(name).cloned(),
                text,
                metadata,
            });
        }
        turns.extend(revisions.clone());
        require_contributors(&revisions, opts.validators.len(), "revision")?;
    }

    // --- Phase 5: synthesis (primary only) ---
    let revisions_block = peer_block(&revisions, &label_map, opts.anonymous, None);
    let synthesis_prompt = build_synthesis_prompt(&opts.template, opts.scenario, &revisions_block)?;
    let raw_synth = generate(&primary_name, &synthesis_prompt, opts.timeout)
        .map_err(DeliberationError::Runtime)?;
    let synthesis_text = bounded_text(redact_text(&raw_synth).trim(), MAX_TURN_BYTES);
    turns.push(CouncilTurn {
        stage: "synthesis".to_string(),
        validator: primary_name.clone(),
        label: label_map.get(&primary_name).cloned(),
        text: synthesis_text.clone(),
        metadata: BTreeMap::new(),
    });

    // --- Phase 6: rubric review ---
    let rubric_prompt = build_rubric_prompt(&opts.template, &synthesis_text);
    let rubric_stage = generate_stage(
        opts.validators,
        |_name| rubric_prompt.clone(),
        &generate,
        opts.timeout,
        opts.parallel,
        opts.max_workers,
    );
    let rubric_turns: Vec<CouncilTurn> = opts
        .validators
        .iter()
        .map(|name| {
            let (text, error) = rubric_stage.get(name).cloned().unwrap_or_default();
            let mut metadata = BTreeMap::new();
            if !error.is_empty() {
                metadata.insert("error".to_string(), error);
            }
            CouncilTurn {
                stage: "rubric".to_string(),
                validator: name.clone(),
                label: label_map.get(name).cloned(),
                text,
                metadata,
            }
        })
        .collect();
    require_contributors(&rubric_turns, opts.validators.len(), "rubric")?;

    let mut rubric_results = Vec::new();
    for name in opts.validators {
        let (text, error) = rubric_stage.get(name).cloned().unwrap_or_default();
        let (status, severity, objections) = parse_rubric_response(&text);
        rubric_results.push(RubricVerdict {
            validator: name.clone(),
            label: label_map.get(name).cloned(),
            status: status.clone(),
            severity: severity.clone(),
            text: text.clone(),
            objections,
        });
        let mut metadata = BTreeMap::new();
        metadata.insert("status".to_string(), status);
        metadata.insert("severity".to_string(), severity);
        if !error.is_empty() {
            metadata.insert("error".to_string(), error);
        }
        turns.push(CouncilTurn {
            stage: "rubric".to_string(),
            validator: name.clone(),
            label: label_map.get(name).cloned(),
            text,
            metadata,
        });
    }

    // --- Phase 7: minority report + decision log ---
    let minority_report = build_minority_report(&rubric_results, opts.anonymous, opts.depth);
    let decision_log = build_decision_log(
        &opts.template,
        opts.scenario,
        opts.validators,
        &primary_name,
        opts.depth,
        opts.anonymous,
        &rubric_results,
        &started_at,
    );
    let finished_at = utc_iso_seconds();

    let mut run = DeliberationRun {
        template: opts.template,
        scenario: opts.scenario.to_string(),
        validators: opts.validators.to_vec(),
        primary: primary_name,
        depth: opts.depth.to_string(),
        anonymous: opts.anonymous,
        turns,
        synthesis: synthesis_text,
        rubric: rubric_results,
        minority_report,
        decision_log,
        output_dir: opts.output_dir.map(Path::to_path_buf),
        started_at,
        finished_at,
    };

    if let Some(dir) = opts.output_dir {
        write_run(&run, dir).map_err(|e| DeliberationError::Runtime(e.to_string()))?;
        run.output_dir = Some(dir.to_path_buf());
    }

    Ok(run)
}

pub fn build_minority_report(
    rubric: &[RubricVerdict],
    anonymous: bool,
    depth: &str,
) -> String {
    let objectors: Vec<&RubricVerdict> =
        rubric.iter().filter(|r| r.status == "NEEDS_FIX").collect();
    if objectors.is_empty() {
        return "# Minority Report\n\nNo council member dissented; the synthesis passed the rubric unanimously.\n".to_string();
    }

    let keep: Vec<&RubricVerdict> = if depth == "deep" {
        objectors.clone()
    } else {
        let high: Vec<&RubricVerdict> = objectors
            .iter()
            .copied()
            .filter(|r| MINORITY_HIGH_SEVERITY.contains(&r.severity.as_str()))
            .collect();
        if high.is_empty() {
            vec![objectors[0]]
        } else {
            high
        }
    };

    let mut lines = vec![
        "# Minority Report".to_string(),
        String::new(),
        format!(
            "Council reported {} dissenting verdict(s); {} preserved here.",
            objectors.len(),
            keep.len()
        ),
        String::new(),
    ];

    for entry in keep {
        let who = if anonymous {
            entry.label.as_deref().unwrap_or(&entry.validator)
        } else {
            &entry.validator
        };
        lines.push(format!("## {who} — {}", entry.severity.to_ascii_uppercase()));
        if !entry.objections.is_empty() {
            for obj in &entry.objections {
                lines.push(format!("- {}", redact_text(obj)));
            }
        } else {
            lines.push("(no specific objections supplied)".to_string());
        }
        lines.push(String::new());
    }
    format!("{}\n", lines.join("\n").trim_end())
}

#[allow(clippy::too_many_arguments)]
pub fn build_decision_log(
    template: &TemplateSpec,
    scenario: &str,
    validators: &[String],
    primary: &str,
    depth: &str,
    anonymous: bool,
    rubric: &[RubricVerdict],
    started_at: &str,
) -> String {
    let pass_count = rubric.iter().filter(|r| r.status == "PASS").count();
    let fix_count = rubric.iter().filter(|r| r.status == "NEEDS_FIX").count();
    let primary_display = if anonymous { "(anonymous)" } else { primary };
    let anon_display = if anonymous { "True" } else { "False" };
    let scenario_len = scenario.chars().count();
    format!(
        "# Decision Log\n\n- Template: {} ({})\n- Started: {started_at}\n- Council size: {}\n- Primary: {primary_display}\n- Depth: {depth}\n- Anonymous critique: {anon_display}\n- Rubric verdicts: {pass_count} PASS, {fix_count} NEEDS_FIX\n- Scenario length: {scenario_len} characters\n",
        template.name,
        template.title,
        validators.len(),
    )
}

pub fn write_run(run: &DeliberationRun, output_dir: &Path) -> std::io::Result<PathBuf> {
    fs::create_dir_all(output_dir)?;
    fs::write(output_dir.join("scenario.md"), redact_text(&run.scenario))?;

    let turns_dir = output_dir.join("turns");
    let pos_dir = turns_dir.join("01-positions");
    let crit_dir = turns_dir.join("02-critiques");
    let rev_dir = turns_dir.join("03-revisions");
    for d in [&pos_dir, &crit_dir, &rev_dir] {
        fs::create_dir_all(d)?;
    }

    let mut label_to_slug: BTreeMap<String, String> = BTreeMap::new();
    let mut slug_for = |label_or_name: &str| -> String {
        if let Some(existing) = label_to_slug.get(label_or_name) {
            return existing.clone();
        }
        let mut cleaned = String::new();
        let mut prev_dash = false;
        for ch in label_or_name.chars() {
            if ch.is_ascii_alphanumeric() {
                cleaned.push(ch);
                prev_dash = false;
            } else if !prev_dash {
                cleaned.push('-');
                prev_dash = true;
            }
        }
        let cleaned = cleaned.trim_matches('-').to_string();
        let base = if cleaned.is_empty() {
            format!("validator-{}", label_to_slug.len() + 1)
        } else {
            cleaned
        };
        let mut suffix = String::new();
        let mut n = 1usize;
        while label_to_slug
            .values()
            .any(|v| v == &format!("{base}{suffix}"))
        {
            n += 1;
            suffix = format!("-{n}");
        }
        let full = format!("{base}{suffix}");
        label_to_slug.insert(label_or_name.to_string(), full.clone());
        full
    };

    for turn in &run.turns {
        let stage_dir = match turn.stage.as_str() {
            "position" => &pos_dir,
            "critique" => &crit_dir,
            "revision" => &rev_dir,
            _ => continue,
        };
        let key = if run.anonymous {
            turn.label.as_deref().unwrap_or(&turn.validator)
        } else {
            &turn.validator
        };
        let slug = slug_for(key);
        let target = stage_dir.join(format!("{slug}.md"));
        fs::write(target, format!("{}\n", redact_text(&turn.text)))?;
    }

    let final_dir = output_dir.join("final");
    fs::create_dir_all(&final_dir)?;
    fs::write(
        final_dir.join("report.md"),
        format!("{}\n", redact_text(&run.synthesis)),
    )?;
    fs::write(final_dir.join("minority-report.md"), &run.minority_report)?;
    fs::write(final_dir.join("decision-log.md"), &run.decision_log)?;

    let mut trace_lines = String::new();
    let opts = Opts {
        sort_keys: true,
        ..Opts::default()
    };
    for turn in &run.turns {
        let mut meta_map = serde_json::Map::new();
        for (k, v) in &turn.metadata {
            meta_map.insert(k.clone(), Value::String(redact_text(v)));
        }
        let validator_str = if run.anonymous {
            "(anonymous)"
        } else {
            &turn.validator
        };
        let payload = json!({
            "stage": turn.stage,
            "validator": validator_str,
            "label": turn.label,
            "text": redact_text(&turn.text),
            "metadata": Value::Object(meta_map),
        });
        trace_lines.push_str(&pyjson::dumps(&payload, &opts));
        trace_lines.push('\n');
    }
    fs::write(output_dir.join("trace.jsonl"), trace_lines)?;

    Ok(output_dir.to_path_buf())
}

pub fn default_output_dir(template: &TemplateSpec, root: Option<&Path>) -> PathBuf {
    let base = match root {
        Some(r) => r.to_path_buf(),
        None => std::env::var("LOPE_RUN_OUTPUT_DIR")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join("lope-runs")),
    };
    let stamp = utc_stamp();
    base.join(format!("{stamp}-{}", template.name))
}

fn utc_parts() -> (i64, u32, u32, u32, u32, u32) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400) as u32;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    let hour = sod / 3600;
    let min = (sod % 3600) / 60;
    let sec = sod % 60;
    (year, m, d, hour, min, sec)
}

fn utc_iso_seconds() -> String {
    let (year, m, d, hour, min, sec) = utc_parts();
    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{min:02}:{sec:02}")
}

fn utc_stamp() -> String {
    let (year, m, d, hour, min, sec) = utc_parts();
    format!("{year:04}{m:02}{d:02}-{hour:02}{min:02}{sec:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn stub_gen(name: &str, prompt: &str, _timeout: u64) -> Result<String, String> {
        let lower = prompt.to_ascii_lowercase();
        if lower.contains("critique") && prompt.contains("Peer positions") {
            return Ok(format!("Critique by {name}: response A is missing X."));
        }
        if lower.contains("revising") || lower.contains("revised version") {
            return Ok(format!("Revision by {name}: addressed peer feedback."));
        }
        if lower.contains("synthesizing") || prompt.contains("Required section headings") {
            return Ok(
                "## Context\nbody\n## Decision\nadopt approach Y\n## Consequences\nupside; downside\n## Alternatives Considered\noption A; option B"
                    .to_string(),
            );
        }
        if lower.contains("score") && lower.contains("rubric") {
            return Ok("VERDICT: PASS\nSEVERITY: low\n- minor wording fix".to_string());
        }
        Ok(format!("Position by {name}: I propose Y because Z."))
    }

    #[test]
    fn list_and_get_templates_cover_all_six_kinds() {
        let names = list_templates();
        assert_eq!(
            names,
            vec![
                "adr",
                "build-vs-buy",
                "incident-review",
                "migration-plan",
                "prd",
                "rfc"
            ]
        );
        let adr = get_template("ADR").unwrap();
        assert_eq!(adr.name, "adr");
        assert_eq!(adr.title, "Architecture Decision Record");
        assert!(adr.sections.contains(&"Decision"));
        assert!(get_template("not-a-template").is_err());
    }

    #[test]
    fn anon_labels_cover_single_and_double_letters() {
        assert_eq!(anon_label(0), "Response A");
        assert_eq!(anon_label(25), "Response Z");
        assert_eq!(anon_label(26), "Response AA");
        assert_eq!(anon_label(27), "Response AB");
    }

    #[test]
    fn parse_rubric_handles_pass_needs_fix_and_redaction() {
        let (status, sev, obj) = parse_rubric_response(
            "VERDICT: PASS\nSEVERITY: low\n- minor typo\n- align section header",
        );
        assert_eq!(status, "PASS");
        assert_eq!(sev, "low");
        assert_eq!(obj, vec!["minor typo", "align section header"]);

        let (status2, sev2, _) = parse_rubric_response("just some prose, no verdict");
        assert_eq!(status2, "NEEDS_FIX");
        assert_eq!(sev2, "medium");

        let (_, sev3, _) =
            parse_rubric_response("VERDICT: NEEDS_FIX\nSEVERITY: critical\n- blocker issue");
        assert_eq!(sev3, "high");

        let secret = "Bearer abcdefghijklmnopqrstuvwxyz123456";
        let (_, _, objs) =
            parse_rubric_response(&format!("VERDICT: NEEDS_FIX\nSEVERITY: high\n- leaks {secret}"));
        assert!(objs.iter().all(|o| !o.contains("abcdefghijklmnop")));
    }

    #[test]
    fn run_deliberation_quick_and_standard_stages() {
        let validators = vec![
            "claude".to_string(),
            "gemini".to_string(),
            "codex".to_string(),
        ];
        let quick = run_deliberation(
            DeliberationOptions {
                template: get_template("adr").unwrap(),
                scenario: "should we adopt JWT?",
                validators: &validators[..2],
                primary: Some("claude"),
                depth: "quick",
                timeout: 60,
                anonymous: true,
                output_dir: None,
                parallel: false,
                max_workers: 5,
            },
            stub_gen,
        )
        .unwrap();
        let quick_stages: Vec<&str> = quick.turns.iter().map(|t| t.stage.as_str()).collect();
        assert!(!quick_stages.contains(&"critique"));
        assert!(!quick_stages.contains(&"revision"));
        assert!(quick_stages.contains(&"position"));
        assert!(quick_stages.contains(&"synthesis"));
        assert!(quick_stages.contains(&"rubric"));

        let standard = run_deliberation(
            DeliberationOptions {
                template: get_template("adr").unwrap(),
                scenario: "adopt JWT?",
                validators: &validators,
                primary: Some("claude"),
                depth: "standard",
                timeout: 60,
                anonymous: true,
                output_dir: None,
                parallel: true,
                max_workers: 5,
            },
            stub_gen,
        )
        .unwrap();
        let count_stage = |s: &str| standard.turns.iter().filter(|t| t.stage == s).count();
        assert_eq!(count_stage("position"), 3);
        assert_eq!(count_stage("critique"), 3);
        assert_eq!(count_stage("revision"), 3);
        assert_eq!(count_stage("synthesis"), 1);
        assert_eq!(count_stage("rubric"), 3);
    }

    #[test]
    fn quorum_failure_and_minority_report_and_write_run() {
        let validators = vec![
            "claude".to_string(),
            "gemini".to_string(),
            "codex".to_string(),
        ];
        let calls = AtomicUsize::new(0);
        let err = run_deliberation(
            DeliberationOptions {
                template: get_template("adr").unwrap(),
                scenario: "decision",
                validators: &validators,
                primary: Some("claude"),
                depth: "standard",
                timeout: 60,
                anonymous: true,
                output_dir: None,
                parallel: false,
                max_workers: 5,
            },
            |name, _prompt, _timeout| {
                calls.fetch_add(1, Ordering::SeqCst);
                if name != "claude" {
                    Err("timeout".to_string())
                } else {
                    Ok("substantive position".to_string())
                }
            },
        )
        .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(matches!(err, DeliberationError::Inconclusive(ref m) if m.contains("position contributor quorum 1/2")));

        let tmp = tempfile::tempdir().unwrap();
        let out_dir = tmp.path().join("lope-runs").join("20260427-adr");
        let run = run_deliberation(
            DeliberationOptions {
                template: get_template("adr").unwrap(),
                scenario: "adopt JWT?",
                validators: &validators,
                primary: Some("claude"),
                depth: "standard",
                timeout: 60,
                anonymous: true,
                output_dir: Some(&out_dir),
                parallel: false,
                max_workers: 5,
            },
            |name, prompt, _timeout| {
                if prompt.contains("Required section headings") {
                    return Ok("## Context\nC\n## Decision\nD\n## Consequences\nQ\n## Alternatives Considered\nA".to_string());
                }
                if prompt.contains("VERDICT") {
                    if name == "gemini" {
                        return Ok("VERDICT: NEEDS_FIX\nSEVERITY: high\n- decision rationale not stated\n- alternatives missing".to_string());
                    }
                    return Ok("VERDICT: PASS\nSEVERITY: low\n- nothing major".to_string());
                }
                Ok(format!("draft from {name}"))
            },
        )
        .unwrap();

        assert!(run.minority_report.contains("decision rationale not stated"));
        assert!(!run.minority_report.contains("gemini"));
        assert!(run.minority_report.contains("Response B"));
        assert!(out_dir.join("scenario.md").exists());
        assert!(out_dir.join("trace.jsonl").exists());
        assert!(out_dir.join("final/report.md").exists());
        assert!(out_dir.join("final/minority-report.md").exists());
        assert!(out_dir.join("final/decision-log.md").exists());
        assert_eq!(
            fs::read_dir(out_dir.join("turns/01-positions"))
                .unwrap()
                .count(),
            3
        );
    }
}
