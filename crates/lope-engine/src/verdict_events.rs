//! `lope/verdict_events.py`: Persist what actually happened on every validator call.

use crate::verdict_repair::{ParseErrorCategory, RepairStatus, VerdictStatus};
use lope_core::redaction::redact_text;
use lope_memory::{is_memory_disabled, open_memory, LopeMemory, VerdictEventInput};
pub use lope_providers::models::ValidatorResult;
use std::collections::HashMap;

pub const PARSER_VERSION: &str = "parse_verdict_block/1";
pub const PROMPT_TEMPLATE_VERSION: &str = "executor/_build_validation_prompt/1";
pub const EVENT_SCHEMA_VERSION: &str = "1.0.0";
pub const ENV_DISABLE_EVENTS: &str = "LOPE_VERDICT_EVENTS";

pub fn events_enabled(env: Option<&HashMap<String, String>>) -> bool {
    let raw = match env {
        Some(m) => m.get(ENV_DISABLE_EVENTS).cloned().unwrap_or_default(),
        None => std::env::var(ENV_DISABLE_EVENTS).unwrap_or_default(),
    };
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "on" | "true" | "yes"
    )
}

pub fn events_disabled(env: Option<&HashMap<String, String>>) -> bool {
    !events_enabled(env)
}

pub fn task_spec_hash(text: &str) -> String {
    lope_core::sha256_hex(text.as_bytes())
}

pub fn classify_parse_error(result: &ValidatorResult) -> Option<ParseErrorCategory> {
    if result.verdict.status != VerdictStatus::InfraError {
        return None;
    }

    let haystack = format!("{} {}", result.error, result.verdict.rationale).to_ascii_lowercase();
    if haystack.contains("timed out") || haystack.contains("timeout") {
        return Some(ParseErrorCategory::Timeout);
    }
    if haystack.contains("output limit") || haystack.contains("output_limit") {
        return Some(ParseErrorCategory::OutputLimit);
    }
    if haystack.contains("no verdict") || haystack.contains("unparseable") {
        return Some(ParseErrorCategory::MissingVerdictBlock);
    }
    if haystack.contains("unknown verdict token") {
        return Some(ParseErrorCategory::UnknownEnum);
    }
    if !result.error.is_empty() {
        return Some(ParseErrorCategory::ProcessExit);
    }
    if result.raw_response.trim().is_empty() {
        return Some(ParseErrorCategory::Transport);
    }
    Some(ParseErrorCategory::MissingVerdictBlock)
}

#[allow(clippy::too_many_arguments)]
pub fn record_verdict_event(
    result: &ValidatorResult,
    prompt: &str,
    run_id: &str,
    gate_id: &str,
    validator_version: &str,
    validation_input_ref: &str,
    candidate_output_ref: &str,
    latency_s: Option<f64>,
    exit_status: Option<i64>,
    repair_attempted: bool,
    repair_status: Option<RepairStatus>,
    repaired_response: &str,
    initial_status: Option<VerdictStatus>,
    memory: Option<&LopeMemory>,
) -> bool {
    if !events_enabled(None) || is_memory_disabled() {
        return false;
    }

    let category_value = result.parse_error_category.clone().or_else(|| {
        classify_parse_error(result).map(|c| c.as_str().to_string())
    });
    let initial = initial_status
        .map(|s| s.as_str().to_string())
        .or_else(|| result.initial_parse_status.clone())
        .unwrap_or_else(|| result.verdict.status.as_str().to_string());
    let timed_out = category_value.as_deref() == Some(ParseErrorCategory::Timeout.as_str());

    let input = VerdictEventInput {
        run_id: run_id.to_string(),
        gate_id: if gate_id.is_empty() { None } else { Some(gate_id.to_string()) },
        validator_name: result.validator_name.clone(),
        validator_version: if validator_version.is_empty() { None } else { Some(validator_version.to_string()) },
        rendered_prompt: redact_text(prompt),
        validation_input_ref: if validation_input_ref.is_empty() { None } else { Some(validation_input_ref.to_string()) },
        raw_response: redact_text(&result.raw_response),
        initial_parse_status: initial,
        parse_error_category: category_value,
        repair_attempted,
        repair_status: repair_status.map(|s| s.as_str().to_string()),
        repaired_response: if repaired_response.is_empty() {
            None
        } else {
            Some(redact_text(repaired_response))
        },
        final_verdict: result.verdict.status.as_str().to_string(),
        candidate_output_ref: if candidate_output_ref.is_empty() { None } else { Some(candidate_output_ref.to_string()) },
        latency_s: Some(latency_s.unwrap_or(result.verdict.duration_seconds)),
        exit_status,
        timed_out,
    };

    if let Some(mem) = memory {
        mem.store_verdict_event(&input).is_ok()
    } else if let Ok(Some(mem)) = open_memory() {
        mem.store_verdict_event(&input).is_ok()
    } else {
        false
    }
}
