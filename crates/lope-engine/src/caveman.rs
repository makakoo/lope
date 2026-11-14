//! Port of `lope/caveman.py`: token-efficient validator communication.

pub const CAVEMAN_VALIDATOR_DIRECTIVE: &str = "\
TOKEN EFFICIENCY: respond terse. Drop articles (a/an/the), filler (just/really/\
basically), hedging (I think/perhaps/might), pleasantries (sure/happy to help). \
Fragments OK. Keep exact: code, paths, line numbers, commands, error messages. \
Rationale = 1-3 short sentences, not paragraphs. \
Pattern: [thing] [action] [reason]. No preamble.";

pub const CAVEMAN_LITE_DIRECTIVE: &str = "\
Be concise. No filler or hedging. Keep technical precision. \
Rationale = 2-3 sentences max.";

pub fn get_directive() -> &'static str {
    let m = std::env::var("LOPE_CAVEMAN")
        .unwrap_or_else(|_| "full".into())
        .to_ascii_lowercase();
    match m.as_str() {
        "off" => "",
        "lite" => CAVEMAN_LITE_DIRECTIVE,
        _ => CAVEMAN_VALIDATOR_DIRECTIVE,
    }
}
