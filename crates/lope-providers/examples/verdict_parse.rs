//! Differential helper: JSON string per line → parsed verdict as JSON.
use std::io::BufRead;
fn main() {
    for line in std::io::stdin().lock().lines() {
        let t: String = serde_json::from_str(&line.unwrap()).unwrap();
        let v = lope_providers::verdict::parse_verdict(&t, "mock", 0.0);
        let red = lope_providers::diagnostics::redact_diagnostic(&t);
        println!("{}", serde_json::json!({
            "status": v.status.as_str(), "confidence": v.confidence, "rationale": v.rationale,
            "required_fixes": v.required_fixes, "nice_to_have": v.nice_to_have,
            "evidence": v.evidence_gate_triggered, "redacted": red,
            "substantive": lope_providers::validity::substantive(&t),
        }));
    }
}
