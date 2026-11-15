//! Static validation & forecast for flow graphs (`lope/flow/validate.py`, Sprint S7, `FR36`).

use crate::model::{FlowConfigError, FlowGraph, NodeKind, MAX_RETRIES};
use lope_core::config::LopeCfg;
use lope_providers::generic::Validator;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub fn validate_graph(graph: &FlowGraph) -> Result<Vec<String>, FlowConfigError> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let _ = graph.max_model_calls()?;
    let _ = graph.max_node_visits()?;

    for edge in &graph.edges {
        if !graph.nodes.contains_key(&edge.source) {
            errors.push(format!(
                "edge -> {:?} (line {}): source {:?} is not a defined node",
                edge.target, edge.line, edge.source
            ));
        }
        if !graph.nodes.contains_key(&edge.target) {
            errors.push(format!(
                "edge {:?} -> (line {}): target {:?} is not a defined node",
                edge.source, edge.line, edge.target
            ));
        }
    }
    if !errors.is_empty() {
        return Err(FlowConfigError(format!(
            "invalid flow graph:\n  - {}",
            errors.join("\n  - ")
        )));
    }

    let starts: Vec<_> = graph
        .ordered_nodes()
        .into_iter()
        .filter(|n| n.kind == NodeKind::Start)
        .collect();
    if starts.is_empty() {
        errors.push("no start node (add a node with type=\"start\")".to_string());
    } else if starts.len() > 1 {
        let names: Vec<_> = starts.iter().map(|n| n.id.as_str()).collect();
        errors.push(format!(
            "{} start nodes ({}); exactly one is required",
            starts.len(),
            names.join(", ")
        ));
    }

    let exits = graph.exits();
    if exits.is_empty() {
        errors.push("no exit node (add a node with type=\"exit\")".to_string());
    }

    for node in graph.ordered_nodes() {
        if matches!(node.kind, NodeKind::Agent | NodeKind::Judge) && node.prompt().trim().is_empty()
        {
            errors.push(format!(
                "node {:?} (type={}) needs a prompt=",
                node.id,
                node.kind.as_str()
            ));
        }
        if node.kind == NodeKind::Script && node.attr("cmd").is_none() && node.attr("gate").is_none()
        {
            errors.push(format!("script node {:?} needs cmd= or gate=", node.id));
        }
        let raw_retry = node.int_attr("retry", 0)?;
        if raw_retry > MAX_RETRIES as i64 {
            warnings.push(format!(
                "node {:?}: retry={raw_retry} exceeds the cap and will be clamped to MAX_RETRIES={MAX_RETRIES} (each retry is another model call)",
                node.id
            ));
        }
        let _ = node.max_visits()?;
    }

    if !starts.is_empty() && errors.is_empty() {
        let reachable = reachable_from(graph, &starts[0].id);
        for node in graph.ordered_nodes() {
            if !reachable.contains(&node.id) {
                warnings.push(format!("node {:?} is unreachable from start", node.id));
            }
            if node.kind != NodeKind::Exit && graph.edges_from(&node.id).is_empty() {
                errors.push(format!(
                    "node {:?} is a dead end (no outgoing edge and not an exit)",
                    node.id
                ));
            }
        }
        if !exits.iter().any(|e| reachable.contains(&e.id)) {
            errors.push("no exit node is reachable from start".to_string());
        }
    }

    if errors.is_empty() {
        for cycle in find_cycles(graph) {
            let graph_capped = graph
                .graph_attrs
                .get("max_node_visits")
                .is_some_and(|s| !s.is_empty());
            let node_capped = cycle
                .iter()
                .any(|nid| graph.nodes[nid].attrs.contains_key("max_visits"));
            if !(graph_capped || node_capped) {
                let mut loop_disp = cycle.clone();
                loop_disp.push(cycle[0].clone());
                errors.push(format!(
                    "loop {} has no visit bound; add max_visits to a node on the loop or a graph-level max_node_visits",
                    loop_disp.join(" -> ")
                ));
            }
        }
    }

    if !errors.is_empty() {
        return Err(FlowConfigError(format!(
            "invalid flow graph:\n  - {}",
            errors.join("\n  - ")
        )));
    }
    Ok(warnings)
}

pub fn forecast_graph(
    graph: &FlowGraph,
    validators: &[Arc<dyn Validator>],
    cfg: &LopeCfg,
) -> Value {
    let validator_count = validators.len().max(1);
    let max_node_visits = graph.max_node_visits().unwrap_or(50);
    let theoretical_calls = max_node_visits * validator_count;
    let configured_call_limit = cfg.max_calls.as_u64().unwrap_or(96).max(1) as usize;
    let graph_call_limit = graph.max_model_calls().ok().flatten();
    let effective_call_limit = match graph_call_limit {
        Some(g) => configured_call_limit.min(g),
        None => configured_call_limit,
    };
    let run_timeout = cfg.run_timeout.as_ref().and_then(|v| v.as_f64()).or_else(|| {
        if cfg.allow_unbounded_run {
            None
        } else {
            Some(lope_providers::budget::mode_run_timeout("flow"))
        }
    });
    let per_call_timeout = cfg.timeout.as_f64().unwrap_or(960.0);
    let theoretical_wall = max_node_visits as f64 * per_call_timeout;

    let mut warnings = Vec::new();
    if theoretical_calls > effective_call_limit {
        warnings.push(format!(
            "theoretical graph path ceiling {theoretical_calls} model calls exceeds effective max_model_calls {effective_call_limit}"
        ));
    }
    if let Some(rt) = run_timeout {
        if theoretical_wall > rt {
            warnings.push(format!(
                "theoretical graph path wall ceiling {theoretical_wall}s exceeds run deadline {rt}s"
            ));
        }
    }

    json!({
        "schema_version": 1,
        "validator_count": validator_count,
        "max_node_visits": max_node_visits,
        "theoretical_model_calls": theoretical_calls,
        "max_model_calls": effective_call_limit,
        "static_prompt_calls": 0,
        "static_prompt_chunks": 0,
        "run_timeout_seconds": run_timeout,
        "theoretical_wall_seconds": theoretical_wall,
        "rejected_nodes": [],
        "warnings": warnings,
    })
}

fn reachable_from(graph: &FlowGraph, start_id: &str) -> HashSet<String> {
    let mut seen = HashSet::new();
    let mut stack = vec![start_id.to_string()];
    while let Some(nid) = stack.pop() {
        if !seen.insert(nid.clone()) {
            continue;
        }
        for edge in graph.edges_from(&nid) {
            stack.push(edge.target.clone());
        }
    }
    seen
}

fn find_cycles(graph: &FlowGraph) -> Vec<Vec<String>> {
    let mut cycles = Vec::new();
    let mut color: HashMap<String, u8> = HashMap::new();
    let mut stack = Vec::new();

    fn dfs(
        graph: &FlowGraph,
        nid: &str,
        color: &mut HashMap<String, u8>,
        stack: &mut Vec<String>,
        cycles: &mut Vec<Vec<String>>,
    ) {
        color.insert(nid.to_string(), 1);
        stack.push(nid.to_string());
        for edge in graph.edges_from(nid) {
            let c = *color.get(&edge.target).unwrap_or(&0);
            if c == 0 {
                dfs(graph, &edge.target, color, stack, cycles);
            } else if c == 1 {
                if let Some(idx) = stack.iter().position(|s| s == &edge.target) {
                    cycles.push(stack[idx..].to_vec());
                }
            }
        }
        stack.pop();
        color.insert(nid.to_string(), 2);
    }

    for nid in &graph.node_order {
        if *color.get(nid).unwrap_or(&0) == 0 {
            dfs(graph, nid, &mut color, &mut stack, &mut cycles);
        }
    }
    cycles
}
