//! Provider error kinds (`provider_errors.py`) and `tool_permission_denial`.

use crate::pyre::{re, search};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// ProviderInfrastructureError
    Infra(String),
    /// ProviderToolPermissionError (terminal, never failover)
    ToolPermission(String),
    /// RuntimeError / generic
    Runtime(String),
    /// ProcessCompletionError (UNVERIFIED), with recovery fields
    Unverified { message: String, stdout: String, cleanup: String, stdout_complete: bool, stdout_truncated: bool, returncode: serde_json::Value },
    /// validator cannot draft (`NotImplementedError`)
    NotImplemented(String),
    /// BudgetExhausted(reason, message)
    Budget { reason: String, message: String },
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderError::Infra(m) | ProviderError::ToolPermission(m) | ProviderError::Runtime(m) | ProviderError::NotImplemented(m) => f.write_str(m),
            ProviderError::Unverified { message, .. } => f.write_str(message),
            ProviderError::Budget { message, .. } => f.write_str(message),
        }
    }
}

pub fn tool_permission_denial(stderr: &str) -> String {
    static R: OnceLock<fancy_regex::Regex> = OnceLock::new();
    let r = R.get_or_init(|| {
        re(r#"(?im)(?:^|;\s*stderr:\s*)jetski: no output produced[^\n]*?a tool required the "(?P<permission>[a-z][a-z0-9_-]{0,63})" permission that headless mode cannot prompt for, so it was auto-denied\."#)
    });
    search(r, stderr).map(|c| c.name("permission").unwrap().as_str().to_lowercase()).unwrap_or_default()
}
