//! Port of `lope/minimality.py`: minimality discipline for Lope prompts.

pub const DEFAULT_MODE: &str = "audit";

pub const OVER_ENGINEERING_FOCUS: &str = "\
Review for over-engineering only. One line per finding:
<path>:L<line>: <tag>: <what to cut>. <replacement>.
Tags: delete, stdlib, native, yagni, reuse, shrink, wrong-layer.
Do not report correctness, security, performance, or style issues unless the issue is unnecessary complexity.
Never recommend removing validation, security, accessibility, data-loss handling, or explicit requirements.
If nothing real: Lean already. Ship.";

pub fn mode(value: Option<&str>) -> &'static str {
    let raw = match value {
        Some(v) => v.to_string(),
        None => std::env::var("LOPE_MINIMALITY").unwrap_or_else(|_| DEFAULT_MODE.into()),
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => "off",
        "enforce" => "enforce",
        _ => "audit",
    }
}

fn effective_mode(value: Option<&str>, domain: Option<&str>) -> &'static str {
    let resolved = mode(value);
    let norm_domain = domain.unwrap_or("engineering").trim().to_ascii_lowercase();
    let explicit = value.is_some() || std::env::var_os("LOPE_MINIMALITY").is_some();
    if !explicit && !norm_domain.is_empty() && norm_domain != "engineering" {
        return "off";
    }
    resolved
}

pub fn implementation_directive(value: Option<&str>, domain: Option<&str>) -> String {
    let resolved = effective_mode(value, domain);
    if resolved == "off" {
        return String::new();
    }
    let level = if resolved == "enforce" {
        "enforced completion contract"
    } else {
        "audit guidance"
    };
    format!(
        "Minimality discipline ({resolved}, {level}):\n\
- Understand touched flow before writing.\n\
- Prefer no build, existing code, stdlib, native platform, installed deps, then minimum custom code.\n\
- Reuse helpers before creating new ones.\n\
- Bugfix root cause once; search sibling callers.\n\
- Do not cut security, validation, accessibility, data-loss handling, or explicit requirements.\n\
- Leave one runnable check for non-trivial logic."
    )
}

pub fn validator_rubric(
    value: Option<&str>,
    stage: Option<&str>,
    domain: Option<&str>,
) -> String {
    let resolved = effective_mode(value, domain);
    if resolved == "off" {
        return String::new();
    }
    if !matches!(stage, None | Some("quality")) {
        return String::new();
    }
    let threshold = if resolved == "enforce" {
        "In enforce mode, return NEEDS_FIX only for material bloat with a concrete safer replacement."
    } else {
        "In audit mode, report material findings but do not fail spec-compliant work for style alone."
    };
    format!(
        "Minimality review ({resolved}):\n\
- Flag unnecessary new dependencies.\n\
- Flag duplicate helpers/patterns already present.\n\
- Flag interfaces/factories/providers/config with one real implementation.\n\
- Flag broad rewrites where a local/shared fix satisfies the phase.\n\
- Flag symptom patches that miss sibling callers.\n\
- Never ask to remove security, validation, accessibility, data-loss handling, or explicit requirements.\n\
{threshold}"
    )
}

pub fn resolve_review_focus(focus: Option<&str>) -> String {
    let text = focus.unwrap_or("").trim();
    match text.to_ascii_lowercase().as_str() {
        "over-engineering" | "overengineering" | "minimality" | "lazy-build" | "lazy build" => {
            OVER_ENGINEERING_FOCUS.to_string()
        }
        _ => text.to_string(),
    }
}
