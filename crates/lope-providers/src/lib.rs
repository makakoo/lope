//! Validator sources (S3), ported from Python Lope with byte-identical
//! messages. Done so far: models, VERDICT parser, diagnostics redaction,
//! provider errors, output validity, retry policy. See
//! docs/sprints/HANDOFF.md for the remaining port order.

pub mod diagnostics;
pub mod errors;
pub mod models;
pub mod pyre;
pub mod retry;
pub mod validity;
pub mod verdict;
