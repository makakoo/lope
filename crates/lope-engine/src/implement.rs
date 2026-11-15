//! Autonomous zero-human swarm implementation (`implement.py` & `implementation_runner.py`, Sprint S7, `FR35`).

use crate::executor::{ImplementationResult, PhaseExecutor};
use crate::models::{EscalationRequired, ExecutionReport, Phase, PhaseVerdict, SprintDoc};
use lope_providers::budget::Context;
use lope_providers::generic::Validator;
use lope_providers::invocation::invoke_generate;
use lope_providers::pool::Pool;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementRoster {
    pub implementers: Vec<String>,
    pub escalations: Vec<String>,
}

pub fn parse_csv_names(raw: Option<&str>) -> Vec<String> {
    let Some(s) = raw else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for part in s.split(',') {
        let name = part.trim();
        if !name.is_empty() && seen.insert(name.to_string()) {
            out.push(name.to_string());
        }
    }
    out
}

pub fn resolve_implement_roster(
    implementers: Option<&str>,
    escalations: Option<&str>,
    available: &[String],
) -> Result<ImplementRoster, String> {
    let impls = parse_csv_names(implementers);
    let escs = parse_csv_names(escalations);
    if impls.is_empty() || escs.is_empty() {
        return Err(
            "non-interactive `lope implement` requires --agents and --escalate-to".to_string(),
        );
    }
    for name in impls.iter().chain(escs.iter()) {
        if !available.is_empty() && !available.iter().any(|a| a == name) {
            return Err(format!(
                "unknown validator `{name}` (available: {})",
                available.join(", ")
            ));
        }
    }
    Ok(ImplementRoster {
        implementers: impls,
        escalations: escs,
    })
}

pub fn apply_phase_filter(doc: &SprintDoc, phase_arg: Option<&str>) -> Result<SprintDoc, String> {
    let Some(raw) = phase_arg.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(doc.clone());
    };
    let mut wanted = HashSet::new();
    for part in raw.split(',') {
        let token = part.trim();
        if token.is_empty() {
            continue;
        }
        if let Some((a, b)) = token.split_once('-') {
            let start: usize = a
                .trim()
                .parse()
                .map_err(|_| format!("invalid phase range `{token}`"))?;
            let end: usize = b
                .trim()
                .parse()
                .map_err(|_| format!("invalid phase range `{token}`"))?;
            for idx in start..=end {
                wanted.insert(idx);
            }
        } else {
            let idx: usize = token
                .parse()
                .map_err(|_| format!("invalid phase index `{token}`"))?;
            wanted.insert(idx);
        }
    }
    let filtered: Vec<Phase> = doc
        .phases
        .iter()
        .filter(|p| wanted.contains(&p.index))
        .cloned()
        .collect();
    if filtered.is_empty() {
        return Err(format!("no phases matched filter `{raw}`"));
    }
    let mut out = doc.clone();
    out.phases = filtered;
    Ok(out)
}

pub fn build_swarm_prompt(
    phase: &Phase,
    doc: &SprintDoc,
    roster: &ImplementRoster,
    fix_context: Option<&PhaseVerdict>,
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
    let checks = phase
        .checks
        .iter()
        .map(|c| format!("- {c}"))
        .collect::<Vec<_>>()
        .join("\n");
    let fix_section = match fix_context {
        Some(v) => {
            let fixes = v
                .required_fixes
                .iter()
                .map(|f| format!("- {f}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "\n\n## Required Fixes From Validation\nRationale: {}\n{fixes}\n",
                v.rationale
            )
        }
        None => String::new(),
    };

    format!(
        "You are an autonomous implementation agent in a Lope swarm (implementers: {}; escalation reviewers: {}).\n\n\
Sprint: {}\nOrigin: {}\n\n\
### Phase {}: {}\n\
**Goal:** {}\n\n\
**Criteria:**\n{criteria}\n\n\
**Deliverables:**\n{artifacts}\n\n\
**Checks:**\n{checks}\
{fix_section}\n\n\
Implement this phase completely in the current workspace, run the checks, and summarize your changes and verification output.",
        roster.implementers.join(", "),
        roster.escalations.join(", "),
        doc.title,
        doc.origin,
        phase.index,
        phase.name,
        phase.goal
    )
}

pub fn render_dry_run(
    doc: &SprintDoc,
    roster: &ImplementRoster,
    parallel: bool,
    timeout: f64,
) -> String {
    let mut lines = vec![
        String::new(),
        format!("Lope implement: {} ({} phases)", doc.title, doc.phases.len()),
        String::new(),
        format!("Lope implement dry-run: {}", doc.title),
        format!("Phases: {}", doc.phases.len()),
        format!("Implementation agents: {}", roster.implementers.join(", ")),
        format!("Escalation agents: {}", roster.escalations.join(", ")),
        format!(
            "Mode: {}, {}",
            if parallel { "parallel" } else { "sequential" },
            format!("timeout {timeout}s")
        ),
    ];
    for p in &doc.phases {
        lines.push(format!("  Phase {}: {} — {}", p.index, p.name, p.goal));
    }
    lines.join("\n")
}

pub fn workspace_snapshot(cwd: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn invoke_writer_failover(
    writers: &[Arc<dyn Validator>],
    prompt: &str,
    timeout: f64,
    phase_index: usize,
    ctx: Option<&Context>,
) -> ImplementationResult {
    let before = workspace_snapshot(Path::new("."));
    let mut errors = Vec::new();
    for w in writers {
        if !w.available() {
            errors.push(format!("{}: unavailable", w.name()));
            continue;
        }
        let t0 = Instant::now();
        match invoke_generate(w.as_ref(), prompt, timeout, ctx, Some("implement_write")) {
            Ok(out) => {
                let elapsed = t0.elapsed().as_secs_f64();
                let after = workspace_snapshot(Path::new("."));
                let mut files_changed = Vec::new();
                if let (Some(b), Some(a)) = (before.as_deref(), after.as_deref()) {
                    if b != a {
                        for line in a.lines() {
                            if line.len() > 3 {
                                files_changed.push(line[3..].trim().to_string());
                            }
                        }
                    }
                }
                return ImplementationResult {
                    ok: true,
                    summary: out,
                    files_changed,
                    error: String::new(),
                    duration_seconds: elapsed,
                    writer: w.name().to_string(),
                };
            }
            Err(e) => {
                errors.push(format!("phase {phase_index} {}: {e}", w.name()));
            }
        }
    }

    ImplementationResult {
        ok: false,
        summary: String::new(),
        files_changed: Vec::new(),
        error: errors.join("; "),
        duration_seconds: 0.0,
        writer: String::new(),
    }
}

pub fn run_implement(
    doc: &mut SprintDoc,
    roster: &ImplementRoster,
    writers: &[Arc<dyn Validator>],
    escalation_pool: Pool,
    max_rounds_per_phase: usize,
    timeout: f64,
    gate_runner: Option<&mut dyn FnMut(&Phase) -> (bool, String)>,
    ctx: Option<&Context>,
    on_event: Option<&mut dyn FnMut(&str)>,
) -> Result<ExecutionReport, EscalationRequired> {
    let executor = PhaseExecutor::new(escalation_pool, max_rounds_per_phase, timeout);
    let doc_snapshot = doc.clone();
    let mut impl_fn = |phase: &Phase, fix_context: Option<&PhaseVerdict>| -> ImplementationResult {
        let prompt = build_swarm_prompt(phase, &doc_snapshot, roster, fix_context);
        invoke_writer_failover(writers, &prompt, timeout, phase.index, ctx)
    };
    executor.run(doc, &mut impl_fn, gate_runner, ctx, on_event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_filter_and_roster() {
        let md = "# SPRINT-X\n\n## Origin\nX\n\n## Phases\n\n### Phase 1: a\n**Goal:** A\n\n### Phase 2: b\n**Goal:** B\n\n### Phase 3: c\n**Goal:** C\n";
        let doc = SprintDoc::from_markdown(md, None);
        let filtered = apply_phase_filter(&doc, Some("2-3")).expect("filter");
        assert_eq!(filtered.phases.len(), 2);
        assert_eq!(filtered.phases[0].index, 2);

        let avail = vec!["claude".to_string(), "codex".to_string()];
        let roster = resolve_implement_roster(Some("claude"), Some("codex"), &avail).unwrap();
        assert_eq!(roster.implementers, vec!["claude"]);
        assert_eq!(roster.escalations, vec!["codex"]);
    }
}
