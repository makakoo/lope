//! Flow data model (`lope/flow/model.py`, Sprint S7, `FR36`).

use crate::stylesheet::Stylesheet;
use lope_core::config::LopeCfg;
use lope_engine::models::{PhaseVerdict, VerdictStatus};
use lope_providers::budget::Context;
use lope_providers::generic::Validator;
use lope_providers::pool::Pool;
use regex::Regex;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

pub const MAX_RETRIES: usize = 3;
pub const DEFAULT_INLINE_LIMIT: usize = 512 * 1024;
pub const DEFAULT_TOTAL_INLINE_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowConfigError(pub String);

impl std::fmt::Display for FlowConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for FlowConfigError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Start,
    Agent,
    Review,
    Judge,
    Script,
    Gate,
    Exit,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Start => "start",
            NodeKind::Agent => "agent",
            NodeKind::Review => "review",
            NodeKind::Judge => "judge",
            NodeKind::Script => "script",
            NodeKind::Gate => "gate",
            NodeKind::Exit => "exit",
        }
    }
}

pub fn truthy(val: Option<&str>) -> bool {
    val.map(|v| {
        matches!(
            v.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
    .unwrap_or(false)
}

pub fn normalize_kind(raw_type: &str, shape: &str) -> Result<NodeKind, FlowConfigError> {
    let mut key = raw_type.trim().to_lowercase();
    if key.is_empty() && !shape.is_empty() {
        key = match shape.trim().to_lowercase().as_str() {
            "mdiamond" => "start",
            "msquare" => "exit",
            "parallelogram" => "script",
            "hexagon" => "gate",
            _ => "",
        }
        .to_string();
    }
    let aliased = match key.as_str() {
        "implement" => "agent",
        "ensemble" => "review",
        "consensus" | "steer" => "judge",
        "verify" => "script",
        "human" | "approve" => "gate",
        other => other,
    };
    match aliased {
        "" => Err(FlowConfigError(
            "node is missing a type (expected one of: start, agent, review, judge, script, gate, exit)".to_string(),
        )),
        "start" => Ok(NodeKind::Start),
        "agent" => Ok(NodeKind::Agent),
        "review" => Ok(NodeKind::Review),
        "judge" => Ok(NodeKind::Judge),
        "script" => Ok(NodeKind::Script),
        "gate" => Ok(NodeKind::Gate),
        "exit" => Ok(NodeKind::Exit),
        _ => Err(FlowConfigError(format!(
            "unknown node type {raw_type:?} (valid: start, agent, review, judge, script, gate, exit)"
        ))),
    }
}

#[derive(Debug, Clone)]
pub struct FlowNode {
    pub id: String,
    pub kind: NodeKind,
    pub attrs: HashMap<String, String>,
    pub line: usize,
}

impl FlowNode {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(|s| s.as_str())
    }

    pub fn int_attr(&self, key: &str, default: i64) -> Result<i64, FlowConfigError> {
        match self.attrs.get(key) {
            None => Ok(default),
            Some(raw) if raw.trim().is_empty() => Ok(default),
            Some(raw) => raw.trim().parse::<i64>().map_err(|_| {
                FlowConfigError(format!(
                    "node {:?}: attribute {:?} must be an integer, got {:?}",
                    self.id, key, raw
                ))
            }),
        }
    }

    pub fn prompt(&self) -> &str {
        self.attrs.get("prompt").map(|s| s.as_str()).unwrap_or("")
    }

    pub fn node_class(&self) -> Option<&str> {
        self.attrs.get("class").map(|s| s.as_str())
    }

    pub fn max_visits(&self) -> Result<usize, FlowConfigError> {
        Ok(self.int_attr("max_visits", 3)?.max(1) as usize)
    }

    pub fn timeout(&self) -> Option<f64> {
        self.attrs
            .get("timeout")
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| s.trim().parse::<f64>().ok())
    }

    pub fn retry(&self) -> usize {
        self.int_attr("retry", 0)
            .unwrap_or(0)
            .clamp(0, MAX_RETRIES as i64) as usize
    }

    pub fn retry_target(&self) -> Option<&str> {
        self.attrs.get("retry_target").map(|s| s.as_str())
    }

    pub fn explicit_join(&self) -> Option<bool> {
        self.attrs.get("join").map(|s| truthy(Some(s.as_str())))
    }

    pub fn status(&self) -> &str {
        self.attrs.get("status").map(|s| s.as_str()).unwrap_or("pass")
    }
}

#[derive(Debug, Clone)]
pub struct FlowEdge {
    pub source: String,
    pub target: String,
    pub condition: Option<String>,
    pub label: String,
    pub loop_restart: bool,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct FlowGraph {
    pub name: String,
    /// Preserves insertion order for deterministic rendering/iteration.
    pub node_order: Vec<String>,
    pub nodes: HashMap<String, FlowNode>,
    pub edges: Vec<FlowEdge>,
    pub graph_attrs: HashMap<String, String>,
}

impl FlowGraph {
    pub fn ordered_nodes(&self) -> Vec<&FlowNode> {
        self.node_order
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .collect()
    }

    pub fn node(&self, node_id: &str) -> Result<&FlowNode, FlowConfigError> {
        self.nodes
            .get(node_id)
            .ok_or_else(|| FlowConfigError(format!("reference to undefined node {node_id:?}")))
    }

    pub fn edges_from(&self, node_id: &str) -> Vec<&FlowEdge> {
        self.edges.iter().filter(|e| e.source == node_id).collect()
    }

    pub fn in_edges(&self, node_id: &str) -> Vec<&FlowEdge> {
        self.edges.iter().filter(|e| e.target == node_id).collect()
    }

    pub fn barrier_sources(&self, node_id: &str) -> HashSet<String> {
        self.in_edges(node_id)
            .into_iter()
            .filter(|e| !e.loop_restart)
            .map(|e| e.source.clone())
            .collect()
    }

    pub fn is_join(&self, node_id: &str) -> bool {
        if let Some(explicit) = self.nodes.get(node_id).and_then(|n| n.explicit_join()) {
            return explicit;
        }
        self.barrier_sources(node_id).len() >= 2
    }

    pub fn start_node(&self) -> Result<&FlowNode, FlowConfigError> {
        let starts: Vec<&FlowNode> = self
            .ordered_nodes()
            .into_iter()
            .filter(|n| n.kind == NodeKind::Start)
            .collect();
        match starts.len() {
            0 => Err(FlowConfigError("graph has no start node (type=\"start\")".into())),
            1 => Ok(starts[0]),
            n => Err(FlowConfigError(format!(
                "graph has {n} start nodes; exactly one is required"
            ))),
        }
    }

    pub fn exits(&self) -> Vec<&FlowNode> {
        self.ordered_nodes()
            .into_iter()
            .filter(|n| n.kind == NodeKind::Exit)
            .collect()
    }

    pub fn max_node_visits(&self) -> Result<usize, FlowConfigError> {
        if let Some(raw) = self.graph_attrs.get("max_node_visits").filter(|s| !s.is_empty()) {
            let v: i64 = raw.trim().parse().map_err(|_| {
                FlowConfigError(format!("graph max_node_visits must be an integer, got {raw:?}"))
            })?;
            return Ok(v.max(1) as usize);
        }
        Ok((8 * self.nodes.len()).max(50))
    }

    pub fn max_model_calls(&self) -> Result<Option<usize>, FlowConfigError> {
        let Some(raw) = self.graph_attrs.get("max_model_calls").filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let v: i64 = raw.trim().parse().map_err(|_| {
            FlowConfigError(format!("graph max_model_calls must be an integer, got {raw:?}"))
        })?;
        if v <= 0 {
            return Err(FlowConfigError("graph max_model_calls must be positive".into()));
        }
        Ok(Some(v as usize))
    }

    pub fn stylesheet_text(&self) -> &str {
        self.graph_attrs
            .get("cli_stylesheet")
            .or_else(|| self.graph_attrs.get("model_stylesheet"))
            .map(|s| s.as_str())
            .unwrap_or("")
    }
}

#[derive(Debug, Clone)]
pub struct NodeResult {
    pub node_id: String,
    pub outcome: String,
    pub label: String,
    pub detail: String,
    pub verdict: Option<PhaseVerdict>,
    pub duration_seconds: f64,
    pub error: String,
    pub raw: String,
    pub attempts: usize,
    pub model_calls: usize,
}

impl NodeResult {
    pub fn simple(node_id: impl Into<String>, outcome: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            outcome: outcome.into(),
            label: String::new(),
            detail: String::new(),
            verdict: None,
            duration_seconds: 0.0,
            error: String::new(),
            raw: String::new(),
            attempts: 1,
            model_calls: 0,
        }
    }
}

pub fn verdict_to_outcome(status: VerdictStatus) -> &'static str {
    match status {
        VerdictStatus::Pass => "succeeded",
        VerdictStatus::NeedsFix => "needs_fix",
        VerdictStatus::Fail => "failed",
        VerdictStatus::InfraError | VerdictStatus::Inconclusive => "infra_error",
    }
}

pub fn edge_matches(edge: &FlowEdge, result: &NodeResult) -> bool {
    let Some(cond) = edge.condition.as_deref() else {
        return true;
    };
    if cond.trim().is_empty() {
        return true;
    }
    static COND_RE: OnceLock<Regex> = OnceLock::new();
    let re = COND_RE.get_or_init(|| Regex::new(r"^\s*([a-zA-Z_]+)\s*(==|!=|=)\s*(.+?)\s*$").unwrap());
    let Some(caps) = re.captures(cond) else {
        return false;
    };
    let key = caps.get(1).map(|m| m.as_str()).unwrap_or("");
    if key != "outcome" {
        return false;
    }
    let op = caps.get(2).map(|m| m.as_str()).unwrap_or("=");
    let val = caps
        .get(3)
        .map(|m| m.as_str().trim().trim_matches(|c| c == '"' || c == '\''))
        .unwrap_or("")
        .to_lowercase();
    let actual = result.outcome.trim().to_lowercase();
    if op == "!=" {
        actual != val
    } else {
        actual == val
    }
}

pub fn parse_outcome_block(text: &str, allowed: &[String]) -> (String, String) {
    static OUTCOME_RE: OnceLock<Regex> = OnceLock::new();
    static NOTE_RE: OnceLock<Regex> = OnceLock::new();
    let outcome_re = OUTCOME_RE.get_or_init(|| {
        Regex::new(r"(?im)^\s*outcome\s*:\s*([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
    });
    let note_re =
        NOTE_RE.get_or_init(|| Regex::new(r"(?im)^\s*note\s*:\s*(.+?)\s*$").unwrap());

    let allowed_lower: HashMap<String, String> = allowed
        .iter()
        .filter(|a| !a.trim().is_empty())
        .map(|a| (a.trim().to_lowercase(), a.clone()))
        .collect();
    let note = note_re
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();

    for cap in outcome_re.captures_iter(text) {
        let token = cap.get(1).map(|m| m.as_str().trim().to_lowercase()).unwrap_or_default();
        if allowed_lower.is_empty() {
            return (token, note);
        }
        if let Some(orig) = allowed_lower.get(&token) {
            return (orig.clone(), note);
        }
    }
    (
        "infra_error".to_string(),
        if note.is_empty() {
            "no recognized outcome token in judge response".to_string()
        } else {
            note
        },
    )
}

pub fn outcome_instructions(allowed: &[String]) -> String {
    let filtered: Vec<&str> = allowed
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
        .collect();
    let opts = if filtered.is_empty() {
        "succeeded | failed".to_string()
    } else {
        filtered.join(" | ")
    };
    format!(
        "\n\nWhen you have decided, end your reply with EXACTLY this block:\n\
---OUTCOME---\n\
outcome: <{opts}>\n\
note: <one short sentence explaining the choice>\n\
---END---"
    )
}

#[derive(Debug, Clone)]
pub struct Blackboard {
    pub inline_limit: usize,
    pub total_inline_limit: usize,
    pub artifact_dir: Option<PathBuf>,
    data: HashMap<String, String>,
    refs: HashMap<String, serde_json::Value>,
}

impl Blackboard {
    pub fn new(initial: Option<HashMap<String, String>>, artifact_dir: Option<PathBuf>) -> Self {
        let mut bb = Self {
            inline_limit: DEFAULT_INLINE_LIMIT,
            total_inline_limit: DEFAULT_TOTAL_INLINE_LIMIT,
            artifact_dir,
            data: HashMap::new(),
            refs: HashMap::new(),
        };
        if let Some(init) = initial {
            for (k, v) in init {
                bb.set(&k, &v);
            }
        }
        bb
    }

    fn bounded(value: &str, limit: usize) -> String {
        if value.len() <= limit {
            return value.to_string();
        }
        let marker = "\n...[blackboard value bounded; see file reference]...\n";
        if limit <= marker.len() {
            let mut end = limit.min(value.len());
            while !value.is_char_boundary(end) {
                end -= 1;
            }
            return value[..end].to_string();
        }
        let keep = limit - marker.len();
        let half = keep / 2;
        let mut head_end = half.min(value.len());
        while !value.is_char_boundary(head_end) {
            head_end -= 1;
        }
        let tail_len = keep - half;
        let mut tail_start = value.len().saturating_sub(tail_len);
        while !value.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        format!("{}{}{}", &value[..head_end], marker, &value[tail_start..])
    }

    fn write_reference(&self, key: &str, value: &str) -> Option<serde_json::Value> {
        let dir = self.artifact_dir.as_ref()?;
        fs::create_dir_all(dir).ok()?;
        let safe: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let safe = safe.trim_matches('-');
        let safe = if safe.is_empty() { "value" } else { safe };
        let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
        let dest = dir.join(format!("{safe}-{}.txt", &digest[..12]));
        fs::write(&dest, value).ok()?;
        Some(json!({
            "path": dest.to_string_lossy(),
            "sha256": digest,
            "utf8_bytes": value.len().to_string(),
        }))
    }

    pub fn set(&mut self, key: &str, value: &str) {
        let used: usize = self
            .data
            .iter()
            .filter(|(k, _)| k.as_str() != key)
            .map(|(_, v)| v.len())
            .sum();
        let remaining = self.total_inline_limit.saturating_sub(used);
        self.data.insert(
            key.to_string(),
            Self::bounded(value, self.inline_limit.min(remaining)),
        );
    }

    pub fn get(&self, key: &str) -> &str {
        self.data.get(key).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn put_result(&mut self, result: &NodeResult) {
        let nid = &result.node_id;
        self.set(&format!("{nid}.outcome"), &result.outcome);
        self.set(&format!("{nid}.detail"), &result.detail);
        let raw = if result.raw.is_empty() {
            result.detail.as_str()
        } else {
            result.raw.as_str()
        };
        let key = format!("{nid}.out");
        let reference = if raw.len() > self.inline_limit {
            self.write_reference(&key, raw)
        } else {
            None
        };
        if let Some(ref r) = reference {
            self.refs.insert(key.clone(), r.clone());
            self.set(&format!("{nid}.out_ref"), &r.to_string());
            let inline = format!(
                "{}\n[full output: {} sha256={}]",
                Self::bounded(raw, self.inline_limit),
                r["path"].as_str().unwrap_or(""),
                r["sha256"].as_str().unwrap_or("")
            );
            self.set(&key, &inline);
        } else {
            self.set(&key, raw);
        }
        if let Some(v) = &result.verdict {
            self.set(&format!("{nid}.verdict"), v.status.as_str());
            self.set(&format!("{nid}.rationale"), &v.rationale);
        }
    }

    pub fn render(&self, template: &str) -> String {
        if template.is_empty() {
            return String::new();
        }
        static PLACEHOLDER_RE: OnceLock<Regex> = OnceLock::new();
        let re = PLACEHOLDER_RE.get_or_init(|| {
            Regex::new(r"\$([a-zA-Z_][a-zA-Z0-9_]*)|\{([a-zA-Z_][a-zA-Z0-9_.]*)\}").unwrap()
        });
        re.replace_all(template, |caps: &regex::Captures| {
            let key = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|m| m.as_str())
                .unwrap_or("");
            self.data
                .get(key)
                .cloned()
                .unwrap_or_else(|| caps.get(0).unwrap().as_str().to_string())
        })
        .into_owned()
    }

    pub fn snapshot(&self) -> HashMap<String, String> {
        let mut out = self.data.clone();
        if !self.refs.is_empty() {
            if let Ok(s) = serde_json::to_string(&self.refs) {
                out.insert("_references".to_string(), s);
            }
        }
        out
    }
}

pub struct FlowContext {
    pub blackboard: Blackboard,
    pub pool: Pool,
    pub cfg: LopeCfg,
    pub cwd: PathBuf,
    pub stylesheet: Option<Stylesheet>,
    pub invocation_context: Option<Arc<Context>>,
}

impl FlowContext {
    pub fn resolve_style(&self, node: &FlowNode) -> HashMap<String, String> {
        if let Some(sheet) = &self.stylesheet {
            return sheet.resolve(node);
        }
        let mut out = HashMap::new();
        for k in ["primary", "validators", "model", "reasoning_effort", "timeout"] {
            if let Some(v) = node.attrs.get(k) {
                out.insert(k.to_string(), v.clone());
            }
        }
        out
    }

    pub fn validator_by_name(&self, name: Option<&str>) -> Option<Arc<dyn Validator>> {
        let name = name?;
        self.pool
            .validators
            .iter()
            .find(|v| v.name() == name)
            .cloned()
    }
}
