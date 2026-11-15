//! Declarative DOT flow graphs (`lope/flow/`, Sprint S7, `FR36`).

pub mod dot;
pub mod model;
pub mod render;
pub mod report;
pub mod runner;
pub mod stylesheet;
pub mod templates;
pub mod validate;

pub use dot::{load_flow_graph, parse_dot};
pub use model::{
    edge_matches, normalize_kind, outcome_instructions, parse_outcome_block, verdict_to_outcome,
    Blackboard, FlowConfigError, FlowContext, FlowEdge, FlowGraph, FlowNode, NodeKind,
    NodeResult,
};
pub use render::{has_dot, render_file, to_canonical_dot};
pub use report::{flow_report_to_execution_report, write_flow_run, FlowReport};
pub use runner::FlowRunner;
pub use stylesheet::{parse_stylesheet, split_names, StyleRule, Stylesheet};
pub use templates::{get_template, template_names};
pub use validate::{forecast_graph, validate_graph};
