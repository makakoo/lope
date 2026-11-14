//! Multi-round sprint negotiation loop (`negotiator.py`, Sprint S7, `FR32`).

use crate::models::{
    domain_config, slug_from_title, EscalationRequired, Phase, PhaseVerdict, Round, SprintDoc,
    VerdictStatus,
};
use crate::pool_validate::pool_validate;
use lope_providers::budget::Context;
use lope_providers::generic::Validator;
use lope_providers::invocation::invoke_generate;
use lope_providers::pool::Pool;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub enum ConvergenceOutcome {
    Converged(SprintDoc),
    Escalated(EscalationRequired),
}

pub fn negotiator_system_prompt(domain: &str) -> String {
    let dc = domain_config(domain);
    format!(
        "You are a disciplined {}. Your job is to produce a clear, testable, phased sprint plan.\n\n\
Output format (strict markdown, follow this exact structure):\n\n\
# SPRINT-<UPPERCASE-SLUG>\n\n\
## Origin\n<one-paragraph summary of the goal>\n\n\
## Phases\n\n\
### Phase 1: <short-name>\n\n\
**Goal:** <one sentence>\n\n\
**Criteria:**\n- <concrete verifiable criterion>\n\n\
**{}:**\n- `<path-or-deliverable>`\n\n\
**{}:**\n- `<command-or-verification>`\n",
        dc.role, dc.artifact_label, dc.check_label
    )
}

pub fn build_user_prompt(goal: &str, context: &str) -> String {
    if context.trim().is_empty() {
        format!("Goal:\n{goal}\n\nProduce the complete phased SPRINT.md document.")
    } else {
        format!(
            "Goal:\n{goal}\n\nContext:\n{context}\n\nProduce the complete phased SPRINT.md document."
        )
    }
}

pub fn slug_from_goal(goal: &str) -> String {
    slug_from_title(goal)
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
            errors.push(format!(
                "phase {} ({}) missing **Criteria:**",
                p.index, p.name
            ));
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

pub fn build_validator_prompt(doc: &SprintDoc, phase: &Phase, domain: &str) -> String {
    let dc = domain_config(domain);
    let criteria = if phase.criteria.is_empty() {
        "(none)".to_string()
    } else {
        phase
            .criteria
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let artifacts = if phase.artifacts.is_empty() {
        "(none)".to_string()
    } else {
        phase
            .artifacts
            .iter()
            .map(|a| format!("- {a}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let checks = if phase.checks.is_empty() {
        "(none)".to_string()
    } else {
        phase
            .checks
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "You are an independent {} reviewing Phase {} of sprint \"{}\".\n\n\
Origin:\n{}\n\n\
### Phase {}: {}\n\
**Goal:** {}\n\
**Criteria:**\n{criteria}\n\
**{}:**\n{artifacts}\n\
**{}:**\n{checks}\n\n\
Evaluate whether this phase is concrete, scoped, testable, and feasible.\n\
Reply with a `---VERDICT---` ... `---END---` block (PASS, NEEDS_FIX, or FAIL) with confidence, rationale, and required_fixes.",
        dc.role, phase.index, doc.title, doc.origin, phase.index, phase.name, phase.goal, dc.artifact_label, dc.check_label
    )
}

pub struct Negotiator {
    pub pool: Pool,
    pub max_rounds_per_phase: usize,
    pub timeout_seconds: f64,
    pub max_total_rounds: usize,
    pub domain: String,
    pub rounds: Vec<Round>,
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
        }
    }

    fn drafter(&self) -> Result<std::sync::Arc<dyn Validator>, String> {
        self.pool
            .primary_validator()
            .ok_or_else(|| "no validators configured in pool".to_string())
    }

    pub fn propose(
        &mut self,
        goal: &str,
        context: &str,
        ctx: Option<&Context>,
    ) -> Result<SprintDoc, String> {
        let system = negotiator_system_prompt(&self.domain);
        let user = build_user_prompt(goal, context);
        let prompt = format!("{system}\n\n{user}");
        let drafter = self.drafter()?;
        let raw = invoke_generate(
            drafter.as_ref(),
            &prompt,
            self.timeout_seconds,
            ctx,
            Some("negotiate_propose"),
        )
        .map_err(|e| format!("drafter failed: {e}"))?;

        self.rounds.push(Round {
            number: self.rounds.len() + 1,
            proposer: "drafter".to_string(),
            text: raw.clone(),
            verdict: None,
        });

        let mut doc = SprintDoc::from_markdown(&raw, None);
        if doc.slug.is_empty() {
            doc.slug = slug_from_goal(goal);
        }
        for _attempt in 1..=3 {
            let lint_errs = lint_proposal(&doc);
            if lint_errs.is_empty() {
                break;
            }
            let fix_prompt = format!(
                "{system}\n\nYour previous draft failed structural lint:\n- {}\n\nPrevious draft:\n{raw}\n\nFix all lint issues and output the complete corrected SPRINT.md.",
                lint_errs.join("\n- ")
            );
            if let Ok(fixed) = invoke_generate(
                drafter.as_ref(),
                &fix_prompt,
                self.timeout_seconds,
                ctx,
                Some("negotiate_draft"),
            ) {
                self.rounds.push(Round {
                    number: self.rounds.len() + 1,
                    proposer: "drafter".to_string(),
                    text: fixed.clone(),
                    verdict: None,
                });
                doc = SprintDoc::from_markdown(&fixed, None);
                if doc.slug.is_empty() {
                    doc.slug = slug_from_goal(goal);
                }
            } else {
                break;
            }
        }
        Ok(doc)
    }

    pub fn refine(
        &mut self,
        doc: &SprintDoc,
        phase: &Phase,
        verdict: &PhaseVerdict,
        ctx: Option<&Context>,
    ) -> Result<SprintDoc, String> {
        let system = negotiator_system_prompt(&self.domain);
        let fixes = if verdict.required_fixes.is_empty() {
            "(none specified)".to_string()
        } else {
            verdict
                .required_fixes
                .iter()
                .map(|f| format!("- {f}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let prompt = format!(
            "{system}\n\nRevise Phase {} ({}) in the sprint plan below to address the validator's feedback.\n\n\
Validator verdict: {}\nRationale: {}\nRequired fixes:\n{fixes}\n\n\
Current sprint document:\n{}\n\n\
Return the complete updated SPRINT.md.",
            phase.index,
            phase.name,
            verdict.status.as_str(),
            verdict.rationale,
            doc.to_markdown()
        );
        let drafter = self.drafter()?;
        let raw = invoke_generate(
            drafter.as_ref(),
            &prompt,
            self.timeout_seconds,
            ctx,
            Some("negotiate_refine"),
        )
        .map_err(|e| format!("drafter refine failed: {e}"))?;

        self.rounds.push(Round {
            number: self.rounds.len() + 1,
            proposer: "drafter".to_string(),
            text: raw.clone(),
            verdict: None,
        });

        let mut updated = SprintDoc::from_markdown(&raw, Some(&doc.slug));
        if updated.phases.is_empty() {
            updated = doc.clone();
        }
        Ok(updated)
    }

    pub fn converge(
        &mut self,
        goal: &str,
        context: &str,
        ctx: Option<&Context>,
        mut on_event: Option<&mut dyn FnMut(&str)>,
    ) -> Result<ConvergenceOutcome, String> {
        if let Some(cb) = on_event.as_deref_mut() {
            cb("Proposing initial sprint plan...");
        }
        let mut doc = self.propose(goal, context, ctx)?;
        if doc.phases.is_empty() {
            let draft_text = self
                .rounds
                .last()
                .map(|r| r.text.clone())
                .unwrap_or_default();
            doc.phases.push(Phase {
                index: 0,
                name: "negotiation-round-1".to_string(),
                goal: goal.to_string(),
                criteria: vec![draft_text],
                artifacts: Vec::new(),
                checks: Vec::new(),
                verdict: None,
            });
        }

        let mut total_rounds = 0usize;
        let phase_count = doc.phases.len();
        for idx in 0..phase_count {
            let mut phase_round = 0usize;
            loop {
                let phase = doc.phases[idx].clone();
                if total_rounds >= self.max_total_rounds {
                    return Ok(ConvergenceOutcome::Escalated(EscalationRequired {
                        reason: format!("max total rounds ({}) reached", self.max_total_rounds),
                        phase_index: phase.index,
                        phase_name: phase.name,
                        rounds: self.rounds.clone(),
                        last_verdict: None,
                    }));
                }
                phase_round += 1;
                total_rounds += 1;
                if let Some(cb) = on_event.as_deref_mut() {
                    cb(&format!(
                        "Validating Phase {} ({}) [round {phase_round}/{}]...",
                        phase.index, phase.name, self.max_rounds_per_phase
                    ));
                }

                let prompt = build_validator_prompt(&doc, &phase, &self.domain);
                let res = pool_validate(
                    &self.pool,
                    &prompt,
                    self.timeout_seconds,
                    ctx,
                    "negotiate_validate",
                );
                let verdict = res.verdict.clone();

                self.rounds.push(Round {
                    number: self.rounds.len() + 1,
                    proposer: "validator".to_string(),
                    text: res.raw_response.clone(),
                    verdict: Some(verdict.clone()),
                });

                match verdict.status {
                    VerdictStatus::Pass => {
                        doc.phases[idx].verdict = Some(verdict);
                        break;
                    }
                    VerdictStatus::NeedsFix => {
                        if phase_round >= self.max_rounds_per_phase {
                            return Ok(ConvergenceOutcome::Escalated(EscalationRequired {
                                reason: format!(
                                    "Phase {} ({}) escalated: {} NEEDS_FIX rounds exhausted without PASS (last verdict: {})",
                                    phase.index,
                                    phase.name,
                                    self.max_rounds_per_phase,
                                    verdict.status.as_str()
                                ),
                                phase_index: phase.index,
                                phase_name: phase.name,
                                rounds: self.rounds.clone(),
                                last_verdict: Some(verdict),
                            }));
                        }
                        doc = self.refine(&doc, &phase, &verdict, ctx)?;
                    }
                    VerdictStatus::Fail
                    | VerdictStatus::InfraError
                    | VerdictStatus::Inconclusive => {
                        return Ok(ConvergenceOutcome::Escalated(EscalationRequired {
                            reason: format!(
                                "validator returned {} for phase {}",
                                verdict.status.as_str(),
                                phase.index
                            ),
                            phase_index: phase.index,
                            phase_name: phase.name,
                            rounds: self.rounds.clone(),
                            last_verdict: Some(verdict),
                        }));
                    }
                }
            }
        }

        Ok(ConvergenceOutcome::Converged(doc))
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
}
