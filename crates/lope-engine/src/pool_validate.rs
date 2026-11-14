//! Two-tier validator pool execution + ensemble synthesis (`ensemble.py` & `validators.py`, Sprint S7).

use crate::models::{new_validator_result, PhaseVerdict, ValidatorResult, VerdictStatus};
use crate::verdict_repair::{
    build_repair_prompt, evaluate_repair_reply, RepairStatus, DEFAULT_REPAIR_TIMEOUT,
    ENV_REPAIR_TIMEOUT,
};
use lope_providers::budget::Context;
use lope_providers::generic::Validator;
use lope_providers::invocation::invoke_generate;
use lope_providers::pool::Pool;
use lope_providers::verdict::{infra_error, parse_verdict};
use std::collections::HashSet;
use std::time::Instant;

pub fn ordered_validators(pool: &Pool) -> Vec<std::sync::Arc<dyn Validator>> {
    if pool.ensemble || pool.primary.is_empty() {
        return pool.validators.clone();
    }
    let mut out = Vec::with_capacity(pool.validators.len());
    for v in &pool.validators {
        if v.name() == pool.primary {
            out.push(v.clone());
        }
    }
    for v in &pool.validators {
        if v.name() != pool.primary {
            out.push(v.clone());
        }
    }
    out
}

pub fn validate_one(
    validator: &dyn Validator,
    prompt: &str,
    timeout: f64,
    ctx: Option<&Context>,
    stage: &str,
) -> ValidatorResult {
    let start = Instant::now();
    match invoke_generate(validator, prompt, timeout, ctx, Some(stage)) {
        Ok(raw) => {
            let elapsed = start.elapsed().as_secs_f64();
            let verdict = parse_verdict(&raw, validator.name(), elapsed);
            let mut res = new_validator_result(validator.name(), verdict);
            res.raw_response = raw.clone();

            if res.verdict.status == VerdictStatus::InfraError && !raw.trim().is_empty() {
                res.initial_parse_status = Some("INFRA_ERROR".to_string());
                if let Ok(repair_prompt) = build_repair_prompt(&raw, validator.name()) {
                    let repair_timeout = std::env::var(ENV_REPAIR_TIMEOUT)
                        .ok()
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|t| *t > 0.0)
                        .unwrap_or(DEFAULT_REPAIR_TIMEOUT)
                        .min(timeout.max(1.0));
                    res.repair_attempted = true;
                    match invoke_generate(
                        validator,
                        &repair_prompt,
                        repair_timeout,
                        ctx,
                        Some("verdict_repair"),
                    ) {
                        Ok(repair_raw) => {
                            let outcome = evaluate_repair_reply(&repair_raw, validator.name(), elapsed);
                            res.repair_status = Some(outcome.status.as_str().to_string());
                            res.repaired_response = repair_raw;
                            if outcome.status == RepairStatus::Accepted {
                                if let Some(v) = outcome.verdict {
                                    res.verdict = v;
                                }
                            }
                        }
                        Err(err) => {
                            res.repair_status = Some(RepairStatus::RejectedProcess.as_str().to_string());
                            res.repaired_response = err.to_string();
                        }
                    }
                }
            }
            res
        }
        Err(err) => {
            let msg = err.to_string();
            let mut res = infra_error(validator.name(), &msg, start.elapsed().as_secs_f64());
            let lower = msg.to_lowercase();
            if lower.contains("unknown")
                || lower.contains("unrecognized")
                || lower.contains("invalid option")
                || lower.contains("flag")
            {
                res.flag_error_hint = msg.chars().take(300).collect();
            }
            res
        }
    }
}

pub fn pool_validate(
    pool: &Pool,
    prompt: &str,
    timeout: f64,
    ctx: Option<&Context>,
    stage: &str,
) -> ValidatorResult {
    if pool.ensemble {
        let available: Vec<_> = pool
            .validators
            .iter()
            .filter(|v| v.available())
            .cloned()
            .collect();
        if available.is_empty() {
            let names: Vec<_> = pool.validators.iter().map(|v| v.name()).collect();
            return infra_error(
                "ensemble",
                &format!("no validators available in pool: {names:?}"),
                0.0,
            );
        }

        let results = if pool.parallel && available.len() > 1 {
            std::thread::scope(|s| {
                let handles: Vec<_> = available
                    .iter()
                    .map(|v| {
                        let v = v.clone();
                        s.spawn(move || validate_one(v.as_ref(), prompt, timeout, ctx, stage))
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| match h.join() {
                        Ok(r) => r,
                        Err(_) => infra_error("ensemble", "validator worker thread panicked", 0.0),
                    })
                    .collect::<Vec<_>>()
            })
        } else {
            available
                .iter()
                .map(|v| validate_one(v.as_ref(), prompt, timeout, ctx, stage))
                .collect()
        };

        let primary = if pool.primary.is_empty() {
            None
        } else {
            Some(pool.primary.as_str())
        };
        return synthesize(&results, primary, Some(available.len()));
    }

    let ordered = ordered_validators(pool);
    let mut last_error = String::new();
    let mut attempts = Vec::new();
    for v in ordered {
        if !v.available() {
            attempts.push(format!("{}:unavailable", v.name()));
            continue;
        }
        attempts.push(v.name().to_string());
        let res = validate_one(v.as_ref(), prompt, timeout, ctx, stage);
        if !matches!(
            res.verdict.status,
            VerdictStatus::InfraError | VerdictStatus::Inconclusive
        ) {
            return res;
        }
        last_error = if res.error.is_empty() {
            res.verdict.rationale.clone()
        } else {
            res.error.clone()
        };
    }

    let attempt_str = if attempts.is_empty() {
        "none".to_string()
    } else {
        attempts.join(",")
    };
    let tail: String = last_error.chars().take(300).collect();
    infra_error(
        "pool",
        &format!("all validators exhausted ({attempt_str}). Last error: {tail}"),
        0.0,
    )
}

pub fn synthesize(
    results: &[ValidatorResult],
    primary: Option<&str>,
    expected_count: Option<usize>,
) -> ValidatorResult {
    let mut decisive = Vec::new();
    let mut infra_errors = Vec::new();

    for r in results {
        let is_decisive = r.error.is_empty()
            && matches!(
                r.verdict.status,
                VerdictStatus::Pass | VerdictStatus::NeedsFix | VerdictStatus::Fail
            )
            && (!r.raw_response.trim().is_empty() || !r.verdict.rationale.trim().is_empty());
        if is_decisive {
            decisive.push(r);
        } else {
            infra_errors.push(r);
        }
    }

    let expected = expected_count.unwrap_or(results.len());
    let quorum = expected / 2 + 1;

    if decisive.len() < quorum {
        let mut reasons = Vec::new();
        for r in &infra_errors {
            let reason = if !r.error.is_empty() {
                r.error.as_str()
            } else if !r.verdict.rationale.is_empty() {
                r.verdict.rationale.as_str()
            } else {
                "invalid or empty output"
            };
            let head: String = reason.chars().take(160).collect();
            reasons.push(format!("{}: {head}", r.validator_name));
        }
        let reason_text = if reasons.is_empty() {
            "no substantive validator output".to_string()
        } else {
            reasons.join("; ")
        };
        let mut res = new_validator_result(
            "ensemble",
            PhaseVerdict::new(
                VerdictStatus::Inconclusive,
                format!(
                    "substantive-result quorum not met: {}/{quorum} required from {expected}; {reason_text}",
                    decisive.len()
                ),
                0.0,
                "ensemble",
            ),
        );
        res.error = "inconclusive: substantive-result quorum not met".to_string();
        return res;
    }

    if decisive.is_empty() {
        let last_err = infra_errors
            .last()
            .map(|l| {
                if !l.error.is_empty() {
                    l.error.clone()
                } else {
                    l.verdict.rationale.clone()
                }
            })
            .unwrap_or_else(|| "all validators failed".to_string());
        let head: String = last_err.chars().take(300).collect();
        let mut res = new_validator_result(
            "ensemble",
            PhaseVerdict::new(
                VerdictStatus::InfraError,
                format!("all validators infra error: {head}"),
                0.0,
                "ensemble",
            ),
        );
        res.error = last_err;
        return res;
    }

    let mut pass_votes = 0usize;
    let mut fix_votes = 0usize;
    let mut fail_votes = 0usize;
    for r in &decisive {
        match r.verdict.status {
            VerdictStatus::Pass => pass_votes += 1,
            VerdictStatus::NeedsFix => fix_votes += 1,
            VerdictStatus::Fail => fail_votes += 1,
            _ => {}
        }
    }

    let final_status = if fail_votes > 0 {
        VerdictStatus::Fail
    } else if pass_votes > fix_votes {
        VerdictStatus::Pass
    } else {
        VerdictStatus::NeedsFix
    };

    let conf_vals: Vec<f64> = decisive
        .iter()
        .map(|r| r.verdict.confidence)
        .filter(|c| *c > 0.0)
        .collect();
    let confidence = if conf_vals.is_empty() {
        0.0
    } else {
        conf_vals.iter().sum::<f64>() / conf_vals.len() as f64
    };

    let mut all_fixes = Vec::new();
    let mut seen = HashSet::new();
    for r in &decisive {
        if r.verdict.status == VerdictStatus::NeedsFix {
            for fix in &r.verdict.required_fixes {
                if seen.insert(fix.clone()) {
                    all_fixes.push(fix.clone());
                }
            }
        }
    }

    let primary_rationale = primary
        .and_then(|p| {
            decisive
                .iter()
                .find(|r| r.validator_name == p && !r.verdict.rationale.is_empty())
                .map(|r| {
                    let head: String = r.verdict.rationale.chars().take(200).collect();
                    format!(" Primary ({p}): {head}")
                })
        })
        .unwrap_or_default();

    let rationale = format!(
        "Ensemble ({} validators): PASS={pass_votes} NEEDS_FIX={fix_votes} FAIL={fail_votes}.{primary_rationale}",
        decisive.len()
    );

    let mut verdict = PhaseVerdict::new(final_status, rationale, 0.0, "ensemble");
    verdict.confidence = confidence;
    verdict.required_fixes = all_fixes;
    new_validator_result("ensemble", verdict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesize_majority_and_fail_veto() {
        let mut p1 = new_validator_result("a", PhaseVerdict::new(VerdictStatus::Pass, "ok", 1.0, "a"));
        p1.verdict.confidence = 0.9;
        p1.raw_response = "VERDICT: PASS".into();
        let mut p2 = new_validator_result("b", PhaseVerdict::new(VerdictStatus::NeedsFix, "fix x", 1.0, "b"));
        p2.verdict.confidence = 0.7;
        p2.verdict.required_fixes = vec!["fix x".into()];
        p2.raw_response = "VERDICT: NEEDS_FIX".into();
        let mut p3 = new_validator_result("c", PhaseVerdict::new(VerdictStatus::Pass, "good", 1.0, "c"));
        p3.verdict.confidence = 0.8;
        p3.raw_response = "VERDICT: PASS".into();

        let res = synthesize(&[p1.clone(), p2.clone(), p3], Some("a"), Some(3));
        assert_eq!(res.verdict.status, VerdictStatus::Pass);
        assert!((res.verdict.confidence - 0.8).abs() < 1e-6);

        let mut fail = new_validator_result("d", PhaseVerdict::new(VerdictStatus::Fail, "bad", 1.0, "d"));
        fail.raw_response = "VERDICT: FAIL".into();
        let vetoed = synthesize(&[p1, p2, fail], Some("a"), Some(3));
        assert_eq!(vetoed.verdict.status, VerdictStatus::Fail);
    }

    #[test]
    fn synthesize_quorum_inconclusive() {
        let p1 = new_validator_result("a", PhaseVerdict::new(VerdictStatus::Pass, "ok", 1.0, "a"));
        let e1 = infra_error("b", "timeout", 0.0);
        let e2 = infra_error("c", "timeout", 0.0);
        let res = synthesize(&[p1, e1, e2], None, Some(3));
        assert_eq!(res.verdict.status, VerdictStatus::Inconclusive);
    }
}
