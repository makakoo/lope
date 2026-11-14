//! Phase-by-phase execution + two-stage validation loop (`executor.py`, Sprint S7, `FR33`).

use crate::models::{
    EscalationRequired, ExecutionReport, Phase, PhaseVerdict, Round, SprintDoc, VerdictStatus,
};
use crate::pool_validate::pool_validate;
use lope_providers::budget::Context;
use lope_providers::pool::Pool;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct ImplementationResult {
    pub ok: bool,
    pub summary: String,
    pub files_changed: Vec<String>,
    pub error: String,
    pub duration_seconds: f64,
    pub writer: String,
}

impl Default for ImplementationResult {
    fn default() -> Self {
        Self {
            ok: false,
            summary: String::new(),
            files_changed: Vec::new(),
            error: String::new(),
            duration_seconds: 0.0,
            writer: String::new(),
        }
    }
}

pub fn single_stage_enabled() -> bool {
    std::env::var("LOPE_SINGLE_STAGE")
        .map(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

pub fn build_validation_prompt(
    phase: &Phase,
    doc: &SprintDoc,
    impl_res: &ImplementationResult,
    gate_summary: &str,
    stage_focus: Option<&str>,
) -> String {
    let criteria = phase
        .criteria
        .iter()
        .map(|c| format!("- {c}"))
        .collect::<Vec<_>>()
        .join("\n");
    let artifacts = phase
        .artifacts
        .iter()
        .map(|a| format!("- {a}"))
        .collect::<Vec<_>>()
        .join("\n");
    let files = if impl_res.files_changed.is_empty() {
        "(none reported)".to_string()
    } else {
        impl_res.files_changed.join(", ")
    };
    let focus_block = stage_focus
        .map(|f| format!("\nValidation Stage Focus:\n{f}\n"))
        .unwrap_or_default();
    let gate_block = if gate_summary.trim().is_empty() {
        String::new()
    } else {
        format!("\nDeterministic Gate Results:\n{gate_summary}\n")
    };

    format!(
        "You are validating Phase {} ({}) of sprint \"{}\".\n\n\
**Goal:** {}\n\
**Criteria:**\n{criteria}\n\
**Expected deliverables:**\n{artifacts}\n\
{focus_block}\n\
Implementation summary ({}):\n{}\n\
Files changed: {files}\n\
{gate_block}\n\
Return a `---VERDICT---` ... `---END---` block (PASS, NEEDS_FIX, or FAIL) citing concrete verification evidence.",
        phase.index,
        phase.name,
        doc.title,
        phase.goal,
        if impl_res.writer.is_empty() {
            "implementer"
        } else {
            impl_res.writer.as_str()
        },
        impl_res.summary
    )
}

pub struct PhaseExecutor {
    pub pool: Pool,
    pub max_rounds_per_phase: usize,
    pub timeout_seconds: f64,
}

impl PhaseExecutor {
    pub fn new(pool: Pool, max_rounds_per_phase: usize, timeout_seconds: f64) -> Self {
        Self {
            pool,
            max_rounds_per_phase: max_rounds_per_phase.max(1),
            timeout_seconds,
        }
    }

    pub fn run(
        &self,
        doc: &mut SprintDoc,
        impl_fn: &mut dyn FnMut(&Phase, Option<&PhaseVerdict>) -> ImplementationResult,
        mut gate_runner: Option<&mut dyn FnMut(&Phase) -> (bool, String)>,
        ctx: Option<&Context>,
        mut on_event: Option<&mut dyn FnMut(&str)>,
    ) -> Result<ExecutionReport, EscalationRequired> {
        let run_start = Instant::now();
        let single_stage = single_stage_enabled();
        let mut rounds = Vec::new();
        let mut phase_verdicts = Vec::new();

        for idx in 0..doc.phases.len() {
            let phase = doc.phases[idx].clone();
            let mut fix_context: Option<PhaseVerdict> = None;
            let mut final_verdict: Option<PhaseVerdict> = None;

            for round_num in 1..=self.max_rounds_per_phase {
                if let Some(cb) = on_event.as_deref_mut() {
                    cb(&format!(
                        "Phase {} ({}) — attempt {round_num}/{}",
                        phase.index, phase.name, self.max_rounds_per_phase
                    ));
                }

                let impl_res = impl_fn(&phase, fix_context.as_ref());
                rounds.push(Round {
                    number: rounds.len() + 1,
                    proposer: "implementer".to_string(),
                    text: impl_res.summary.clone(),
                    verdict: None,
                });

                if !impl_res.ok {
                    let v = PhaseVerdict::new(
                        VerdictStatus::InfraError,
                        format!("implementation failed: {}", impl_res.error),
                        impl_res.duration_seconds,
                        "implementer",
                    );
                    if round_num >= self.max_rounds_per_phase {
                        return Err(EscalationRequired {
                            reason: format!(
                                "implementation failed on phase {}: {}",
                                phase.index, impl_res.error
                            ),
                            phase_index: phase.index,
                            phase_name: phase.name,
                            rounds,
                            last_verdict: Some(v),
                        });
                    }
                    fix_context = Some(v);
                    continue;
                }

                let mut gate_summary = String::new();
                if let Some(runner) = gate_runner.as_deref_mut() {
                    let (ok, summary) = runner(&phase);
                    gate_summary = summary.clone();
                    if !ok {
                        let mut v = PhaseVerdict::new(
                            VerdictStatus::NeedsFix,
                            format!("deterministic gate failure:\n{summary}"),
                            0.0,
                            "gates",
                        );
                        v.required_fixes = vec![format!("Fix failing deterministic gates:\n{summary}")];
                        rounds.push(Round {
                            number: rounds.len() + 1,
                            proposer: "gates".to_string(),
                            text: summary,
                            verdict: Some(v.clone()),
                        });
                        if round_num >= self.max_rounds_per_phase {
                            return Err(EscalationRequired {
                                reason: format!("gates failed on phase {}", phase.index),
                                phase_index: phase.index,
                                phase_name: phase.name,
                                rounds,
                                last_verdict: Some(v),
                            });
                        }
                        fix_context = Some(v);
                        continue;
                    }
                }

                let stage1_focus = if single_stage {
                    None
                } else {
                    Some("Stage 1 (Spec Compliance): verify every criterion and deliverable is implemented and backed by evidence.")
                };
                let p1 = build_validation_prompt(&phase, doc, &impl_res, &gate_summary, stage1_focus);
                let r1 = pool_validate(&self.pool, &p1, self.timeout_seconds, ctx, "execute_stage1");
                let mut v1 = r1.verdict.clone();
                v1.stage = Some(if single_stage { "single" } else { "spec" }.to_string());
                rounds.push(Round {
                    number: rounds.len() + 1,
                    proposer: "validator".to_string(),
                    text: r1.raw_response.clone(),
                    verdict: Some(v1.clone()),
                });

                if v1.status != VerdictStatus::Pass {
                    if v1.status == VerdictStatus::NeedsFix && round_num < self.max_rounds_per_phase {
                        fix_context = Some(v1);
                        continue;
                    }
                    return Err(EscalationRequired {
                        reason: format!(
                            "phase {} validation returned {}",
                            phase.index,
                            v1.status.as_str()
                        ),
                        phase_index: phase.index,
                        phase_name: phase.name,
                        rounds,
                        last_verdict: Some(v1),
                    });
                }

                if !single_stage {
                    let p2 = build_validation_prompt(
                        &phase,
                        doc,
                        &impl_res,
                        &gate_summary,
                        Some("Stage 2 (Code Quality): verify cleanliness, edge cases, error handling, and tests."),
                    );
                    let r2 = pool_validate(&self.pool, &p2, self.timeout_seconds, ctx, "execute_stage2");
                    let mut v2 = r2.verdict.clone();
                    v2.stage = Some("quality".to_string());
                    rounds.push(Round {
                        number: rounds.len() + 1,
                        proposer: "validator".to_string(),
                        text: r2.raw_response.clone(),
                        verdict: Some(v2.clone()),
                    });
                    if v2.status != VerdictStatus::Pass {
                        if v2.status == VerdictStatus::NeedsFix
                            && round_num < self.max_rounds_per_phase
                        {
                            fix_context = Some(v2);
                            continue;
                        }
                        return Err(EscalationRequired {
                            reason: format!(
                                "phase {} quality validation returned {}",
                                phase.index,
                                v2.status.as_str()
                            ),
                            phase_index: phase.index,
                            phase_name: phase.name,
                            rounds,
                            last_verdict: Some(v2),
                        });
                    }
                    final_verdict = Some(v2);
                } else {
                    final_verdict = Some(v1);
                }
                break;
            }

            if let Some(v) = final_verdict {
                doc.phases[idx].verdict = Some(v.clone());
                phase_verdicts.push(v);
            }
        }

        Ok(ExecutionReport {
            sprint_doc: doc.clone(),
            phase_verdicts,
            ok: true,
            error: String::new(),
            total_duration_seconds: run_start.elapsed().as_secs_f64(),
        })
    }
}
