//! Tier 2: supervised one-shot provider processes over pipes (DEC-2), with
//! Python Lope's fail-closed completion policy and a realistic cleanup
//! budget (DEC-4, Spec 01 §7).
//!
//! Layout mirrors Python: the caller ([`call::run_subprocess_group`]) starts
//! a supervisor process (this same binary, argv0 `lope-supervisor`), sends it
//! one JSON spec line and keeps the pipe open. The supervisor owns the
//! provider's process group; pipe EOF means the owner died, so the group is
//! killed even if `lope` itself is SIGKILLed.

pub mod bounded;
pub mod call;
pub mod diag;
pub mod platform;
pub mod signals;
pub mod supervisor;

/// Default cleanup budget (seconds) for confirming the provider group is
/// empty. Python uses 0.7 s; see DEC-4 and docs/evidence/s2-runtime.txt.
pub const DEFAULT_CLEANUP_BUDGET_SECONDS: f64 = 2.0;

/// `LOPE_CLEANUP_BUDGET_SECONDS` (additive, Lope v2 only) or the default.
pub fn cleanup_budget() -> f64 {
    std::env::var("LOPE_CLEANUP_BUDGET_SECONDS")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0 && *v <= 30.0)
        .unwrap_or(DEFAULT_CLEANUP_BUDGET_SECONDS)
}
