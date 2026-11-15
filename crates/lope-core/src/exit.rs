//! Exit codes (FR3).

pub const OK: i32 = 0;
pub const FAILURE: i32 = 1;
/// Usage error (argparse convention) or nested-orchestration refusal.
pub const USAGE: i32 = 2;
/// Untrusted gate commands, or `--manual` without a TTY.
pub const UNTRUSTED_OR_NO_TTY: i32 = 3;
/// Run lock held.
pub const RUN_LOCK_HELD: i32 = 75;
/// Run deadline exhausted, or installer timeout.
pub const DEADLINE: i32 = 124;
/// The interpreter exists but cannot be executed (shell convention).
pub const CANNOT_EXECUTE: i32 = 126;
/// The interpreter was not found (shell convention).
pub const NOT_FOUND: i32 = 127;
