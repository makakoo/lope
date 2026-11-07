//! `retry_policy.py`: typed, budget-aware transient retry.

use crate::errors::{tool_permission_denial, ProviderError};
use crate::pyre::{is_match, re, search};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub const MAX_TRANSIENT_RETRIES: u32 = 1;
pub const DEFAULT_TRANSIENT_DELAY_SECONDS: f64 = 0.25;
pub const MAX_JITTER_SECONDS: f64 = 0.25;

#[derive(Debug, Clone)]
pub struct Classification {
    pub kind: &'static str,
    pub retryable: bool,
    pub retry_after: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub retry: bool,
    pub reason: &'static str,
    pub delay: f64,
    pub classification: &'static str,
}

/// The error as `classify_failure` sees it: `str(error)` plus its type.
pub struct ErrView<'a> {
    pub text: &'a str,
    pub unverified: bool,
    pub tool_permission: bool,
}

impl<'a> From<&'a ProviderError> for ErrView<'a> {
    fn from(e: &'a ProviderError) -> Self {
        let text: &str = match e {
            ProviderError::Infra(m) | ProviderError::ToolPermission(m) | ProviderError::Runtime(m) | ProviderError::NotImplemented(m) => m,
            ProviderError::Unverified { message, .. } => message,
            ProviderError::Budget { message, .. } => message,
        };
        ErrView {
            text,
            unverified: matches!(e, ProviderError::Unverified { .. }),
            tool_permission: matches!(e, ProviderError::ToolPermission(_)),
        }
    }
}

pub fn classify(e: ErrView) -> Classification {
    let c = |kind, retryable, retry_after| Classification { kind, retryable, retry_after };
    if e.unverified {
        return c("cleanup_failed", false, None);
    }
    static R: OnceLock<[fancy_regex::Regex; 4]> = OnceLock::new();
    let [split, legacy, retry_after_re, empty] = R.get_or_init(|| {
        [
            re(r";\s*(?:stdout|stderr):"),
            re(r"(?s)\A[^;\n]+ returned empty output;\s*stderr:\s*(.*)\z"),
            re(r"(?i)retry[-_ ]after\s*(?::|=)?\s*(?P<seconds>\d+(?:\.\d+)?)"),
            re(r"(?<![\w-])empty output\b"),
        ]
    });
    let text = crate::pyre::strip(e.text);
    let lowered = text.to_lowercase();
    let header = match split.find(text).ok().flatten() {
        Some(m) => &text[..m.start()],
        None => text,
    };
    let legacy_stderr = search(legacy, text).map(|c| c.get(1).unwrap().as_str().to_string()).unwrap_or_default();
    static TP: OnceLock<fancy_regex::Regex> = OnceLock::new();
    let tp = TP.get_or_init(|| re(r"\A[\w.-]+ tool_permission_denied\s*:"));
    if e.tool_permission
        || is_match(tp, header)
        || (text.starts_with("jetski:") && !tool_permission_denial(text).is_empty())
        || !tool_permission_denial(&legacy_stderr).is_empty()
    {
        return c("tool_permission_denied", false, None);
    }
    let retry_after = search(retry_after_re, text).and_then(|m| m.name("seconds").unwrap().as_str().parse::<f64>().ok()).map(|v| v.max(0.0));
    if lowered.contains("429") || lowered.contains("rate limit") || lowered.contains("too many requests") {
        return c("rate_limited", true, retry_after);
    }
    if lowered.contains("503") || lowered.contains("service unavailable") {
        return c("service_unavailable", true, retry_after);
    }
    if ["connection reset", "connectionreseterror", "econnreset", "remote end closed connection", "remotedisconnected"]
        .iter()
        .any(|t| lowered.contains(t))
    {
        return c("connection_reset", true, retry_after);
    }
    if lowered.contains("timed out") || lowered.contains("timeout after") || lowered.contains("provider_timeout") {
        return c("provider_timeout", false, None);
    }
    if lowered.contains("output_limit") || lowered.contains("output limit") || (lowered.contains("exceeded") && lowered.contains("stdout")) {
        return c("output_limit", false, None);
    }
    if lowered.contains("input_limit") || lowered.contains("argv payload") || (lowered.contains("prompt") && lowered.contains("too large")) {
        return c("input_limit", false, None);
    }
    if ["unauthorized", "forbidden", "invalid api key", "authentication", "http 401", "http 403"].iter().any(|t| lowered.contains(t)) {
        return c("auth_error", false, None);
    }
    if ["parse error", "invalid json", "no ---verdict---", "unparseable"].iter().any(|t| lowered.contains(t)) {
        return c("parse_error", false, None);
    }
    if is_match(empty, &lowered) {
        return c("empty_output", false, None);
    }
    c("launch_error", false, None)
}

fn jitter(seed: &str) -> f64 {
    if matches!(std::env::var("LOPE_RETRY_JITTER").unwrap_or_default().trim().to_lowercase().as_str(), "0" | "off" | "false" | "no") {
        return 0.0;
    }
    let d = Sha256::digest(seed.as_bytes());
    let frac = u32::from_be_bytes([d[0], d[1], d[2], d[3]]) as f64 / (u32::MAX as f64);
    frac * MAX_JITTER_SECONDS
}

pub fn decide(e: ErrView, attempt: u32, remaining: f64, requested_timeout: f64, seed: &str, max_retries: u32) -> Decision {
    let c = classify(e);
    if !c.retryable {
        return Decision { retry: false, reason: "not_retryable", delay: 0.0, classification: c.kind };
    }
    if attempt >= max_retries {
        return Decision { retry: false, reason: "retry_limit_reached", delay: 0.0, classification: c.kind };
    }
    let delay = c.retry_after.unwrap_or_else(|| DEFAULT_TRANSIENT_DELAY_SECONDS + jitter(seed));
    if remaining < delay + requested_timeout.max(0.0) {
        let reason = if c.retry_after.is_some() { "retry_after_exceeds_budget" } else { "insufficient_budget_for_complete_retry" };
        return Decision { retry: false, reason, delay, classification: c.kind };
    }
    Decision { retry: true, reason: "transient_retry", delay, classification: c.kind }
}
