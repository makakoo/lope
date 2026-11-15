//! Validator sources (S3), ported from Python Lope with byte-identical
//! messages. See docs/sprints/HANDOFF.md for what is native and what still
//! makes a verb delegate.

// ProviderError carries UNVERIFIED recovery fields by design.
#![allow(clippy::result_large_err)]

pub mod budget;
pub mod builtin;
pub mod curl;
pub mod diagnostics;
pub mod errors;
pub mod generic;
pub mod invocation;
pub mod latency;
pub mod models;
pub mod pool;
pub mod progress;
pub mod pyre;
pub mod request_plan;
pub mod retry;
pub mod validity;
pub mod verdict;
