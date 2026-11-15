//! Multi-round sprint negotiation loop (`negotiator.py`, Sprint S7/S8B, `FR32`).

use crate::caveman::get_directive as caveman_directive;
use crate::models::{
    domain_config, EscalationRequired, Phase, PhaseVerdict, Proposal, Round, SprintDoc,
    VerdictStatus,
};
use crate::pool_validate::pool_validate;
use lope_providers::budget::Context;
use lope_providers::generic::Validator;
use lope_providers::invocation::invoke_generate;
use lope_providers::pool::Pool;
use lope_state::registry::py_repr_str;
use regex::Regex;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

pub const EVIDENCE_BRIEF_MAX_BYTES: usize = 48 * 1024;
pub const PROPOSAL_MAX_BYTES: usize = 48 * 1024;

#[derive(Debug, Clone)]
pub enum ConvergenceOutcome {
    Converged(SprintDoc),
    Escalated(EscalationRequired),
}

fn require_bounded_text(label: &str, text: &str, limit: usize) -> Result<String, String> {
    let size = text.len();
    if size > limit {
        return Err(format!(
            "{label} is {size} UTF-8 bytes; lossless negotiate limit is {limit}. \
             Provide a smaller evidence brief; Lope will not truncate evidence implicitly."
        ));
    }
    Ok(text.to_string())
}

pub fn negotiator_system_prompt(domain: &str) -> String {
    let dc = domain_config(domain);
    let domain_line = if domain != "engineering" {
        format!("\n  - **Domain:** {domain}")
    } else {
        String::new()
    };
    format!(
        concat!(
            "You are a {} drafting a sprint proposal.\n\n",
            "DO NOT USE ANY TOOLS. Do not call read, write, bash, edit, search, or any\n",
            "other tool. Write the sprint doc INLINE based ONLY on the GOAL and CONTEXT\n",
            "provided in this prompt. If the context mentions file paths, treat them as\n",
            "references the user already inspected — do not try to read them yourself.\n\n",
            "Output is markdown sprint doc only. No preamble. No postscript.\n\n",
            "Requirements:\n",
            "  - # title: \"SPRINT-<SLUG>\"{}\n",
            "  - ## Origin: user request\n",
            "  - ## Phases: one ### Phase N: <name> per phase\n",
            "  - Each phase: **Goal:** line, **Criteria:** bullets, **{}:** bullets, **{}:** bullets\n",
            "  - 3-5 phases. Over/under-decomposition = bugs.\n",
            "  - Revision rounds: address REQUIRED_FIXES line by line. Verify each claim\n",
            "    against the in-prompt context before accepting/rejecting (no file reads).\n\n",
            "Output ONLY the sprint doc.\n"
        ),
        dc.role, domain_line, dc.artifact_label, dc.check_label
    )
}

pub fn build_user_prompt(goal: &str, context: &str) -> String {
    let mut parts = vec![format!("## Goal\n\n{goal}\n")];
    if !context.is_empty() {
        parts.push(format!("\n## Context\n\n{context}\n"));
    }
    parts.push(
        "\n## Task\n\nDraft a sprint doc for this goal. Follow the format \
         in the system prompt. Think hard about scope — prefer fewer phases \
         and smaller changes when the payoff is the same."
            .to_string(),
    );
    parts.join("\n")
}

pub fn slug_from_goal(goal: &str) -> String {
    let lower: String = goal.trim().to_lowercase().chars().take(60).collect();
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in lower.chars() {
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
        "sprint".to_string()
    } else {
        trimmed
    }
}

pub fn build_negotiation_validator_prompt(
    goal: &str,
    proposal_text: &str,
    round_number: usize,
    _domain: &str,
    evidence_brief: &str,
) -> String {
    let caveman = caveman_directive();
    let brief = if evidence_brief.is_empty() {
        "(none provided)"
    } else {
        evidence_brief
    };
    format!(
        concat!(
            "{}\n\n",
            "Review sprint proposal. Verify claims against the in-prompt proposal/context BEFORE accepting.\n",
            "Push back on: scope creep, missing edge cases, unverified assumptions, over-complexity.\n",
            "No rubber-stamping.\n\n",
            "DO NOT USE TOOLS. Do not read files, run shell commands, browse, search, edit,\n",
            "or inspect the local repository. This is a paper-review of the prompt content\n",
            "only. If evidence is missing from the prompt, mark that as a REQUIRED_FIX; do\n",
            "not chase file paths and do not invent file:line citations.\n\n",
            "## Goal\n\n{}\n\n",
            "## Original evidence brief (preserved across every round)\n\n{}\n\n",
            "## Proposal (round {})\n\n{}\n\n",
            "## Your job\n\n",
            "1. Review the in-prompt proposal\n",
            "2. Flag any claim that lacks in-prompt evidence\n",
            "3. Question unnecessary scope\n",
            "4. Respond with VERDICT block:\n\n",
            "---VERDICT---\n",
            "status: PASS | NEEDS_FIX | FAIL\n",
            "confidence: 0.0-1.0\n",
            "rationale: 1-3 sentences, terse, no filler\n",
            "required_fixes:\n",
            "  - fix 1\n",
            "  - fix 2\n",
            "---END---\n\n",
            "PASS=ready. NEEDS_FIX=list specific fixes. FAIL=escalate. Conf<0.7 on PASS→NEEDS_FIX.\n"
        ),
        caveman, goal, brief, round_number, proposal_text
    )
}

pub fn build_validator_prompt(doc: &SprintDoc, phase: &Phase, domain: &str) -> String {
    let dc = domain_config(domain);
    let join_list = |items: &[String]| -> String {
        if items.is_empty() {
            "(none)".to_string()
        } else {
            items
                .iter()
                .map(|c| format!("- {c}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    format!(
        "You are an independent {} reviewing Phase {} of sprint \"{}\".\n\n\
Origin:\n{}\n\n\
### Phase {}: {}\n\
**Goal:** {}\n\
**Criteria:**\n{}\n\
**{}:**\n{}\n\
**{}:**\n{}\n\n\
Evaluate whether this phase is concrete, scoped, testable, and feasible.\n\
Reply with a `---VERDICT---` ... `---END---` block (PASS, NEEDS_FIX, or FAIL) with confidence, rationale, and required_fixes.",
        dc.role,
        phase.index,
        doc.title,
        doc.origin,
        phase.index,
        phase.name,
        phase.goal,
        join_list(&phase.criteria),
        dc.artifact_label,
        join_list(&phase.artifacts),
        dc.check_label,
        join_list(&phase.checks),
    )
}

fn lint_regexes() -> &'static (Regex, Regex, Regex, Regex, Regex) {
    static RE: OnceLock<(Regex, Regex, Regex, Regex, Regex)> = OnceLock::new();
    RE.get_or_init(|| {
        (
            Regex::new(r"```[\s\S]*?```").unwrap(),
            Regex::new(
                r"(?i)\bTBD\b|\bTODO\b|\bXXX\b|\bFIXME\b|<placeholder>|<your code here>|<your text here>|\[insert[^\]]*\]",
            )
            .unwrap(),
            Regex::new(r"(?m)^###\s+Phase\s+\d+.*$").unwrap(),
            Regex::new(r"(?m)^\s*\*\*Goal\*?\*?\s*:?\*?\*?\s*(.+)$").unwrap(),
            Regex::new(
                r"(?i)\*\*(?:artifacts|files|deliverables|checks|tests|success metrics|validation criteria)\*?\*?\s*:?\s*\*?\*?[ \t]*\n",
            )
            .unwrap(),
        )
    })
}

fn count_prose_ellipsis(prose: &str) -> usize {
    let bytes = prose.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b"..." {
            let prev_ok = i == 0
                || !prose[..i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c == '.' || c.is_alphanumeric() || c == '_');
            let next_ok = i + 3 == bytes.len()
                || !prose[i + 3..]
                    .chars()
                    .next()
                    .is_some_and(|c| c == '.' || c.is_alphanumeric() || c == '_');
            if prev_ok && next_ok {
                count += 1;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    count
}

fn has_nonempty_list(body: &str, candidate_labels: &[&str]) -> bool {
    let escaped: Vec<String> = candidate_labels
        .iter()
        .map(|l| regex::escape(l))
        .collect();
    let pat = format!(
        r"(?im)^\s*\*\*(?:{})\*?\*?\s*:?\s*\*?\*?[ \t]*\n",
        escaped.join("|")
    );
    let Ok(re) = Regex::new(&pat) else {
        return false;
    };
    for m in re.find_iter(body) {
        let tail = &body[m.end()..];
        let mut first_nonblank = "";
        for line in tail.split('\n') {
            let s = line.trim();
            if !s.is_empty() {
                first_nonblank = s;
                break;
            }
        }
        if first_nonblank.is_empty() {
            continue;
        }
        if first_nonblank.starts_with("- ") || first_nonblank.starts_with("• ") {
            return true;
        }
        if first_nonblank.starts_with("* ") && !first_nonblank.starts_with("**") {
            return true;
        }
    }
    false
}

pub fn lint_proposal_text(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return vec!["proposal is empty".to_string()];
    }
    let (fence_re, placeholder_re, phase_re, goal_re, _) = lint_regexes();
    let prose = fence_re.replace_all(text, "");
    let mut errors = Vec::new();

    for m in placeholder_re.find_iter(&prose) {
        errors.push(format!(
            "placeholder token {} — replace with concrete content",
            py_repr_str(m.as_str())
        ));
        if errors.len() >= 5 {
            break;
        }
    }

    let ellipsis_count = count_prose_ellipsis(&prose);
    if ellipsis_count > 0 {
        errors.push(format!(
            "{ellipsis_count} prose ellipsis token(s) '...' — write out the steps, do not trail off"
        ));
    }

    let matches: Vec<_> = phase_re.find_iter(text).collect();
    if matches.is_empty() {
        errors.push(
            "no `### Phase N:` blocks found — sprint doc must have 3-5 numbered phases".to_string(),
        );
        return errors;
    }

    for (i, m) in matches.iter().enumerate() {
        let start = m.end();
        let end = matches
            .get(i + 1)
            .map_or(text.len(), |next| next.start());
        let body = &text[start..end];
        let index = i + 1;

        match goal_re.captures(body) {
            None => errors.push(format!("phase {index}: missing **Goal:** line")),
            Some(cap) => {
                let goal_text = cap[1].trim().trim_end_matches('*').trim();
                if goal_text.split_whitespace().count() < 2 {
                    errors.push(format!(
                        "phase {index}: goal too short: {}",
                        py_repr_str(goal_text)
                    ));
                }
            }
        }
        if !has_nonempty_list(body, &["artifacts", "files", "deliverables"]) {
            errors.push(format!(
                "phase {index}: needs a non-empty **Artifacts/Files/Deliverables:** bullet list"
            ));
        }
        if !has_nonempty_list(
            body,
            &["checks", "tests", "success metrics", "validation criteria"],
        ) {
            errors.push(format!(
                "phase {index}: needs a non-empty **Checks/Tests/Success Metrics:** bullet list"
            ));
        }
        if errors.len() >= 10 {
            errors.push("(truncated at 10 errors; more phases unchecked)".to_string());
            break;
        }
    }

    errors
}

fn render_lint_feedback(errors: &[String]) -> String {
    let bullets = errors
        .iter()
        .map(|e| format!("  - {e}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "--- DRAFT LINT FAILED — FIX BEFORE VALIDATOR REVIEW ---\n\
Your previous draft failed the pre-validation lint with these issues:\n\
{bullets}\n\n\
Rewrite the draft. Replace every placeholder token with concrete text. \
Every phase must have (1) a **Goal:** line with at least a few words, \
(2) a non-empty **Artifacts/Files/Deliverables:** bullet list, and \
(3) a non-empty **Checks/Tests/Success Metrics:** bullet list. \
Output the full revised sprint doc, not a diff."
    )
}

fn render_refinement_suffix(verdict: &PhaseVerdict, prior_round: usize) -> String {
    let fix_block = if verdict.required_fixes.is_empty() {
        "  (none listed; improve rigor generally)".to_string()
    } else {
        verdict
            .required_fixes
            .iter()
            .map(|f| format!("  - {f}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let rationale = if verdict.rationale.trim().is_empty() {
        "(no rationale provided)"
    } else {
        verdict.rationale.as_str()
    };
    format!(
        "\n--- VALIDATOR FEEDBACK (round {prior_round}) ---\n\
status: {} (conf={:.2})\n\
rationale: {rationale}\n\
fixes:\n\
{fix_block}\n\n\
--- REVISE ---\n\
Address each fix using only the in-prompt proposal/context. DO NOT USE TOOLS.\n\
Do not read files, run shell commands, browse, search, or edit. If a fix needs\n\
code evidence that is not present in the prompt, add an explicit verification\n\
task to the sprint instead of inventing source citations.\n\
Output full revised sprint doc, not a diff.\n",
        verdict.status.as_str(),
        verdict.confidence,
    )
}

pub fn lint_proposal(doc: &SprintDoc) -> Vec<String> {
    let mut errors = Vec::new();
    if doc.phases.is_empty() {
        errors.push("proposal has zero phases".to_string());
        return errors;
    }
    let mut seen_indices = HashSet::new();
    for p in &doc.phases {
        if !seen_indices.insert(p.index) {
            errors.push(format!("duplicate phase index {}", p.index));
        }
        if p.name.trim().is_empty() {
            errors.push(format!("phase {} has empty name", p.index));
        }
        if p.goal.trim().is_empty() {
            errors.push(format!("phase {} ({}) missing **Goal:**", p.index, p.name));
        }
        if p.criteria.is_empty() {
            errors.push(format!("phase {} ({}) missing **Criteria:**", p.index, p.name));
        }
        if p.artifacts.is_empty() {
            errors.push(format!(
                "phase {} ({}) missing deliverables/files",
                p.index, p.name
            ));
        }
        if p.checks.is_empty() {
            errors.push(format!(
                "phase {} ({}) missing checks/tests",
                p.index, p.name
            ));
        }
    }
    errors
}

pub struct Negotiator {
    pub pool: Pool,
    pub max_rounds_per_phase: usize,
    pub timeout_seconds: f64,
    pub max_total_rounds: usize,
    pub domain: String,
    pub rounds: Vec<Round>,
    evidence_brief: String,
    healer: crate::healer::SelfHealer,
}

impl Negotiator {
    pub fn new(
        pool: Pool,
        max_rounds_per_phase: usize,
        timeout_seconds: f64,
        max_total_rounds: usize,
        domain: impl Into<String>,
    ) -> Self {
        Self {
            pool,
            max_rounds_per_phase: max_rounds_per_phase.max(1),
            timeout_seconds,
            max_total_rounds: max_total_rounds.max(1),
            domain: domain.into(),
            rounds: Vec::new(),
            evidence_brief: String::new(),
            healer: crate::healer::SelfHealer::new(),
        }
    }

    fn drafter_chain(&self) -> Result<Vec<Arc<dyn Validator>>, String> {
        let Some(primary) = self.pool.primary_validator() else {
            return Err("no validators configured in pool".to_string());
        };
        let mut chain = vec![primary.clone()];
        for v in &self.pool.validators {
            if !Arc::ptr_eq(v, &primary) {
                chain.push(v.clone());
            }
        }
        Ok(chain)
    }

    fn call_drafter(
        &mut self,
        system: &str,
        user: &str,
        ctx: Option<&Context>,
    ) -> Result<String, String> {
        let combined = format!("{system}\n\n{user}");
        let chain = self.drafter_chain()?;
        let mut errors = Vec::new();
        let mut last_err_msg = String::new();
        for (idx, drafter) in chain.iter().enumerate() {
            if idx > 0 {
                let reason = if last_err_msg.is_empty() {
                    String::new()
                } else {
                    format!(" ({last_err_msg})")
                };
                println!(
                    "[drafter fallback] {} failed{reason}; trying {}...",
                    chain[idx - 1].name(),
                    drafter.name()
                );
            }
            match invoke_generate(
                drafter.as_ref(),
                &combined,
                self.timeout_seconds,
                ctx,
                Some("negotiate_draft"),
            ) {
                Ok(out) => return Ok(out),
                Err(e) => {
                    let mut msg = e.to_string();
                    if lope_providers::verdict::is_flag_error(&msg) {
                        if let Some(healed) = crate::healer::try_self_heal_for_validator(
                            &mut self.healer,
                            drafter.as_ref(),
                            &self.pool.validators,
                            &msg,
                            ctx,
                        ) {
                            match invoke_generate(
                                healed.as_ref(),
                                &combined,
                                self.timeout_seconds,
                                ctx,
                                Some("negotiate_draft"),
                            ) {
                                Ok(out) => return Ok(out),
                                Err(e2) => msg = e2.to_string(),
                            }
                        }
                    }
                    let first_line = msg.lines().next().unwrap_or(&msg);
                    let short: String = first_line.chars().take(200).collect();
                    last_err_msg = short.clone();
                    errors.push(format!("{}: {short}", drafter.name()));
                }
            }
        }
        let summary = errors
            .iter()
            .map(|e| format!("    - {e}"))
            .collect::<Vec<_>>()
            .join("\n");
        Err(format!(
            "All {} drafters in the pool failed:\n{summary}",
            chain.len()
        ))
    }

    fn llm_and_lint(
        &mut self,
        system: &str,
        user: &str,
        round_number: usize,
        ctx: Option<&Context>,
    ) -> Result<String, String> {
        if std::env::var("LOPE_LINT")
            .map(|v| v.eq_ignore_ascii_case("off"))
            .unwrap_or(false)
        {
            return self.call_drafter(system, user, ctx);
        }
        let max_lint_retries = 2usize;
        let total_attempts = max_lint_retries + 1;
        let mut current_user = user.to_string();
        let mut last_text = String::new();
        for attempt in 1..=total_attempts {
            last_text = self.call_drafter(system, &current_user, ctx)?;
            let errors = lint_proposal_text(&last_text);
            if errors.is_empty() {
                return Ok(last_text);
            }
            let first3: Vec<String> = errors.iter().take(3).map(|e| py_repr_str(e)).collect();
            eprintln!(
                "lope.negotiator | [negotiator] round {round_number} lint-fix attempt {attempt}/{total_attempts} — {} issue(s): [{}]",
                errors.len(),
                first3.join(", ")
            );
            current_user = format!("{user}\n\n{}", render_lint_feedback(&errors));
        }
        eprintln!(
            "lope.negotiator | [negotiator] round {round_number} lint did not converge after {total_attempts} attempts; passing draft to validators anyway"
        );
        Ok(last_text)
    }

    pub fn propose(
        &mut self,
        goal: &str,
        context: &str,
        ctx: Option<&Context>,
    ) -> Result<Proposal, String> {
        self.evidence_brief =
            require_bounded_text("original evidence brief", context, EVIDENCE_BRIEF_MAX_BYTES)?;
        let system = negotiator_system_prompt(&self.domain);
        let user = build_user_prompt(goal, &self.evidence_brief);
        let text = self.llm_and_lint(&system, &user, 1, ctx)?;
        self.rounds.push(Round {
            number: 1,
            proposer: "drafter".to_string(),
            text: text.clone(),
            verdict: None,
        });
        Ok(Proposal {
            round_number: 1,
            goal: goal.to_string(),
            text,
            prior_feedback: String::new(),
        })
    }

    pub fn refine(
        &mut self,
        previous: &Proposal,
        verdict: &PhaseVerdict,
        ctx: Option<&Context>,
    ) -> Result<Proposal, String> {
        if verdict.status == VerdictStatus::Pass {
            return Ok(previous.clone());
        }
        let prior =
            require_bounded_text("previous proposal", &previous.text, PROPOSAL_MAX_BYTES)?;
        let next_round = previous.round_number + 1;
        let system = negotiator_system_prompt(&self.domain);
        let suffix = render_refinement_suffix(verdict, previous.round_number);
        let user_prompt = format!(
            "{}\n\n## Previous proposal (round {})\n\n{}{}",
            build_user_prompt(&previous.goal, &self.evidence_brief),
            previous.round_number,
            prior,
            suffix
        );
        let text = self.llm_and_lint(&system, &user_prompt, next_round, ctx)?;
        self.rounds.push(Round {
            number: next_round,
            proposer: "drafter".to_string(),
            text: text.clone(),
            verdict: None,
        });
        Ok(Proposal {
            round_number: next_round,
            goal: previous.goal.clone(),
            text,
            prior_feedback: verdict.rationale.clone(),
        })
    }

    pub fn converge(
        &mut self,
        goal: &str,
        context: &str,
        ctx: Option<&Context>,
        _on_event: Option<&mut dyn FnMut(&str)>,
    ) -> Result<ConvergenceOutcome, String> {
        let mut proposal = self.propose(goal, context, ctx)?;
        for round_idx in 1..=self.max_rounds_per_phase {
            let vprompt = build_negotiation_validator_prompt(
                goal,
                &proposal.text,
                proposal.round_number,
                &self.domain,
                &self.evidence_brief,
            );
            let res = pool_validate(&self.pool, &vprompt, self.timeout_seconds, ctx, "provider");
            let verdict = res.verdict.clone();
            let round_text = if res.raw_response.is_empty() {
                verdict.rationale.clone()
            } else {
                res.raw_response.clone()
            };
            self.rounds.push(Round {
                number: round_idx,
                proposer: "validator".to_string(),
                text: round_text,
                verdict: Some(verdict.clone()),
            });

            match verdict.status {
                VerdictStatus::Pass => {
                    let mut doc = SprintDoc::from_markdown(&proposal.text, None);
                    if doc.slug == "untitled" || doc.slug.is_empty() {
                        doc.slug = slug_from_goal(goal);
                    }
                    return Ok(ConvergenceOutcome::Converged(doc));
                }
                VerdictStatus::Fail => {
                    return Ok(ConvergenceOutcome::Escalated(EscalationRequired {
                        reason: "validator returned FAIL — architectural pushback".to_string(),
                        phase_index: 0,
                        phase_name: format!("negotiation-round-{round_idx}"),
                        rounds: self.rounds.clone(),
                        last_verdict: Some(verdict),
                    }));
                }
                VerdictStatus::InfraError | VerdictStatus::Inconclusive => {
                    let msg = if res.error.is_empty() {
                        verdict.rationale.as_str()
                    } else {
                        res.error.as_str()
                    };
                    return Ok(ConvergenceOutcome::Escalated(EscalationRequired {
                        reason: format!("validator infra error: {msg}"),
                        phase_index: 0,
                        phase_name: format!("negotiation-round-{round_idx}"),
                        rounds: self.rounds.clone(),
                        last_verdict: Some(verdict),
                    }));
                }
                VerdictStatus::NeedsFix => {
                    if round_idx >= self.max_rounds_per_phase {
                        return Ok(ConvergenceOutcome::Escalated(EscalationRequired {
                            reason: format!(
                                "{} NEEDS_FIX rounds exhausted without PASS",
                                self.max_rounds_per_phase
                            ),
                            phase_index: 0,
                            phase_name: format!("negotiation-round-{round_idx}"),
                            rounds: self.rounds.clone(),
                            last_verdict: Some(verdict),
                        }));
                    }
                    proposal = self.refine(&proposal, &verdict, ctx)?;
                }
            }
        }

        Ok(ConvergenceOutcome::Escalated(EscalationRequired {
            reason: "negotiation loop exited unexpectedly".to_string(),
            phase_index: 0,
            phase_name: "negotiation-exit".to_string(),
            rounds: self.rounds.clone(),
            last_verdict: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_proposal_catches_missing_sections() {
        let empty = SprintDoc::from_markdown("# Empty\n", None);
        assert!(!lint_proposal(&empty).is_empty());

        let valid = SprintDoc::from_markdown(
            "# SPRINT-TEST\n\n## Origin\nTest\n\n## Phases\n\n### Phase 1: setup\n\n**Goal:** Init\n\n**Criteria:**\n- Builds\n\n**Files:**\n- `src/lib.rs`\n\n**Tests:**\n- `cargo test`\n",
            None,
        );
        assert!(lint_proposal(&valid).is_empty());
    }

    #[test]
    fn lint_proposal_text_and_slug_match_python() {
        assert_eq!(slug_from_goal("Add a health endpoint!!!"), "add-a-health-endpoint");
        assert_eq!(slug_from_goal("   "), "sprint");
        assert_eq!(slug_from_goal(&"a".repeat(80)).len(), 60);

        let no_phases = "Just some text\n";
        assert_eq!(
            lint_proposal_text(no_phases),
            vec!["no `### Phase N:` blocks found — sprint doc must have 3-5 numbered phases"]
        );

        let valid = "# SPRINT-TEST\n\n## Origin\nTest\n\n## Phases\n\n### Phase 1: setup\n\n**Goal:** Initialize crate cleanly\n\n**Files:**\n- `src/lib.rs`\n\n**Tests:**\n- `cargo test`\n";
        assert!(lint_proposal_text(valid).is_empty());
    }
}
