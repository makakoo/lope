//! CLI-routing stylesheet (`lope/flow/stylesheet.py`, Sprint S7, `FR36`).

use crate::model::FlowNode;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const PROP_KEYS: &[&str] = &["primary", "validators", "model", "reasoning_effort", "timeout"];

#[derive(Debug, Clone)]
pub struct StyleRule {
    pub selector: String,
    pub props: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    pub rules: Vec<StyleRule>,
}

impl Stylesheet {
    pub fn parse(text: &str) -> Self {
        if text.trim().is_empty() {
            return Self::default();
        }
        static RULE_RE: OnceLock<Regex> = OnceLock::new();
        let re = RULE_RE.get_or_init(|| Regex::new(r"([^{}]+)\{([^{}]*)\}").unwrap());
        let mut rules = Vec::new();
        for caps in re.captures_iter(text) {
            let selector = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let body = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let mut props = HashMap::new();
            for decl in body.split(';') {
                let decl = decl.trim();
                let Some((k, v)) = decl.split_once(':') else {
                    continue;
                };
                let key = k.trim().to_lowercase();
                let val = v.trim();
                if PROP_KEYS.contains(&key.as_str()) && !val.is_empty() {
                    props.insert(key, val.to_string());
                }
            }
            if !selector.is_empty() && !props.is_empty() {
                rules.push(StyleRule {
                    selector: selector.to_string(),
                    props,
                });
            }
        }
        Self { rules }
    }

    pub fn resolve(&self, node: &FlowNode) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for r in &self.rules {
            if r.selector == "*" {
                out.extend(r.props.clone());
            }
        }
        if let Some(cls) = node.node_class() {
            let wanted: HashSet<String> = cls
                .split_whitespace()
                .filter(|c| !c.is_empty())
                .map(|c| format!(".{c}"))
                .collect();
            for r in &self.rules {
                if wanted.contains(&r.selector) {
                    out.extend(r.props.clone());
                }
            }
        }
        let id_sel = format!("#{}", node.id);
        for r in &self.rules {
            if r.selector == id_sel {
                out.extend(r.props.clone());
            }
        }
        for k in PROP_KEYS {
            if let Some(v) = node.attrs.get(*k) {
                out.insert((*k).to_string(), v.clone());
            }
        }
        out
    }

    pub fn unknown_validator_warnings(&self, known: &[String]) -> Vec<String> {
        let known_set: HashSet<&str> = known.iter().map(|s| s.as_str()).collect();
        let mut warnings = Vec::new();
        for r in &self.rules {
            for key in ["primary", "validators"] {
                if let Some(raw) = r.props.get(key) {
                    for name in split_names(Some(raw.as_str())) {
                        if !known_set.contains(name.as_str()) {
                            warnings.push(format!(
                                "stylesheet {} {key}: {name:?} is not a known validator",
                                r.selector
                            ));
                        }
                    }
                }
            }
        }
        warnings
    }
}

pub fn parse_stylesheet(text: &str) -> Option<Stylesheet> {
    let sheet = Stylesheet::parse(text);
    if sheet.rules.is_empty() {
        None
    } else {
        Some(sheet)
    }
}

pub fn split_names(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}
