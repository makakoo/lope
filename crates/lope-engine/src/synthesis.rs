//! Port of `lope/synthesis.py`: synthesis pass for single-shot and review verbs.

use crate::findings::ConsensusFinding;
use lope_core::redaction::redact_text;
use lope_providers::budget::Context;
use lope_providers::generic::Validator;
use lope_providers::invocation::invoke_generate;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const REQUIRED_SECTIONS: [&str; 4] = [
    "## Consensus",
    "## Disagreements",
    "## Highest-risk item",
    "## Recommended action",
];

pub const OPTIONAL_SECTIONS: [&str; 1] = ["## Follow-up questions"];

pub const DEFAULT_SOURCE_BYTE_LIMIT: usize = 64 * 1024;
pub const HARD_SOURCE_BYTE_LIMIT: usize = 256 * 1024;
pub const DEFAULT_TOTAL_PROMPT_BYTE_LIMIT: usize = 96 * 1024;
pub const HARD_TOTAL_PROMPT_BYTE_LIMIT: usize = 1024 * 1024;
const TRUNCATION_MARKER: &str = "\n...[source truncated; head and tail retained]...\n";

pub type FanoutTuple = (String, String, Option<String>);

#[derive(Debug, Clone)]
pub struct SynthesisResult {
    pub ok: bool,
    pub text: String,
    pub error: String,
    pub primary: String,
    pub truncations: Vec<Value>,
}

pub fn anon_label(index: usize) -> String {
    if index < 26 {
        return format!("Response {}", (b'A' + index as u8) as char);
    }
    let first = (index / 26) - 1;
    let second = index % 26;
    format!(
        "Response {}{}",
        (b'A' + (first % 26) as u8) as char,
        (b'A' + second as u8) as char
    )
}

pub fn truncate_utf8(text: &str, limit: usize) -> (String, usize) {
    let raw = text.as_bytes();
    if raw.len() <= limit {
        return (text.to_string(), 0);
    }
    let marker_bytes = &TRUNCATION_MARKER.as_bytes()[..TRUNCATION_MARKER.len().min(limit)];
    let remaining = limit.saturating_sub(marker_bytes.len());
    let head_size = remaining / 2;
    let tail_size = remaining - head_size;
    let mut head_end = head_size.min(raw.len());
    while head_end > 0 && std::str::from_utf8(&raw[..head_end]).is_err() {
        head_end -= 1;
    }
    let head_text = std::str::from_utf8(&raw[..head_end]).unwrap_or("");

    let mut tail_start = if tail_size == 0 {
        raw.len()
    } else {
        raw.len().saturating_sub(tail_size)
    };
    while tail_start < raw.len() && std::str::from_utf8(&raw[tail_start..]).is_err() {
        tail_start += 1;
    }
    let tail_text = std::str::from_utf8(&raw[tail_start..]).unwrap_or("");
    let marker_str = String::from_utf8_lossy(marker_bytes);
    let rendered = format!("{head_text}{marker_str}{tail_text}");
    let retained = rendered.len();
    (rendered, raw.len().saturating_sub(retained))
}

fn format_finding_line<F>(f: &ConsensusFinding, mapper: F) -> String
where
    F: Fn(&str) -> String,
{
    let mut location = String::new();
    if let Some(fl) = &f.file {
        if !fl.is_empty() {
            location = fl.clone();
            if let Some(ln) = f.line {
                location.push_str(&format!(":{ln}"));
            }
        }
    }
    let detected = if f.detected_by.is_empty() {
        "—".to_string()
    } else {
        f.detected_by
            .iter()
            .map(|d| mapper(d))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "- [{}] [{}] {location} — {} (consensus {:.2}, agreement {}/{}, detected_by: {detected})",
        f.consensus_level.as_str().to_ascii_uppercase(),
        f.severity.to_ascii_uppercase(),
        f.message,
        f.consensus_score,
        f.agreement_count,
        f.total_validators
    )
}

fn build_anonymizer(
    responses: &[FanoutTuple],
    structured_findings: Option<&[ConsensusFinding]>,
) -> HashMap<String, String> {
    let mut ordered: Vec<String> = Vec::new();
    for (name, _, _) in responses {
        if !name.is_empty() && !ordered.contains(name) {
            ordered.push(name.clone());
        }
    }
    if let Some(sfs) = structured_findings {
        for f in sfs {
            for det in &f.detected_by {
                if !det.is_empty() && !ordered.contains(det) {
                    ordered.push(det.clone());
                }
            }
        }
    }
    ordered
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n, anon_label(i)))
        .collect()
}

pub fn build_synthesis_prompt(
    task: &str,
    responses: &[FanoutTuple],
    structured_findings: Option<&[ConsensusFinding]>,
    anonymous: bool,
    source_byte_limit: usize,
    total_byte_limit: usize,
    truncation_log: &mut Vec<Value>,
) -> String {
    let src_limit = source_byte_limit.clamp(1024, HARD_SOURCE_BYTE_LIMIT);
    let tot_limit = total_byte_limit.clamp(16 * 1024, HARD_TOTAL_PROMPT_BYTE_LIMIT);
    let raw_task = redact_text(task).trim().to_string();
    let task_in = if raw_task.is_empty() {
        "(no task description provided)".to_string()
    } else {
        raw_task
    };
    let (task_text, task_omitted) = truncate_utf8(&task_in, 32 * 1024);
    if task_omitted > 0 {
        truncation_log.push(json!({
            "source": "task",
            "omitted_bytes": task_omitted,
            "retained_bytes": task_text.len(),
        }));
    }

    let mut parts: Vec<String> = vec![
        "You are synthesizing N independent AI critiques of the same task.".into(),
        String::new(),
        format!("Task: {task_text}"),
        String::new(),
    ];

    let mut successes: Vec<(String, String)> = Vec::new();
    for (name, answer, err) in responses {
        if err.is_some() || answer.trim().is_empty() {
            continue;
        }
        let safe_name = redact_text(name).trim().to_string();
        let safe_answer = redact_text(answer).trim().to_string();
        let (trunc_ans, omitted) = truncate_utf8(&safe_answer, src_limit);
        if omitted > 0 {
            let src_name = if safe_name.is_empty() {
                "unknown"
            } else {
                &safe_name
            };
            truncation_log.push(json!({
                "source": src_name,
                "omitted_bytes": omitted,
                "retained_bytes": trunc_ans.len(),
            }));
        }
        successes.push((safe_name, trunc_ans));
    }

    let errors: Vec<(String, String)> = responses
        .iter()
        .filter_map(|(name, _, err)| {
            let e = err.as_ref()?;
            Some((
                redact_text(name).trim().to_string(),
                redact_text(e).trim().to_string(),
            ))
        })
        .collect();

    let anon_map = if anonymous {
        Some(build_anonymizer(responses, structured_findings))
    } else {
        None
    };
    let label_of = |name: &str| -> String {
        if let Some(m) = &anon_map {
            m.get(name)
                .cloned()
                .unwrap_or_else(|| "Response ?".into())
        } else if name.is_empty() {
            "unknown".into()
        } else {
            name.to_string()
        }
    };

    if let Some(sfs) = structured_findings {
        parts.push(format!(
            "Consensus findings (already deduped + ranked across {} validators):",
            responses.len()
        ));
        if sfs.is_empty() {
            parts.push("- (no findings parsed by the consensus pipeline)".into());
        } else {
            let findings_budget = (tot_limit / 2).max(8 * 1024);
            let mut used = 0usize;
            for (index, f) in sfs.iter().enumerate() {
                let raw_line = redact_text(&format_finding_line(f, &label_of));
                let (line, mut omitted) =
                    truncate_utf8(&raw_line, findings_budget.min(8 * 1024));
                let encoded = line.len() + 1;
                if used + encoded > findings_budget {
                    let remaining = findings_budget.saturating_sub(used);
                    if remaining >= 1024 {
                        let (l2, extra) = truncate_utf8(&line, remaining);
                        parts.push(l2);
                        omitted += extra;
                    }
                    let rest_bytes: usize = sfs[index + 1..]
                        .iter()
                        .map(|r| format_finding_line(r, |x| x.to_string()).len())
                        .sum();
                    truncation_log.push(json!({
                        "source": format!("structured_findings[{index}]"),
                        "omitted_bytes": omitted + rest_bytes,
                        "retained_bytes": remaining,
                    }));
                    break;
                }
                parts.push(line.clone());
                used += encoded;
                if omitted > 0 {
                    truncation_log.push(json!({
                        "source": format!("structured_findings[{index}]"),
                        "omitted_bytes": omitted,
                        "retained_bytes": line.len(),
                    }));
                }
            }
        }
        parts.push(String::new());
        if !successes.is_empty() {
            let label_kind = if anonymous { "responses" } else { "validators" };
            let names = successes
                .iter()
                .map(|(n, _)| label_of(n))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!("Source {label_kind}: {names}"));
            parts.push(String::new());
        }
    } else {
        parts.push("Validator responses:".into());
        parts.push(String::new());
        if successes.is_empty() {
            parts.push("(no validator returned a non-empty response)".into());
            parts.push(String::new());
        }
        for (name, answer) in &successes {
            parts.push(format!("[{}]", label_of(name)));
            parts.push(answer.clone());
            parts.push(String::new());
        }
    }

    if !errors.is_empty() {
        parts.push("Validator errors (these validators did NOT contribute opinions):".into());
        for (name, err) in &errors {
            let e_str = if err.is_empty() {
                "(empty error)"
            } else {
                err.as_str()
            };
            parts.push(format!("- {}: {e_str}", label_of(name)));
        }
        parts.push(String::new());
    }

    parts.push("Produce a synthesis using these EXACT section headings, in this order:".into());
    for s in REQUIRED_SECTIONS {
        parts.push(s.into());
    }
    for opt in OPTIONAL_SECTIONS {
        parts.push(format!(
            "{opt}   (only if blocking decisions need clarification)"
        ));
    }
    parts.extend([
        String::new(),
        "Rules:".into(),
        "- Never invent a finding not present in the responses or consensus findings above. If you must extrapolate, prefix the bullet with `Inference:`.".into(),
        "- Be concise. Each section is at most 5 short bullets.".into(),
        "- Do not repeat the original responses verbatim.".into(),
        "- Identify the single highest-risk item and explain why in one sentence.".into(),
        "- Make ONE Recommended action — the next concrete step.".into(),
        "- If validators errored, acknowledge the gap; do not fabricate their position.".into(),
    ]);
    if anonymous {
        parts.push(
            "- Refer to sources only by Response A/B/C labels. Do not infer or guess validator identity.".into(),
        );
    }

    let mut prompt = format!("{}\n", parts.join("\n").trim_end());
    if prompt.len() > tot_limit {
        let (p2, omitted) = truncate_utf8(&prompt, tot_limit);
        prompt = p2;
        truncation_log.push(json!({
            "source": "aggregate_synthesis_prompt",
            "omitted_bytes": omitted,
            "retained_bytes": prompt.len(),
        }));
    }
    prompt
}

pub fn run_synthesis(
    primary: Option<&dyn Validator>,
    prompt: &str,
    timeout: f64,
    truncations: Vec<Value>,
    ctx: Option<&Context>,
) -> SynthesisResult {
    let Some(p) = primary else {
        return SynthesisResult {
            ok: false,
            text: String::new(),
            error: "No primary validator available for synthesis.".into(),
            primary: String::new(),
            truncations,
        };
    };
    let name = p.name().to_string();
    match invoke_generate(p, prompt, timeout, ctx, Some("synthesis")) {
        Ok(text) => {
            let redacted = redact_text(&text).trim().to_string();
            if redacted.is_empty() {
                SynthesisResult {
                    ok: false,
                    text: String::new(),
                    error: "Primary returned empty synthesis output.".into(),
                    primary: name,
                    truncations,
                }
            } else {
                SynthesisResult {
                    ok: true,
                    text: redacted,
                    error: String::new(),
                    primary: name,
                    truncations,
                }
            }
        }
        Err(e) => SynthesisResult {
            ok: false,
            text: String::new(),
            error: redact_text(&format!("RuntimeError: {e}")).trim().to_string(),
            primary: name,
            truncations,
        },
    }
}

pub fn format_synthesis(result: &SynthesisResult, machine_json: bool) -> String {
    if machine_json {
        return result.text.clone();
    }
    if !result.ok {
        let err = if result.error.is_empty() {
            "unknown error"
        } else {
            &result.error
        };
        return format!("━━━ synthesis ━━━\n[synthesis unavailable: {err}]\n");
    }
    let prim = if result.primary.is_empty() {
        "primary"
    } else {
        &result.primary
    };
    let header = format!("━━━ synthesis ({prim}) ━━━");
    let trunc_note = if !result.truncations.is_empty() {
        format!(
            "[bounded input: {} source truncation(s)]\n",
            result.truncations.len()
        )
    } else {
        String::new()
    };
    format!("{header}\n{trunc_note}{}\n", result.text.trim_end())
}
