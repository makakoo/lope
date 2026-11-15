//! `request_plan.py`: deterministic byte-safe request admission and semantic chunk planning.

use crate::budget::InputProfile;
use crate::generic::Validator;
use serde_json::{json, Value};
use std::collections::HashSet;

pub const DEFAULT_DIRECT_PROMPT_BYTES: usize = 96 * 1024;
pub const DEFAULT_CHUNK_BYTES: usize = 64 * 1024;
pub const PROMPT_WRAPPER_RESERVE_BYTES: usize = 4 * 1024;
const ARGV_POLICY_LIMIT: usize = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticUnit {
    pub label: String,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub kind: String,
}

impl SemanticUnit {
    pub fn new(label: impl Into<String>, content: impl Into<String>, start_line: usize, end_line: usize, kind: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            content: content.into(),
            start_line,
            end_line,
            kind: kind.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestChunk {
    pub index: usize,
    pub content: String,
    pub labels: Vec<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub kind: String,
    pub overlap_bytes: usize,
}

impl RequestChunk {
    pub fn profile(&self) -> InputProfile {
        InputProfile::from_text(&self.content)
    }

    pub fn digest(&self) -> String {
        lope_core::sha256_hex(self.content.as_bytes())
    }

    pub fn label(&self) -> String {
        let joined = self.labels.join(", ");
        if joined.is_empty() {
            format!("chunk-{}", self.index + 1)
        } else {
            joined
        }
    }

    pub fn to_dict(&self, include_content: bool) -> Value {
        let profile = self.profile();
        let mut data = json!({
            "index": self.index,
            "label": self.label(),
            "labels": self.labels,
            "kind": self.kind,
            "start_line": self.start_line,
            "end_line": self.end_line,
            "utf8_bytes": profile.utf8_bytes,
            "lines": profile.lines,
            "estimated_tokens": profile.estimated_tokens,
            "estimated_tokens_label": "approximate (ceil UTF-8 bytes / 3)",
            "overlap_bytes": self.overlap_bytes,
            "sha256": self.digest(),
        });
        if include_content {
            if let Some(obj) = data.as_object_mut() {
                obj.insert("content".into(), json!(self.content));
            }
        }
        data
    }
}

pub struct Transport {
    pub validator: String,
    pub transport: &'static str,
    pub max_prompt_bytes: usize,
}

pub fn transport_for(v: &dyn Validator) -> Transport {
    let t = v.transport();
    let cap = match t {
        "argv" => ARGV_POLICY_LIMIT - PROMPT_WRAPPER_RESERVE_BYTES,
        _ => 1024 * 1024,
    };
    Transport { validator: v.name().to_string(), transport: t, max_prompt_bytes: cap }
}

pub struct Plan {
    pub json: Value,
    pub direct: bool,
    pub action: String,
    pub reason: String,
    pub mitigation: String,
    pub chunks: Vec<RequestChunk>,
    pub required_chunks: usize,
    pub planned_calls: usize,
    pub utf8_bytes: usize,
    pub maximum_concurrency: usize,
    pub nominal: f64,
}

pub struct Limits {
    pub max_chunks: i64,
    pub max_calls: i64,
    pub max_input_bytes: i64,
}

fn keepends_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            out.push(&text[start..=idx]);
            start = idx + 1;
        } else if ch == '\r' {
            let next_is_nl = text[idx + 1..].starts_with('\n');
            if !next_is_nl {
                out.push(&text[start..=idx]);
                start = idx + 1;
            }
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

fn line_end(start_line: usize, text: &str) -> usize {
    let count = text.matches('\n').count() + usize::from(!text.ends_with('\n'));
    start_line + count.max(1) - 1
}

fn units_from_boundaries(
    text: &str,
    boundaries: &[usize],
    label: &str,
    kind: &str,
) -> Vec<SemanticUnit> {
    let lines = keepends_lines(text);
    if lines.is_empty() {
        return vec![SemanticUnit::new(label, "", 1, 1, kind)];
    }
    let mut starts_set: HashSet<usize> = boundaries
        .iter()
        .map(|&v| v.max(1).min(lines.len()))
        .collect();
    starts_set.insert(1);
    let mut starts: Vec<usize> = starts_set.into_iter().collect();
    starts.sort_unstable();

    let mut units = Vec::new();
    for (index, &start) in starts.iter().enumerate() {
        let end_exclusive = starts.get(index + 1).copied().unwrap_or(lines.len() + 1);
        let content: String = lines[start - 1..end_exclusive - 1].concat();
        if !content.is_empty() {
            units.push(SemanticUnit::new(
                format!("{label} lines {start}-{}", end_exclusive - 1),
                content,
                start,
                end_exclusive - 1,
                kind,
            ));
        }
    }
    units
}

fn diff_units(text: &str, label: &str) -> Vec<SemanticUnit> {
    let mut boundaries = Vec::new();
    for (idx, line) in keepends_lines(text).iter().enumerate() {
        if line.starts_with("diff --git ") || line.starts_with("@@ ") {
            boundaries.push(idx + 1);
        }
    }
    units_from_boundaries(text, &boundaries, label, "diff")
}

fn markdown_units(text: &str, label: &str) -> Vec<SemanticUnit> {
    let lines = keepends_lines(text);
    let mut boundaries = Vec::new();
    let mut in_fence = false;
    let mut saw_structure = false;
    for (idx, line) in lines.iter().enumerate() {
        let stripped = line.trim_start();
        if stripped.starts_with("```") || stripped.starts_with("~~~") {
            in_fence = !in_fence;
            saw_structure = true;
            continue;
        }
        if !in_fence && stripped.starts_with('#') && stripped.trim_start_matches('#').starts_with(' ') {
            boundaries.push(idx + 1);
            saw_structure = true;
        }
    }
    if !saw_structure {
        return Vec::new();
    }
    units_from_boundaries(text, &boundaries, label, "markdown")
}

fn paragraph_units(text: &str, label: &str) -> Vec<SemanticUnit> {
    let lines = keepends_lines(text);
    if lines.is_empty() {
        return vec![SemanticUnit::new(label, "", 1, 1, "text")];
    }
    let mut units = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut start = 1usize;
    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        if current.is_empty() {
            start = line_no;
        }
        current.push(*line);
        if line.trim().is_empty() {
            let content = current.concat();
            units.push(SemanticUnit::new(
                format!("{label} lines {start}-{line_no}"),
                content,
                start,
                line_no,
                "paragraph",
            ));
            current.clear();
        }
    }
    if !current.is_empty() {
        let content = current.concat();
        units.push(SemanticUnit::new(
            format!("{label} lines {start}-{}", lines.len()),
            content,
            start,
            lines.len(),
            "paragraph",
        ));
    }
    units
}

pub fn semantic_units(text: &str, source_label: &str, kind: &str) -> Vec<SemanticUnit> {
    let mut selected = kind;
    if selected == "auto" {
        if text.contains("diff --git ") || text.contains("\n@@ -") {
            selected = "diff";
        } else if source_label.to_ascii_lowercase().ends_with(".py") {
            selected = "python";
        } else if text.lines().any(|line| {
            let s = line.trim_start();
            s.starts_with("```")
                || s.starts_with("~~~")
                || (s.starts_with('#') && s.trim_start_matches('#').starts_with(' '))
        }) {
            selected = "markdown";
        } else {
            selected = "python";
        }
    }
    if selected == "diff" {
        return diff_units(text, source_label);
    }
    if selected == "markdown" {
        let units = markdown_units(text, source_label);
        if !units.is_empty() {
            return units;
        }
    }
    paragraph_units(text, source_label)
}

fn split_utf8(text: &str, max_bytes: usize) -> Vec<String> {
    let max_bytes = max_bytes.max(4);
    let raw = text.as_bytes();
    let mut parts = Vec::new();
    let mut offset = 0;
    while offset < raw.len() {
        let mut end = (offset + max_bytes).min(raw.len());
        if end < raw.len() {
            if let Some(pos) = raw[offset..end].iter().rposition(|&b| b == b'\n') {
                end = offset + pos + 1;
            }
        }
        let mut part_str = None;
        while end > offset {
            if let Ok(s) = std::str::from_utf8(&raw[offset..end]) {
                part_str = Some(s.to_string());
                break;
            }
            end -= 1;
        }
        let part = part_str.expect("unable to split UTF-8 input within byte limit");
        parts.push(part);
        offset = end;
    }
    if parts.is_empty() {
        vec![String::new()]
    } else {
        parts
    }
}

fn explode_units(units: &[SemanticUnit], max_bytes: usize) -> Vec<SemanticUnit> {
    let mut out = Vec::new();
    for unit in units {
        if unit.content.len() <= max_bytes {
            out.push(unit.clone());
            continue;
        }
        let mut line_cursor = unit.start_line;
        for (index, part) in split_utf8(&unit.content, max_bytes).into_iter().enumerate() {
            let end_line = line_end(line_cursor, &part);
            let nl_count = part.matches('\n').count();
            out.push(SemanticUnit::new(
                format!("{} part {}", unit.label, index + 1),
                part,
                line_cursor,
                end_line,
                unit.kind.clone(),
            ));
            line_cursor += nl_count;
        }
    }
    out
}

pub fn pack_units(
    units: &[SemanticUnit],
    max_bytes: usize,
    overlap_lines: usize,
) -> Vec<RequestChunk> {
    let max_bytes = max_bytes.max(4);
    let exploded = explode_units(units, max_bytes);
    let mut groups: Vec<Vec<SemanticUnit>> = Vec::new();
    let mut current: Vec<SemanticUnit> = Vec::new();
    let mut current_bytes = 0usize;
    for unit in exploded {
        let size = unit.content.len();
        if !current.is_empty() && current_bytes + size > max_bytes {
            groups.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current_bytes += size;
        current.push(unit);
    }
    if !current.is_empty() {
        groups.push(current);
    }

    let mut chunks: Vec<RequestChunk> = Vec::new();
    for group in groups {
        let content: String = group.iter().map(|u| u.content.as_str()).collect();
        let mut labels: Vec<String> = Vec::new();
        for u in &group {
            if !labels.contains(&u.label) {
                labels.push(u.label.clone());
            }
        }
        let first_kind = &group[0].kind;
        let kind = if group.iter().all(|u| &u.kind == first_kind) {
            first_kind.clone()
        } else {
            "mixed".to_string()
        };
        chunks.push(RequestChunk {
            index: chunks.len(),
            content,
            labels,
            start_line: group[0].start_line,
            end_line: group.last().unwrap().end_line,
            kind,
            overlap_bytes: 0,
        });
    }

    if overlap_lines > 0 && chunks.len() > 1 {
        let mut with_overlap = vec![chunks[0].clone()];
        for chunk in chunks.into_iter().skip(1) {
            let prev_content = &with_overlap.last().unwrap().content;
            let prev_lines = keepends_lines(prev_content);
            let take_from = prev_lines.len().saturating_sub(overlap_lines);
            let mut prefix: String = prev_lines[take_from..].concat();
            while !prefix.is_empty() && prefix.len() + chunk.content.len() > max_bytes {
                let p_lines = keepends_lines(&prefix);
                if p_lines.len() > 1 {
                    prefix = p_lines[1..].concat();
                } else {
                    prefix.clear();
                }
            }
            let prefix_nl = prefix.matches('\n').count();
            let overlap_bytes = prefix.len();
            with_overlap.push(RequestChunk {
                index: chunk.index,
                content: format!("{prefix}{}", chunk.content),
                labels: chunk.labels,
                start_line: chunk.start_line.saturating_sub(prefix_nl).max(1),
                end_line: chunk.end_line,
                kind: chunk.kind,
                overlap_bytes,
            });
        }
        chunks = with_overlap;
    }

    let mut unique: Vec<RequestChunk> = Vec::new();
    let mut seen: HashSet<(String, Vec<String>, usize, usize)> = HashSet::new();
    for chunk in chunks {
        let key = (chunk.digest(), chunk.labels.clone(), chunk.start_line, chunk.end_line);
        if seen.contains(&key) {
            continue;
        }
        seen.insert(key);
        unique.push(RequestChunk {
            index: unique.len(),
            content: chunk.content,
            labels: chunk.labels,
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            kind: chunk.kind,
            overlap_bytes: chunk.overlap_bytes,
        });
    }
    unique
}

/// `plan_request(...)` for the direct case; `direct == false` means Python
/// would chunk or reject.
#[allow(clippy::too_many_arguments)]
pub fn plan_direct(
    text: &str,
    mode: &str,
    validators: &[std::sync::Arc<dyn Validator>],
    policy: &str,
    lim: &Limits,
    per_call_timeout: f64,
    parallel: bool,
    extra_calls: usize,
) -> Plan {
    plan_request_full(
        text,
        mode,
        validators,
        policy,
        lim,
        per_call_timeout,
        parallel,
        true,
        "input",
        "auto",
        0,
        extra_calls,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn plan_request_full(
    text: &str,
    mode: &str,
    validators: &[std::sync::Arc<dyn Validator>],
    policy: &str,
    lim: &Limits,
    per_call_timeout: f64,
    parallel: bool,
    allow_chunk: bool,
    source_label: &str,
    kind: &str,
    overlap_lines: usize,
    extra_calls: usize,
    chunk_extra_calls: Option<usize>,
    units: Option<&[SemanticUnit]>,
) -> Plan {
    let prof = InputProfile::from_text(text);
    let ts: Vec<Transport> = validators.iter().map(|v| transport_for(v.as_ref())).collect();
    let n = ts.len();
    let direct_limit = ts.iter().map(|t| t.max_prompt_bytes.max(4)).fold(DEFAULT_DIRECT_PROMPT_BYTES, usize::min);
    let conc = if parallel { n } else { n.min(1) };
    let direct_calls = n + extra_calls;
    let direct_nominal = (if parallel && n > 0 { per_call_timeout } else { per_call_timeout * n as f64 })
        + extra_calls as f64 * per_call_timeout;
    let direct_forecast = prof.utf8_bytes * n;
    let fnum = |x: f64| {
        if x.fract() == 0.0 && x.abs() < 1e15 {
            json!(x as i64)
        } else {
            serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
        }
    };

    let mut action = "direct".to_string();
    let mut reason = format!("input fits direct byte ceiling {direct_limit}");
    let mut mitigation = String::new();
    let mut chunks: Vec<RequestChunk> = Vec::new();
    let mut required_chunks = 1usize;
    let mut planned_calls = direct_calls;
    let mut required_calls = direct_calls;
    let mut forecast_input = direct_forecast;
    let mut nominal = direct_nominal;

    let direct_safe = prof.utf8_bytes <= direct_limit;
    if policy != "chunk" && direct_safe {
        if (direct_calls as i64) > lim.max_calls {
            action = "reject".into();
            reason = format!("direct plan needs {direct_calls} calls but max is {}", lim.max_calls);
            mitigation = format!("raise --max-calls to at least {direct_calls} or reduce validators");
        } else if (direct_forecast as i64) > lim.max_input_bytes {
            action = "reject".into();
            reason = format!("direct plan accounts {direct_forecast} input bytes but max is {}", lim.max_input_bytes);
            mitigation = format!("raise LOPE_MAX_INPUT_BYTES to at least {direct_forecast} or reduce input");
        }
    } else if policy == "direct" {
        action = "reject".into();
        reason = format!("input is {} bytes; direct ceiling is {direct_limit}", prof.utf8_bytes);
        mitigation = "use --request-policy chunk, reduce the input, or select a file/stdin-capable provider".into();
    } else if !allow_chunk {
        action = "reject".into();
        reason = format!("{mode} cannot losslessly shape {} bytes into a direct request", prof.utf8_bytes);
        mitigation = "provide a smaller evidence brief or explicitly reduce the source context".into();
    } else {
        let effective_chunk_bytes = DEFAULT_CHUNK_BYTES.min(direct_limit.saturating_sub(PROMPT_WRAPPER_RESERVE_BYTES).max(4));
        let semantic = match units {
            Some(u) => u.to_vec(),
            None => semantic_units(text, source_label, kind),
        };
        chunks = pack_units(&semantic, effective_chunk_bytes, overlap_lines);
        required_chunks = chunks.len().max(1);
        let eff_extra = chunk_extra_calls.unwrap_or(extra_calls);
        required_calls = required_chunks * n + eff_extra;
        planned_calls = required_calls;
        forecast_input = chunks
            .iter()
            .map(|c| c.profile().utf8_bytes + PROMPT_WRAPPER_RESERVE_BYTES)
            .sum::<usize>()
            * n;
        nominal = required_chunks as f64 * (if parallel { per_call_timeout } else { per_call_timeout * n as f64 })
            + eff_extra as f64 * per_call_timeout;
        action = "chunk".into();
        reason = if policy == "chunk" {
            "chunk policy forced".into()
        } else {
            format!("input exceeds direct byte ceiling {direct_limit}")
        };
        if (required_chunks as i64) > lim.max_chunks {
            action = "reject".into();
            reason = format!("request needs {required_chunks} chunks but max is {}", lim.max_chunks);
            mitigation = format!(
                "use --max-chunks {required_chunks} --max-calls {required_calls}; nominal call ceiling {}s",
                crate::progress::g(nominal)
            );
        } else if (required_calls as i64) > lim.max_calls {
            action = "reject".into();
            reason = format!("request needs {required_calls} calls but max is {}", lim.max_calls);
            mitigation = format!("use --max-calls {required_calls}, reduce validators, or increase chunk size");
        } else if (forecast_input as i64) > lim.max_input_bytes {
            action = "reject".into();
            reason = format!("request accounts {forecast_input} input bytes but max is {}", lim.max_input_bytes);
            mitigation = format!("raise LOPE_MAX_INPUT_BYTES to at least {forecast_input} or reduce input");
        }
    }

    let direct = action == "direct";
    let json = json!({
        "schema_version": 1,
        "mode": mode,
        "action": action,
        "reason": reason,
        "policy": policy,
        "input": {
            "utf8_bytes": prof.utf8_bytes, "lines": prof.lines, "estimated_tokens": prof.estimated_tokens,
            "estimated_tokens_label": "approximate (ceil UTF-8 bytes / 3)",
        },
        "transports": ts.iter().map(|t| json!({
            "validator": t.validator, "transport": t.transport, "max_prompt_bytes": t.max_prompt_bytes,
            "direct_safe": prof.utf8_bytes <= DEFAULT_DIRECT_PROMPT_BYTES.min(t.max_prompt_bytes),
        })).collect::<Vec<_>>(),
        "chunks": chunks.iter().map(|c| c.to_dict(false)).collect::<Vec<_>>(),
        "required_chunks": required_chunks,
        "planned_calls": planned_calls,
        "required_calls": required_calls,
        "maximum_concurrency": conc,
        "nominal_wall_ceiling_seconds": fnum(nominal),
        "forecast_input_bytes": forecast_input,
        "limits": {"max_chunks": lim.max_chunks, "max_calls": lim.max_calls, "max_input_bytes": lim.max_input_bytes},
        "mitigation": if mitigation.is_empty() { Value::Null } else { json!(mitigation) },
    });
    Plan {
        json,
        direct,
        action,
        reason,
        mitigation,
        chunks,
        required_chunks,
        planned_calls,
        utf8_bytes: prof.utf8_bytes,
        maximum_concurrency: conc,
        nominal,
    }
}
