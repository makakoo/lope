//! FlowReport + bridge to Auditor (`lope/flow/report.py`, Sprint S7, `FR36`).

use crate::model::NodeResult;
use lope_core::redaction::redact_text;
use lope_engine::models::{
    EscalationRequired, ExecutionReport, Phase, PhaseVerdict, SprintDoc, VerdictStatus,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FlowReport {
    pub graph_name: String,
    pub node_results: Vec<NodeResult>,
    pub ok: bool,
    pub escalation: Option<EscalationRequired>,
    pub total_duration_seconds: f64,
    pub blackboard_snapshot: HashMap<String, String>,
    pub model_calls: usize,
    pub model_call_forecast: usize,
}

impl FlowReport {
    pub fn new(graph_name: impl Into<String>, model_call_forecast: usize) -> Self {
        Self {
            graph_name: graph_name.into(),
            node_results: Vec::new(),
            ok: true,
            escalation: None,
            total_duration_seconds: 0.0,
            blackboard_snapshot: HashMap::new(),
            model_calls: 0,
            model_call_forecast,
        }
    }

    pub fn path(&self) -> Vec<&str> {
        self.node_results
            .iter()
            .map(|r| r.node_id.as_str())
            .collect()
    }

    pub fn scorecard(&self) -> String {
        let mut lines = vec![
            format!("Flow: {}", self.graph_name),
            format!(
                "Steps: {}  ·  path: {}",
                self.node_results.len(),
                self.path().join(" -> ")
            ),
            format!("Total duration: {:.1}s", self.total_duration_seconds),
            format!(
                "Model calls: {}/{}",
                self.model_calls,
                if self.model_call_forecast == 0 {
                    self.model_calls
                } else {
                    self.model_call_forecast
                }
            ),
            "---".to_string(),
        ];
        for r in &self.node_results {
            let mut tag = format!("{}: {}", r.node_id, r.outcome);
            if !r.label.is_empty() {
                tag.push_str(&format!(" ({})", r.label));
            }
            tag.push_str(&format!(" {:.0}s", r.duration_seconds));
            lines.push(tag);
            if !r.error.is_empty() {
                let head: String = r.error.chars().take(160).collect();
                lines.push(format!("  error: {head}"));
            }
        }
        lines.push("---".to_string());
        let overall = if self.ok {
            "OK"
        } else if self.escalation.is_some() {
            "ESCALATED"
        } else {
            "FAILED (reached a fail-exit)"
        };
        lines.push(format!("Overall: {overall}"));
        if let Some(esc) = &self.escalation {
            lines.push(format!("Escalation: {}", esc.reason));
        }
        lines.join("\n")
    }

    pub fn to_json(&self) -> Value {
        json!({
            "graph_name": self.graph_name,
            "ok": self.ok,
            "path": self.path(),
            "total_duration_seconds": self.total_duration_seconds,
            "model_calls": self.model_calls,
            "model_call_forecast": self.model_call_forecast,
            "escalation": self.escalation.as_ref().map(|e| e.reason.clone()).unwrap_or_default(),
            "node_results": self.node_results.iter().map(|r| json!({
                "node_id": r.node_id,
                "outcome": r.outcome,
                "label": r.label,
                "detail": r.detail.chars().take(500).collect::<String>(),
                "duration_seconds": r.duration_seconds,
                "error": r.error.chars().take(300).collect::<String>(),
                "model_calls": r.model_calls,
            })).collect::<Vec<_>>(),
        })
    }
}

pub fn flow_report_to_execution_report(fr: &FlowReport) -> ExecutionReport {
    let mut phases = Vec::new();
    let mut verdicts = Vec::new();
    for (idx, r) in fr.node_results.iter().enumerate() {
        let mut v = match &r.verdict {
            Some(existing) => existing.clone(),
            None => {
                let status = match r.outcome.as_str() {
                    "succeeded" | "passed" | "started" | "exited" | "done" | "ok" => {
                        VerdictStatus::Pass
                    }
                    "needs_fix" => VerdictStatus::NeedsFix,
                    "infra_error" => VerdictStatus::InfraError,
                    _ => VerdictStatus::Fail,
                };
                let mut pv = PhaseVerdict::new(status, &r.detail, r.duration_seconds, &r.label);
                pv.confidence = 0.0;
                pv
            }
        };
        v.duration_seconds = r.duration_seconds;
        phases.push(Phase {
            index: idx + 1,
            name: r.node_id.clone(),
            goal: r.outcome.clone(),
            criteria: Vec::new(),
            artifacts: Vec::new(),
            checks: Vec::new(),
            verdict: Some(v.clone()),
        });
        verdicts.push(v);
    }

    ExecutionReport {
        sprint_doc: SprintDoc {
            title: format!("FLOW-{}", fr.graph_name),
            slug: fr.graph_name.clone(),
            origin: "lope flow run".to_string(),
            domain: "engineering".to_string(),
            path: None,
            phases,
        },
        phase_verdicts: verdicts,
        ok: fr.ok,
        error: fr
            .escalation
            .as_ref()
            .map(|e| e.to_string())
            .unwrap_or_default(),
        total_duration_seconds: fr.total_duration_seconds,
    }
}

pub fn write_flow_run(fr: &FlowReport, out_dir: &str) -> std::io::Result<PathBuf> {
    let base = PathBuf::from(lope_core::paths::expanduser(out_dir));
    fs::create_dir_all(&base)?;

    let trace = base.join("trace.jsonl");
    let mut tf = fs::File::create(&trace)?;
    for r in &fr.node_results {
        let detail: String = r.detail.chars().take(1000).collect();
        let error: String = r.error.chars().take(500).collect();
        let line = json!({
            "node_id": r.node_id,
            "outcome": r.outcome,
            "label": r.label,
            "detail": redact_text(&detail),
            "duration_seconds": (r.duration_seconds * 100.0).round() / 100.0,
            "error": redact_text(&error),
            "model_calls": r.model_calls,
        });
        writeln!(tf, "{}", serde_json::to_string(&line).unwrap_or_default())?;
    }

    let report_md = base.join("report.md");
    fs::write(
        &report_md,
        format!("# Flow run: {}\n\n```\n{}\n```\n", fr.graph_name, redact_text(&fr.scorecard())),
    )?;
    Ok(base)
}
