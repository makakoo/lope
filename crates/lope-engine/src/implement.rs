//! Autonomous zero-human swarm implementation (`implement.py` & `implementation_runner.py`, Sprint S7, `FR35`).

use crate::executor::{ImplementationResult, PhaseExecutor};
use crate::minimality::implementation_directive;
use crate::models::{EscalationRequired, ExecutionReport, Phase, PhaseVerdict, SprintDoc};
use lope_providers::budget::Context;
use lope_providers::generic::Validator;
use lope_providers::invocation::invoke_generate;
use lope_providers::pool::Pool;
use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementRoster {
    pub implementers: Vec<String>,
    pub escalations: Vec<String>,
    pub validators: Vec<String>,
    pub primary: String,
}

fn unique_ordered(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for item in items {
        if !item.is_empty() && seen.insert(item.clone()) {
            out.push(item);
        }
    }
    out
}

pub fn parse_csv_names(raw: Option<&str>) -> Vec<String> {
    let Some(s) = raw else {
        return Vec::new();
    };
    unique_ordered(
        s.replace('\n', ",")
            .split(',')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string),
    )
}

pub fn parse_selection(raw: &str, available: &[String]) -> Vec<String> {
    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    for part in raw.trim().split(',') {
        let token = part.trim();
        if token.is_empty() {
            continue;
        }
        let name = if token.chars().all(|c| c.is_ascii_digit()) {
            let Ok(one_based) = token.parse::<usize>() else {
                continue;
            };
            let Some(idx) = one_based.checked_sub(1) else {
                continue;
            };
            let Some(picked) = available.get(idx) else {
                continue;
            };
            picked.clone()
        } else {
            token.to_string()
        };
        if seen.insert(name.clone()) {
            selected.push(name);
        }
    }
    selected
}

pub fn default_escalation_names(available: &[String], primary: &str) -> Vec<String> {
    available
        .iter()
        .filter(|n| n.as_str() != primary)
        .take(2)
        .cloned()
        .collect()
}

fn validate_names(
    label: &str,
    selected: Vec<String>,
    available: &[String],
) -> Result<Vec<String>, String> {
    let missing: Vec<&str> = selected
        .iter()
        .filter(|n| !available.iter().any(|a| a == *n))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        let avail_str = if available.is_empty() {
            "(none)".to_string()
        } else {
            available.join(", ")
        };
        return Err(format!(
            "Unknown {label}: {}. Available: {avail_str}",
            missing.join(", ")
        ));
    }
    Ok(selected)
}

fn prompt_for_names_with_io<R: BufRead, W: Write>(
    label: &str,
    validate_label: &str,
    available: &[String],
    default: &[String],
    reader: &mut R,
    writer: &mut W,
) -> Result<Vec<String>, String> {
    let _ = writeln!(writer, "\nSelect {label} agents:");
    for (idx, name) in available.iter().enumerate() {
        let marker = if default.iter().any(|d| d == name) {
            "  (default)"
        } else {
            ""
        };
        let _ = writeln!(writer, "  [{}] {name}{marker}", idx + 1);
    }
    let default_str = if default.is_empty() {
        "(none)".to_string()
    } else {
        default.join(", ")
    };
    let _ = writeln!(writer, "Default: {default_str}");
    let _ = write!(writer, "> ");
    let _ = writer.flush();
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => return Err(format!("EOF while prompting for {validate_label}")),
        Ok(_) => {}
        Err(e) => return Err(format!("failed reading {validate_label}: {e}")),
    }
    let parsed = parse_selection(&line, available);
    let chosen = if parsed.is_empty() {
        default.to_vec()
    } else {
        parsed
    };
    validate_names(validate_label, chosen, available)
}

pub fn resolve_implement_roster_interactive_with_io<R: BufRead, W: Write>(
    implementers: Option<&str>,
    escalations: Option<&str>,
    available: &[String],
    cfg_primary: Option<&str>,
    interactive: bool,
    reader: &mut R,
    writer: &mut W,
) -> Result<ImplementRoster, String> {
    if available.is_empty() {
        return Err("No validators available. Run: lope team list".to_string());
    }
    let primary_default = cfg_primary
        .filter(|p| !p.is_empty() && available.iter().any(|a| a == p))
        .map(str::to_string)
        .unwrap_or_else(|| available[0].clone());
    let agent_default = vec![primary_default.clone()];
    let escalation_default = default_escalation_names(available, &primary_default);

    let impl_parsed = parse_csv_names(implementers);
    let impls = if impl_parsed.is_empty() {
        if interactive {
            prompt_for_names_with_io(
                "implementation",
                "implementation agents",
                available,
                &agent_default,
                reader,
                writer,
            )?
        } else {
            return Err(format!(
                "lope implement needs --agents in non-interactive mode. Available: {}",
                available.join(", ")
            ));
        }
    } else {
        validate_names("implementation agents", impl_parsed, available)?
    };

    let esc_parsed = parse_csv_names(escalations);
    let escs = if esc_parsed.is_empty() {
        if interactive {
            prompt_for_names_with_io(
                "escalation",
                "escalation agents",
                available,
                &escalation_default,
                reader,
                writer,
            )?
        } else {
            return Err(format!(
                "lope implement needs --escalate-to in non-interactive mode. Available: {}",
                available.join(", ")
            ));
        }
    } else {
        validate_names("escalation agents", esc_parsed, available)?
    };

    let primary = impls
        .first()
        .cloned()
        .ok_or_else(|| "No implementation agent selected".to_string())?;
    let validators = unique_ordered(escs.iter().cloned().chain(impls.iter().cloned()));
    Ok(ImplementRoster {
        implementers: impls,
        escalations: escs,
        validators,
        primary,
    })
}

pub fn resolve_implement_roster_interactive(
    implementers: Option<&str>,
    escalations: Option<&str>,
    available: &[String],
    cfg_primary: Option<&str>,
    interactive: bool,
) -> Result<ImplementRoster, String> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut stdout = io::stdout();
    resolve_implement_roster_interactive_with_io(
        implementers,
        escalations,
        available,
        cfg_primary,
        interactive,
        &mut reader,
        &mut stdout,
    )
}

pub fn resolve_implement_roster(
    implementers: Option<&str>,
    escalations: Option<&str>,
    available: &[String],
) -> Result<ImplementRoster, String> {
    resolve_implement_roster_interactive_with_io(
        implementers,
        escalations,
        available,
        None,
        false,
        &mut io::empty(),
        &mut io::sink(),
    )
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
        if raw.chars().all(|c| c.is_ascii_digit()) {
            let avail = doc
                .phases
                .iter()
                .map(|p| p.index.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let avail_str = if avail.is_empty() {
                "none".to_string()
            } else {
                avail
            };
            return Err(format!("Phase {raw} not found. Available phases: {avail_str}"));
        }
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
    let mut parts = vec![
        "Role: Autonomous Implementation Agent".to_string(),
        "Primary Directive: implement this sprint phase directly; the outer Lope process performs fallback and validation.".to_string(),
        String::new(),
        "## Execution protocol".to_string(),
        "- Uninterrupted momentum: implement now. Do not ask the human for input, approval, or clarification.".to_string(),
        "- Internal resolution: solve repository-local blockers, dependency conflicts, and structural questions directly.".to_string(),
        "- Orchestration boundary: do not invoke Lope, another AI CLI, an MCP agent, or a nested model process.".to_string(),
        "- Outer ownership: the outer Lope process performs fallback, escalation, and validation after this writer returns.".to_string(),
        "- Strict constraint: the human is out of the loop after roster selection.".to_string(),
        "- Repository safety: you are the single file-writing agent for this phase. Do not spawn parallel editors in the same checkout.".to_string(),
        String::new(),
    ];
    let minimality = implementation_directive(None, Some(&doc.domain));
    if !minimality.is_empty() {
        parts.push("## Minimality discipline".to_string());
        parts.push(minimality);
        parts.push(String::new());
    }
    parts.extend([
        "## Selected team".to_string(),
        format!("- Implementation agents: {}", roster.implementers.join(", ")),
        format!("- Escalation agents: {}", roster.escalations.join(", ")),
        format!("- First writing lead: {}", roster.primary),
        String::new(),
        format!("## Sprint: {}", doc.title),
        format!("## Phase {}: {}", phase.index, phase.name),
        String::new(),
        format!("Goal: {}", phase.goal),
        String::new(),
    ]);
    if !phase.criteria.is_empty() {
        parts.push("## Exit criteria".to_string());
        for c in &phase.criteria {
            parts.push(format!("- {c}"));
        }
        parts.push(String::new());
    }
    if !phase.artifacts.is_empty() {
        parts.push("## Files / artifacts / deliverables".to_string());
        for a in &phase.artifacts {
            parts.push(format!("- {a}"));
        }
        parts.push(String::new());
    }
    if !phase.checks.is_empty() {
        parts.push("## Tests / checks / success metrics".to_string());
        for c in &phase.checks {
            parts.push(format!("- {c}"));
        }
        parts.push(String::new());
    }
    if let Some(v) = fix_context {
        parts.push("## Required fixes from prior Lope review".to_string());
        if !v.rationale.is_empty() {
            parts.push(format!("Rationale: {}", v.rationale));
        }
        for f in &v.required_fixes {
            parts.push(format!("- {f}"));
        }
        parts.push(String::new());
    }
    parts.extend([
        "## Completion contract".to_string(),
        "Implement the phase directly in the repository. Run the relevant checks when possible.".to_string(),
        "When done, return a terse summary with files changed and commands/tests run.".to_string(),
        "Do not wait for the human. If uncertain, choose the smallest safe implementation that satisfies the phase and let the Lope validators catch issues.".to_string(),
    ]);
    parts.join("\n")
}

pub fn render_dry_run(doc: &SprintDoc, roster: &ImplementRoster) -> String {
    let phases = if doc.phases.is_empty() {
        "none".to_string()
    } else {
        doc.phases
            .iter()
            .map(|p| p.index.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    [
        format!("Lope implement dry-run: {}", doc.title),
        format!("Phases: {phases}"),
        format!("Implementation agents: {}", roster.implementers.join(", ")),
        format!("Escalation agents: {}", roster.escalations.join(", ")),
        format!("Validators: {}", roster.validators.join(", ")),
        format!("Writing lead: {}", roster.primary),
        "Mode: zero-human after roster selection, single-writer safety model".to_string(),
    ]
    .join("\n")
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
    let mut healer = crate::healer::SelfHealer::new();
    invoke_writer_failover_with_reviewers(
        writers,
        writers,
        &mut healer,
        prompt,
        timeout,
        phase_index,
        ctx,
    )
}

pub fn invoke_writer_failover_with_reviewers(
    writers: &[Arc<dyn Validator>],
    reviewers: &[Arc<dyn Validator>],
    healer: &mut crate::healer::SelfHealer,
    prompt: &str,
    timeout: f64,
    phase_index: usize,
    ctx: Option<&Context>,
) -> ImplementationResult {
    let before = workspace_snapshot(Path::new("."));
    let mut writer_ctx = ctx.cloned();
    if let Some(ref mut c) = writer_ctx {
        c.implementation = true;
    }
    let finish_ok = |out: String, elapsed: f64, writer_name: String| {
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
        ImplementationResult {
            ok: true,
            summary: out,
            files_changed,
            error: String::new(),
            duration_seconds: elapsed,
            writer: writer_name,
        }
    };
    let mut errors = Vec::new();
    for w in writers {
        if !w.available() {
            errors.push(format!("{}: unavailable", w.name()));
            continue;
        }
        let t0 = Instant::now();
        match invoke_generate(
            w.as_ref(),
            prompt,
            timeout,
            writer_ctx.as_ref(),
            Some("implement_write"),
        ) {
            Ok(out) => return finish_ok(out, t0.elapsed().as_secs_f64(), w.name().to_string()),
            Err(e) => {
                let err_msg = e.to_string();
                if lope_providers::verdict::is_flag_error(&err_msg) {
                    if let Some(healed) = crate::healer::try_self_heal_for_validator(
                        healer,
                        w.as_ref(),
                        reviewers,
                        &err_msg,
                        writer_ctx.as_ref(),
                    ) {
                        if let Ok(out) = invoke_generate(
                            healed.as_ref(),
                            prompt,
                            timeout,
                            writer_ctx.as_ref(),
                            Some("implement_write"),
                        ) {
                            return finish_ok(
                                out,
                                t0.elapsed().as_secs_f64(),
                                w.name().to_string(),
                            );
                        }
                    }
                }
                errors.push(format!("phase {phase_index} {}: {err_msg}", w.name()));
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
    let executor = PhaseExecutor::new(escalation_pool.clone(), max_rounds_per_phase, timeout);
    let doc_snapshot = doc.clone();
    let reviewers = escalation_pool.validators;
    let mut healer = crate::healer::SelfHealer::new();
    let mut impl_fn = |phase: &Phase, fix_context: Option<&PhaseVerdict>| -> ImplementationResult {
        let prompt = build_swarm_prompt(phase, &doc_snapshot, roster, fix_context);
        invoke_writer_failover_with_reviewers(
            writers,
            &reviewers,
            &mut healer,
            &prompt,
            timeout,
            phase.index,
            ctx,
        )
    };
    executor.run(doc, &mut impl_fn, gate_runner, ctx, on_event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lope_providers::budget::RunBudget;
    use lope_providers::errors::ProviderError;
    use lope_providers::models::{ValidatorResult, VerdictStatus};
    use std::io::Cursor;
    use std::sync::Mutex;

    fn test_context(implementation: bool) -> Context {
        Context {
            budget: Arc::new(RunBudget::new(
                "implement",
                None,
                false,
                16,
                100_000_000,
                100_000_000,
            )),
            mode: "implement".into(),
            stage: "provider".into(),
            validator: String::new(),
            registry: None,
            progress: None,
            call_id: None,
            implementation,
            cleanup_result: Arc::new(Mutex::new(None)),
            owned_paths: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[derive(Default)]
    struct SpyValidator {
        name: String,
        gen_impl_flags: Mutex<Vec<bool>>,
        val_impl_flags: Mutex<Vec<bool>>,
    }

    impl SpyValidator {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                ..Default::default()
            }
        }
    }

    impl Validator for SpyValidator {
        fn name(&self) -> &str {
            &self.name
        }
        fn available(&self) -> bool {
            true
        }
        fn transport(&self) -> &'static str {
            "mock"
        }
        fn validate(&self, _prompt: &str, _timeout: f64, ctx: Option<&Context>) -> ValidatorResult {
            self.val_impl_flags
                .lock()
                .unwrap()
                .push(ctx.is_some_and(|c| c.implementation));
            ValidatorResult {
                validator_name: self.name.clone(),
                verdict: PhaseVerdict::new(VerdictStatus::Pass, "ok", 0.0, &self.name),
                raw_response: "VERDICT: PASS".into(),
                error: String::new(),
                flag_error_hint: String::new(),
                repair_attempted: false,
                repair_status: None,
                repaired_response: String::new(),
                initial_parse_status: None,
                parse_error_category: None,
            }
        }
        fn generate(
            &self,
            _prompt: &str,
            _timeout: f64,
            ctx: Option<&Context>,
        ) -> Result<String, ProviderError> {
            let is_impl = ctx.is_some_and(|c| c.implementation);
            if is_impl {
                self.gen_impl_flags.lock().unwrap().push(true);
                Ok("implemented".into())
            } else {
                self.val_impl_flags.lock().unwrap().push(false);
                Ok("---VERDICT---\nstatus: PASS\nconfidence: 0.95\nrationale: verified in src/lib.rs:10 (cargo test passed)\n---END---\n"
                    .into())
            }
        }
    }

    #[test]
    fn phase_filter_and_roster() {
        let md = "# SPRINT-X\n\n## Origin\nX\n\n## Phases\n\n### Phase 1: a\n**Goal:** A\n\n### Phase 2: b\n**Goal:** B\n\n### Phase 3: c\n**Goal:** C\n";
        let doc = SprintDoc::from_markdown(md, None);
        let filtered = apply_phase_filter(&doc, Some("2-3")).expect("filter");
        assert_eq!(filtered.phases.len(), 2);
        assert_eq!(filtered.phases[0].index, 2);
        let err = apply_phase_filter(&doc, Some("99")).unwrap_err();
        assert!(err.contains("Phase 99 not found"));

        let avail = vec![
            "pi".to_string(),
            "claude".to_string(),
            "opencode".to_string(),
            "antigravity".to_string(),
        ];
        let roster =
            resolve_implement_roster(Some("pi,antigravity"), Some("claude,opencode"), &avail)
                .unwrap();
        assert_eq!(roster.implementers, vec!["pi", "antigravity"]);
        assert_eq!(roster.escalations, vec!["claude", "opencode"]);
        assert_eq!(roster.primary, "pi");
        assert_eq!(
            roster.validators,
            vec!["claude", "opencode", "pi", "antigravity"]
        );
    }

    #[test]
    fn interactive_roster_defaults_and_numeric_indices() {
        let avail = vec![
            "pi".to_string(),
            "claude".to_string(),
            "opencode".to_string(),
            "antigravity".to_string(),
        ];
        let mut out = Vec::new();
        let mut r1 = Cursor::new(b"\n\n");
        let defaults = resolve_implement_roster_interactive_with_io(
            None,
            None,
            &avail,
            Some("pi"),
            true,
            &mut r1,
            &mut out,
        )
        .unwrap();
        assert_eq!(defaults.implementers, vec!["pi"]);
        assert_eq!(defaults.escalations, vec!["claude", "opencode"]);
        assert_eq!(defaults.primary, "pi");

        out.clear();
        let mut r2 = Cursor::new(b"1\n2\n");
        let numeric = resolve_implement_roster_interactive_with_io(
            None,
            None,
            &avail[..3],
            Some("pi"),
            true,
            &mut r2,
            &mut out,
        )
        .unwrap();
        assert_eq!(numeric.implementers, vec!["pi"]);
        assert_eq!(numeric.escalations, vec!["claude"]);
    }

    #[test]
    fn render_dry_run_and_swarm_prompt_match_python() {
        let md = "# SPRINT-X\n\n## Origin\nX\n\n## Phases\n\n### Phase 1: a\n**Goal:** A\n";
        let doc = SprintDoc::from_markdown(md, None);
        let roster = ImplementRoster {
            implementers: vec!["pi".into()],
            escalations: vec!["claude".into(), "opencode".into()],
            validators: vec!["claude".into(), "opencode".into(), "pi".into()],
            primary: "pi".into(),
        };
        let dry = render_dry_run(&doc, &roster);
        assert!(dry.contains("Lope implement dry-run: SPRINT-X"));
        assert!(dry.contains("Writing lead: pi"));
        assert!(dry.contains("single-writer safety model"));

        let mut fix = PhaseVerdict::new(VerdictStatus::NeedsFix, "fix x", 0.0, "kimi");
        fix.required_fixes = vec!["fix x".into()];
        let roster2 = ImplementRoster {
            implementers: vec!["pi".into(), "antigravity".into()],
            escalations: vec!["kimi".into(), "qwen".into()],
            validators: vec!["kimi".into(), "qwen".into(), "pi".into(), "antigravity".into()],
            primary: "pi".into(),
        };
        let prompt = build_swarm_prompt(&doc.phases[0], &doc, &roster2, Some(&fix));
        assert!(prompt.contains("Escalation agents: kimi, qwen"));
        assert!(prompt.contains("Implementation agents: pi, antigravity"));
        assert!(prompt.contains("Do not ask the human"));
        assert!(prompt.contains("do not invoke Lope"));
        assert!(prompt.contains("outer Lope process performs fallback"));
        assert!(prompt.contains("fix x"));
    }

    #[test]
    fn writer_gets_implementation_true_while_validator_stays_readonly() {
        let md = "# SPRINT-W\n\n## Origin\nW\n\n## Phases\n\n### Phase 1: a\n**Goal:** G\n";
        let mut doc = SprintDoc::from_markdown(md, None);
        let writer = Arc::new(SpyValidator::new("w"));
        let reviewer = Arc::new(SpyValidator::new("r"));
        let roster = ImplementRoster {
            implementers: vec!["w".into()],
            escalations: vec!["r".into()],
            validators: vec!["r".into()],
            primary: "w".into(),
        };
        let pool = Pool {
            validators: vec![reviewer.clone()],
            primary: "r".into(),
            parallel: false,
            ensemble: false,
            unsupported: vec![],
            warnings: vec![],
        };
        let ctx = test_context(false);
        let report = run_implement(
            &mut doc,
            &roster,
            &[writer.clone()],
            pool,
            1,
            30.0,
            None,
            Some(&ctx),
            None,
        )
        .expect("run_implement succeeds");
        assert!(report.ok);
        assert_eq!(*writer.gen_impl_flags.lock().unwrap(), vec![true]);
        let val_flags = reviewer.val_impl_flags.lock().unwrap().clone();
        assert!(!val_flags.is_empty() && val_flags.iter().all(|&f| !f));
    }
}
