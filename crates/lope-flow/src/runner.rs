//! FlowRunner — wave-based graph workflow executor (`lope/flow/runner.py`, Sprint S7, `FR36`).

use crate::model::{
    edge_matches, outcome_instructions, parse_outcome_block, verdict_to_outcome, Blackboard,
    FlowConfigError, FlowContext, FlowEdge, FlowGraph, FlowNode, NodeKind, NodeResult,
};
use crate::report::FlowReport;
use crate::stylesheet::{parse_stylesheet, split_names, Stylesheet};
use crate::validate::validate_graph;
use lope_core::config::LopeCfg;
use lope_engine::gates::{load_gate_specs, run_gate, GateSpec};
use lope_engine::models::EscalationRequired;
use lope_engine::pool_validate::pool_validate;
use lope_providers::budget::Context;
use lope_providers::invocation::invoke_generate;
use lope_providers::pool::Pool;
use std::collections::{HashMap, HashSet};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub const DEFAULT_TIMEOUT_SECONDS: f64 = 960.0;

pub const VERDICT_INSTRUCTIONS: &str = "\n\nReply with a verdict block in EXACTLY this format. The literal ---VERDICT--- and ---END--- delimiter lines are REQUIRED:\n---VERDICT---\nstatus: PASS | NEEDS_FIX | FAIL\nconfidence: 0.0-1.0\nrationale: 1-3 sentences, terse; cite file:line where relevant\nrequired_fixes:\n  - fix 1   (include this section only when NEEDS_FIX or FAIL)\n---END---";

fn resolve_timeout(node: &FlowNode, ctx: &FlowContext) -> f64 {
    node.timeout()
        .unwrap_or_else(|| ctx.cfg.timeout.as_f64().unwrap_or(DEFAULT_TIMEOUT_SECONDS))
}

fn build_ensemble(node: &FlowNode, ctx: &FlowContext) -> Pool {
    let style = ctx.resolve_style(node);
    let mut validators = ctx.pool.validators.clone();
    let names = split_names(style.get("validators").map(|s| s.as_str()));
    if !names.is_empty() {
        let subset: Vec<_> = validators
            .iter()
            .filter(|v| names.iter().any(|n| n == v.name()))
            .cloned()
            .collect();
        if !subset.is_empty() {
            validators = subset;
        }
    }
    let mut primary = style
        .get("primary")
        .cloned()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            if ctx.pool.primary.is_empty() {
                None
            } else {
                Some(ctx.pool.primary.clone())
            }
        })
        .unwrap_or_default();
    if !primary.is_empty() && !validators.iter().any(|v| v.name() == primary) {
        primary = validators
            .first()
            .map(|v| v.name().to_string())
            .unwrap_or_default();
    }
    Pool {
        validators,
        primary,
        parallel: ctx.cfg.parallel,
        ensemble: true,
        unsupported: Vec::new(),
        warnings: Vec::new(),
    }
}

fn parse_outcome_map(raw: Option<&str>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in raw.unwrap_or("").split(',') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once(':') {
            let key = k.trim().to_lowercase();
            if !key.is_empty() {
                out.insert(key, v.trim().to_string());
            }
        }
    }
    out
}

fn handle_start(node: &FlowNode) -> NodeResult {
    let mut r = NodeResult::simple(&node.id, "started");
    r.label = "start".to_string();
    r
}

fn handle_exit(node: &FlowNode) -> NodeResult {
    let mut r = NodeResult::simple(&node.id, "exited");
    r.label = "exit".to_string();
    r
}

fn handle_agent(node: &FlowNode, ctx: &FlowContext) -> NodeResult {
    let style = ctx.resolve_style(node);
    let Some(validator) = ctx
        .validator_by_name(style.get("primary").map(|s| s.as_str()))
        .or_else(|| ctx.pool.primary_validator())
    else {
        let mut r = NodeResult::simple(&node.id, "infra_error");
        r.error = "no validator available".to_string();
        return r;
    };
    let prompt = ctx.blackboard.render(node.prompt());
    if prompt.trim().is_empty() {
        let mut r = NodeResult::simple(&node.id, "failed");
        r.error = "agent node has an empty prompt".to_string();
        return r;
    }
    let timeout = resolve_timeout(node, ctx);
    let stage = format!("flow:{}", node.id);
    match invoke_generate(
        validator.as_ref(),
        &prompt,
        timeout,
        ctx.invocation_context.as_deref(),
        Some(&stage),
    ) {
        Ok(out) => {
            let trimmed = out.trim().to_string();
            let mut r = NodeResult::simple(
                &node.id,
                if trimmed.is_empty() { "failed" } else { "succeeded" },
            );
            r.label = validator.name().to_string();
            r.detail = trimmed.chars().take(1500).collect();
            r.raw = trimmed;
            r.model_calls = 1;
            r
        }
        Err(e) => {
            let mut r = NodeResult::simple(&node.id, "failed");
            r.label = validator.name().to_string();
            r.error = e.to_string().chars().take(300).collect();
            r.model_calls = 1;
            r
        }
    }
}

fn handle_review(node: &FlowNode, ctx: &FlowContext) -> NodeResult {
    let ensemble = build_ensemble(node, ctx);
    let rendered = ctx.blackboard.render(node.prompt());
    let user_prompt = if rendered.trim().is_empty() {
        format!(
            "Review the current implementation for the goal: {}. Check correctness and completeness against the plan.",
            ctx.blackboard.get("goal")
        )
    } else {
        rendered
    };
    let timeout = resolve_timeout(node, ctx);
    let final_prompt = format!("{user_prompt}{VERDICT_INSTRUCTIONS}");
    let stage = format!("flow:{}", node.id);
    let res = pool_validate(
        &ensemble,
        &final_prompt,
        timeout,
        ctx.invocation_context.as_deref(),
        &stage,
    );
    let mut r = NodeResult::simple(&node.id, verdict_to_outcome(res.verdict.status));
    r.label = "ensemble".to_string();
    r.detail = res.verdict.rationale.chars().take(500).collect();
    r.raw = if res.raw_response.is_empty() {
        res.verdict.rationale.clone()
    } else {
        res.raw_response.clone()
    };
    r.verdict = Some(res.verdict);
    r.error = res.error;
    r.model_calls = ensemble.validators.len().max(1);
    r
}

fn handle_judge(node: &FlowNode, ctx: &FlowContext) -> NodeResult {
    let mode = node
        .attr("mode")
        .unwrap_or("ensemble")
        .trim()
        .to_lowercase();
    let outcome_map = parse_outcome_map(node.attr("outcome_map"));
    let prompt = ctx.blackboard.render(node.prompt());
    let timeout = resolve_timeout(node, ctx);
    let stage = format!("flow:{}", node.id);

    if mode == "generate" {
        let mut allowed = split_names(node.attr("outcomes"));
        if allowed.is_empty() {
            allowed = vec!["succeeded".to_string(), "failed".to_string()];
        }
        let style = ctx.resolve_style(node);
        let Some(validator) = ctx
            .validator_by_name(style.get("primary").map(|s| s.as_str()))
            .or_else(|| ctx.pool.primary_validator())
        else {
            let mut r = NodeResult::simple(&node.id, "infra_error");
            r.error = "no validator available".to_string();
            return r;
        };
        let final_prompt = format!("{prompt}{}", outcome_instructions(&allowed));
        match invoke_generate(
            validator.as_ref(),
            &final_prompt,
            timeout,
            ctx.invocation_context.as_deref(),
            Some(&stage),
        ) {
            Ok(out) => {
                let (token, note) = parse_outcome_block(&out, &allowed);
                let mut r = NodeResult::simple(&node.id, token.clone());
                r.label = validator.name().to_string();
                r.detail = note;
                r.raw = out.chars().take(1500).collect();
                if token == "infra_error" {
                    r.error = "judge produced no recognized outcome".to_string();
                }
                r.model_calls = 1;
                r
            }
            Err(e) => {
                let mut r = NodeResult::simple(&node.id, "infra_error");
                r.label = validator.name().to_string();
                r.error = e.to_string().chars().take(300).collect();
                r.model_calls = 1;
                r
            }
        }
    } else {
        let ensemble = build_ensemble(node, ctx);
        let base_prompt = if prompt.trim().is_empty() {
            "Assess the work so far."
        } else {
            prompt.as_str()
        };
        let review_prompt = format!("{base_prompt}{VERDICT_INSTRUCTIONS}");
        let res = pool_validate(
            &ensemble,
            &review_prompt,
            timeout,
            ctx.invocation_context.as_deref(),
            &stage,
        );
        let base_outcome = verdict_to_outcome(res.verdict.status);
        let mapped = outcome_map
            .get(base_outcome)
            .cloned()
            .unwrap_or_else(|| base_outcome.to_string());
        let mut r = NodeResult::simple(&node.id, mapped);
        r.label = "ensemble".to_string();
        r.detail = res.verdict.rationale.chars().take(500).collect();
        r.verdict = Some(res.verdict);
        r.raw = res.raw_response;
        r.error = res.error;
        r.model_calls = ensemble.validators.len().max(1);
        r
    }
}

fn handle_script(node: &FlowNode, ctx: &FlowContext) -> NodeResult {
    let timeout = resolve_timeout(node, ctx) as u64;
    let spec = if let Some(cmd) = node.attr("cmd") {
        GateSpec {
            name: node.id.clone(),
            cmd: cmd.to_string(),
            gate_type: node.attr("type").unwrap_or("exit").to_string(),
            required: true,
            path: node.attr("path").map(|s| s.to_string()),
            regex: node.attr("regex").map(|s| s.to_string()),
            min_value: None,
            max_value: None,
            min_delta: None,
            max_delta_drop: None,
            timeout: Some(timeout.max(1) as i64),
        }
    } else if let Some(gate_name) = node.attr("gate") {
        let cfg_path = node.attr("gate_config");
        match load_gate_specs(cfg_path, Some(&ctx.cwd)) {
            Ok((specs, _)) => match specs.into_iter().find(|s| s.name == gate_name) {
                Some(s) => s,
                None => {
                    let mut r = NodeResult::simple(&node.id, "infra_error");
                    r.error = format!("gate {gate_name:?} not found in .lope/rules.json");
                    return r;
                }
            },
            Err(e) => {
                let mut r = NodeResult::simple(&node.id, "infra_error");
                r.error = e.to_string();
                return r;
            }
        }
    } else {
        let mut r = NodeResult::simple(&node.id, "infra_error");
        r.error = "script node needs cmd= or gate=".to_string();
        return r;
    };

    let res = run_gate(
        &spec,
        &ctx.cwd,
        timeout.max(1) as i64,
        ctx.invocation_context.as_deref(),
    );
    let mut r = NodeResult::simple(&node.id, if res.ok { "succeeded" } else { "failed" });
    r.label = "gate".to_string();
    let detail_src = if res.stdout_tail.is_empty() {
        res.error.as_deref().unwrap_or("")
    } else {
        res.stdout_tail.as_str()
    };
    r.detail = detail_src.chars().take(500).collect();
    r.raw = res.stdout_tail;
    r
}

fn handle_gate(node: &FlowNode, ctx: &FlowContext) -> NodeResult {
    let rendered = ctx.blackboard.render(node.prompt());
    let prompt = if rendered.trim().is_empty() {
        "Approve to continue?"
    } else {
        rendered.as_str()
    };
    println!("\n[human gate] {prompt}");
    print!("approve? [y/N] ");
    let _ = io::stdout().flush();
    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => {
            let mut r = NodeResult::simple(&node.id, "failed");
            r.detail = "no interactive stdin for human gate".to_string();
            r
        }
        Ok(_) => {
            let ans = line.trim().to_lowercase();
            let mut r = NodeResult::simple(
                &node.id,
                if matches!(ans.as_str(), "y" | "yes" | "approve") {
                    "succeeded"
                } else {
                    "failed"
                },
            );
            r.label = "human".to_string();
            r
        }
    }
}

fn dispatch_handler(node: &FlowNode, ctx: &FlowContext) -> NodeResult {
    match node.kind {
        NodeKind::Start => handle_start(node),
        NodeKind::Exit => handle_exit(node),
        NodeKind::Agent => handle_agent(node, ctx),
        NodeKind::Review => handle_review(node, ctx),
        NodeKind::Judge => handle_judge(node, ctx),
        NodeKind::Script => handle_script(node, ctx),
        NodeKind::Gate => handle_gate(node, ctx),
    }
}

pub struct FlowRunner {
    pub graph: FlowGraph,
    pub pool: Pool,
    pub cfg: LopeCfg,
    pub cwd: PathBuf,
    pub max_node_visits: usize,
    pub max_model_calls: usize,
    pub stylesheet: Option<Stylesheet>,
    pub invocation_context: Option<Arc<Context>>,
}

impl FlowRunner {
    pub fn new(
        graph: FlowGraph,
        pool: Pool,
        cfg: LopeCfg,
        cwd: Option<PathBuf>,
        invocation_context: Option<Arc<Context>>,
    ) -> Result<Self, FlowConfigError> {
        let max_node_visits = graph.max_node_visits()?;
        let graph_calls = graph.max_model_calls()?;
        let configured_calls = cfg.max_calls.as_u64().unwrap_or(96).max(1) as usize;
        let max_model_calls = match graph_calls {
            Some(g) => configured_calls.min(g),
            None => configured_calls,
        };
        let stylesheet = parse_stylesheet(graph.stylesheet_text());
        Ok(Self {
            graph,
            pool,
            cfg,
            cwd: cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
            max_node_visits,
            max_model_calls,
            stylesheet,
            invocation_context,
        })
    }

    pub fn run(&self, task: &str) -> Result<FlowReport, FlowConfigError> {
        let _warnings = validate_graph(&self.graph)?;
        let goal = if task.is_empty() {
            self.graph
                .graph_attrs
                .get("goal")
                .cloned()
                .unwrap_or_default()
        } else {
            task.to_string()
        };

        let mut init = HashMap::new();
        init.insert("task".to_string(), task.to_string());
        init.insert("goal".to_string(), goal.clone());
        let artifact_dir = self.cwd.join("lope-runs").join("flow-blackboard");
        let mut bb = Blackboard::new(Some(init), Some(artifact_dir));
        if !goal.contains('$') {
            bb.set("goal", &goal);
        }

        let mut ctx = FlowContext {
            blackboard: bb,
            pool: self.pool.clone(),
            cfg: self.cfg.clone(),
            cwd: self.cwd.clone(),
            stylesheet: self.stylesheet.clone(),
            invocation_context: self.invocation_context.clone(),
        };

        let validator_count = self.pool.validators.len().max(1);
        let mut report = FlowReport::new(
            &self.graph.name,
            self.max_node_visits * validator_count,
        );

        let start = self.graph.start_node()?.clone();
        let mut ready: Vec<FlowNode> = vec![start.clone()];
        let mut visit_counts: HashMap<String, usize> = HashMap::new();
        let mut pending_joins: HashMap<String, HashSet<String>> = HashMap::new();
        let mut global_visits = 0usize;
        let mut terminated = false;
        let t0 = Instant::now();

        while !ready.is_empty() && !terminated {
            let wave = Self::dedupe(ready);
            ready = Vec::new();
            let mut runnable = Vec::new();
            for node in wave {
                global_visits += 1;
                if global_visits > self.max_node_visits {
                    report.escalation = Some(Self::escalation(
                        &node,
                        report.node_results.len(),
                        format!(
                            "global max_node_visits ({}) exceeded",
                            self.max_node_visits
                        ),
                        None,
                    ));
                    report.ok = false;
                    terminated = true;
                    break;
                }
                let count = visit_counts.entry(node.id.clone()).or_insert(0);
                *count += 1;
                let node_max = node.max_visits()?;
                if *count > node_max {
                    report.escalation = Some(Self::escalation(
                        &node,
                        report.node_results.len(),
                        format!("node {:?} exceeded max_visits={node_max}", node.id),
                        None,
                    ));
                    report.ok = false;
                    terminated = true;
                    break;
                }
                runnable.push(node);
            }
            if terminated {
                break;
            }

            let wave_results = self.run_wave(&runnable, &ctx);
            for (node, result) in &wave_results {
                report.model_calls += result.model_calls;
                ctx.blackboard.put_result(result);
                global_visits += result.attempts.saturating_sub(1);
                report.node_results.push(result.clone());
                let _ = node;
            }

            if report.model_calls > self.max_model_calls {
                let last_node = runnable.last().unwrap_or(&start);
                report.escalation = Some(Self::escalation(
                    last_node,
                    report.node_results.len(),
                    format!(
                        "max_model_calls ({}) exceeded: actual={}",
                        self.max_model_calls, report.model_calls
                    ),
                    None,
                ));
                report.ok = false;
                break;
            }

            let mut next_ready = Vec::new();
            for (node, result) in wave_results {
                if node.kind == NodeKind::Exit {
                    report.ok = node.status() != "fail";
                    terminated = true;
                    break;
                }
                if result.outcome == "infra_error" {
                    if let Some(target_id) = node.retry_target() {
                        next_ready.push(self.graph.node(target_id)?.clone());
                        continue;
                    }
                    let msg = if result.error.is_empty() {
                        result.detail.clone()
                    } else {
                        result.error.clone()
                    };
                    report.escalation = Some(Self::escalation(
                        &node,
                        report.node_results.len(),
                        format!("infra_error: {msg}"),
                        result.verdict.clone(),
                    ));
                    report.ok = false;
                    terminated = true;
                    break;
                }

                let chosen: Vec<&FlowEdge> = self
                    .graph
                    .edges_from(&node.id)
                    .into_iter()
                    .filter(|e| edge_matches(e, &result))
                    .collect();
                if chosen.is_empty() {
                    report.escalation = Some(Self::escalation(
                        &node,
                        report.node_results.len(),
                        format!(
                            "dead-end at {:?} (outcome={:?}, no matching out-edge)",
                            node.id, result.outcome
                        ),
                        result.verdict.clone(),
                    ));
                    report.ok = false;
                    terminated = true;
                    break;
                }
                for edge in chosen {
                    self.enqueue(edge, &node, &mut next_ready, &mut pending_joins)?;
                }
            }
            ready = next_ready;
        }

        if !terminated && ready.is_empty() && report.ok && report.escalation.is_none() {
            report.ok = false;
            report.escalation = Some(Self::escalation(
                &start,
                report.node_results.len(),
                "frontier drained without reaching an exit node".to_string(),
                None,
            ));
        }

        report.total_duration_seconds = t0.elapsed().as_secs_f64();
        report.blackboard_snapshot = ctx.blackboard.snapshot();
        Ok(report)
    }

    fn enqueue(
        &self,
        edge: &FlowEdge,
        source_node: &FlowNode,
        next_ready: &mut Vec<FlowNode>,
        pending_joins: &mut HashMap<String, HashSet<String>>,
    ) -> Result<(), FlowConfigError> {
        let target = self.graph.node(&edge.target)?;
        if edge.loop_restart {
            pending_joins.remove(&target.id);
            next_ready.push(target.clone());
            return Ok(());
        }
        if self.graph.is_join(&target.id) {
            let entry = pending_joins
                .entry(target.id.clone())
                .or_insert_with(|| self.graph.barrier_sources(&target.id));
            entry.remove(&source_node.id);
            if entry.is_empty() {
                pending_joins.remove(&target.id);
                next_ready.push(target.clone());
            }
            return Ok(());
        }
        next_ready.push(target.clone());
        Ok(())
    }

    fn run_wave(&self, nodes: &[FlowNode], ctx: &FlowContext) -> Vec<(FlowNode, NodeResult)> {
        if nodes.is_empty() {
            return Vec::new();
        }
        if nodes.len() == 1 {
            return vec![(nodes[0].clone(), self.run_node_guarded(&nodes[0], ctx))];
        }
        std::thread::scope(|s| {
            let handles: Vec<_> = nodes
                .iter()
                .map(|n| s.spawn(move || (n.clone(), self.run_node_guarded(n, ctx))))
                .collect();
            handles
                .into_iter()
                .enumerate()
                .map(|(i, h)| match h.join() {
                    Ok(pair) => pair,
                    Err(_) => {
                        let mut r = NodeResult::simple(&nodes[i].id, "infra_error");
                        r.error = "worker thread panicked".to_string();
                        (nodes[i].clone(), r)
                    }
                })
                .collect()
        })
    }

    fn run_node_guarded(&self, node: &FlowNode, ctx: &FlowContext) -> NodeResult {
        let attempts = node.retry() + 1;
        let t0 = Instant::now();
        let mut last = NodeResult::simple(&node.id, "infra_error");
        let mut made = 0usize;
        for _ in 0..attempts {
            made += 1;
            last = dispatch_handler(node, ctx);
            if last.outcome != "infra_error" {
                break;
            }
        }
        last.duration_seconds = t0.elapsed().as_secs_f64();
        last.attempts = made;
        last
    }

    fn escalation(
        node: &FlowNode,
        index: usize,
        reason: String,
        verdict: Option<lope_engine::models::PhaseVerdict>,
    ) -> EscalationRequired {
        EscalationRequired {
            reason,
            phase_index: index,
            phase_name: node.id.clone(),
            rounds: Vec::new(),
            last_verdict: verdict,
        }
    }

    fn dedupe(nodes: Vec<FlowNode>) -> Vec<FlowNode> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for n in nodes {
            if seen.insert(n.id.clone()) {
                out.push(n);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dot::parse_dot;
    use crate::templates::{get_template, template_names};

    #[test]
    fn bundled_templates_parse_and_validate() {
        for name in template_names() {
            let src = get_template(name).unwrap();
            let g = parse_dot(src, None).expect("parse template");
            let warnings = validate_graph(&g).expect("validate template");
            assert!(warnings.is_empty(), "{name}: {warnings:?}");
        }
    }

    #[test]
    fn fanout_join_and_script_gate_run_end_to_end() {
        let dot = r#"
        digraph demo {
          Start [ type="start" ]
          A [ type="script", cmd="echo alpha" ]
          B [ type="script", cmd="echo beta" ]
          Join [ type="script", join="true", cmd="echo joined" ]
          Exit [ type="exit" ]
          Start -> A
          Start -> B
          A -> Join
          B -> Join
          Join -> Exit
        }
        "#;
        let g = parse_dot(dot, None).unwrap();
        let pool = Pool {
            validators: Vec::new(),
            primary: String::new(),
            parallel: false,
            ensemble: false,
            unsupported: Vec::new(),
            warnings: Vec::new(),
        };
        let runner = FlowRunner::new(g, pool, LopeCfg::defaults(), None, None).unwrap();
        let report = runner.run("demo").unwrap();
        assert!(report.ok);
        assert_eq!(report.path(), vec!["Start", "A", "B", "Join", "Exit"]);
    }

    #[test]
    fn unbounded_loop_is_rejected_and_at_file_escape_is_blocked() {
        let bad_loop = r#"
        digraph loop_bad {
          Start [ type="start" ]
          Step [ type="script", cmd="echo hi" ]
          Exit [ type="exit" ]
          Start -> Step
          Step -> Step
          Step -> Exit [ condition="outcome=never" ]
        }
        "#;
        let g = parse_dot(bad_loop, None).unwrap();
        assert!(validate_graph(&g).is_err());

        let dir = tempfile::tempdir().unwrap();
        let escape = r#"
        digraph esc {
          Start [ type="start" ]
          A [ type="agent", prompt="@../../etc/passwd" ]
          Exit [ type="exit" ]
          Start -> A -> Exit
        }
        "#;
        assert!(parse_dot(escape, Some(dir.path())).is_err());
    }
}
