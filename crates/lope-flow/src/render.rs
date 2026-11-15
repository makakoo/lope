//! Graphviz DOT rendering (`lope/flow/render.py`, Sprint S7, `FR36`).

use crate::dot::load_flow_graph;
use crate::model::{FlowConfigError, FlowGraph, NodeKind};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn kind_shape(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Start => "Mdiamond",
        NodeKind::Exit => "Msquare",
        NodeKind::Agent => "box",
        NodeKind::Review => "doubleoctagon",
        NodeKind::Judge => "box",
        NodeKind::Script => "parallelogram",
        NodeKind::Gate => "hexagon",
    }
}

pub fn to_canonical_dot(graph: &FlowGraph) -> String {
    let mut lines = vec![format!("digraph {} {{", graph.name)];
    let rankdir = graph
        .graph_attrs
        .get("rankdir")
        .map(|s| s.as_str())
        .unwrap_or("LR");
    lines.push(format!("  rankdir=\"{rankdir}\";"));
    lines.push(
        "  node [style=\"rounded,filled\", fillcolor=\"#f5f5f5\", fontname=\"Helvetica\"];"
            .to_string(),
    );
    for node in graph.ordered_nodes() {
        let shape = node.attr("shape").unwrap_or_else(|| kind_shape(node.kind));
        lines.push(format!(
            "  {} [shape=\"{}\", label=\"{}\"];",
            node.id, shape, node.id
        ));
    }
    for edge in &graph.edges {
        let mut attrs = Vec::new();
        let text = if !edge.label.is_empty() {
            edge.label.as_str()
        } else {
            edge.condition.as_deref().unwrap_or("")
        };
        if !text.is_empty() {
            let escaped = text.replace('"', "\\\"").replace('\n', " ");
            attrs.push(format!("label=\"{escaped}\""));
        }
        if edge.loop_restart {
            attrs.push("style=\"dashed\"".to_string());
        }
        let suffix = if attrs.is_empty() {
            String::new()
        } else {
            format!(" [{}]", attrs.join(", "))
        };
        lines.push(format!("  {} -> {}{};", edge.source, edge.target, suffix));
    }
    lines.push("}".to_string());
    lines.join("\n")
}

pub fn has_dot() -> bool {
    Command::new("which")
        .arg("dot")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn render_file(path: &Path, out_path: Option<&Path>, fmt: &str) -> Result<String, FlowConfigError> {
    let graph = load_flow_graph(path)?;
    let canonical = to_canonical_dot(&graph);
    if fmt == "dot" && out_path.is_none() {
        return Ok(canonical);
    }
    if !has_dot() {
        return Ok(
            "Graphviz `dot` not found — the workflow is already valid DOT.\n\
Install it (`brew install graphviz`) to render, or paste the file into https://dreampuf.github.io/GraphvizOnline.\n\
Tip: `lope flow render <file> -T dot` prints normalized DOT to stdout."
                .to_string(),
        );
    }
    let out = match out_path {
        Some(p) => p.to_path_buf(),
        None => path.with_extension(fmt),
    };
    let mut child = Command::new("dot")
        .arg(format!("-T{fmt}"))
        .arg("-o")
        .arg(&out)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| FlowConfigError(format!("failed to launch dot: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(canonical.as_bytes());
    }
    let output = child
        .wait_with_output()
        .map_err(|e| FlowConfigError(format!("dot wait failed: {e}")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Ok(format!(
            "dot failed (exit {:?}): {}",
            output.status.code(),
            err.trim().chars().take(300).collect::<String>()
        ));
    }
    Ok(format!("Rendered {}", out.display()))
}
