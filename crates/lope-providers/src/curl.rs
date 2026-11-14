//! Port of `curl_parser.py`: parse a pasted `curl` command into a lope HTTP provider entry.
//!
//! Exports `parse_curl`, `suggest_env_name`, `curl_to_provider_entry`, and
//! `parse_curl_command` with byte-identical error strings and output JSON to Python.

use crate::pyre;
use lope_core::pyjson;
use lope_state::registry::py_repr_str;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCurl {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub data: Option<String>,
}

fn preclean(curl_str: &str) -> String {
    let re = pyre::re(r"\\\s*\n\s*");
    re.replace_all(curl_str, " ").into_owned()
}

/// POSIX `shlex.split(s, posix=True)`.
pub fn shlex_split_strict(s: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            Some('"') => {
                if c == '"' {
                    quote = None;
                } else if c == '\\' {
                    match chars.peek().copied() {
                        Some(nc @ ('$' | '`' | '"' | '\\' | '\n')) => {
                            let _ = chars.next();
                            if nc != '\n' {
                                cur.push(nc);
                            }
                        }
                        Some(_) => {
                            cur.push('\\');
                        }
                        None => return Err("No escaped character".into()),
                    }
                } else {
                    cur.push(c);
                }
            }
            Some(_) => unreachable!(),
            None => {
                if c.is_ascii_whitespace() {
                    if in_token {
                        tokens.push(std::mem::take(&mut cur));
                        in_token = false;
                    }
                } else if c == '\'' || c == '"' {
                    quote = Some(c);
                    in_token = true;
                } else if c == '\\' {
                    match chars.next() {
                        Some('\n') => {}
                        Some(nc) => {
                            in_token = true;
                            cur.push(nc);
                        }
                        None => return Err("No escaped character".into()),
                    }
                } else if c == '#' && !in_token {
                    for nc in chars.by_ref() {
                        if nc == '\n' {
                            break;
                        }
                    }
                } else {
                    in_token = true;
                    cur.push(c);
                }
            }
        }
    }

    if quote.is_some() {
        return Err("No closing quotation".into());
    }
    if in_token {
        tokens.push(cur);
    }
    Ok(tokens)
}

/// `curl_parser.py:parse_curl(curl_str)`
pub fn parse_curl(curl_str: &str) -> Result<ParsedCurl, String> {
    if curl_str.trim().is_empty() {
        return Err("empty curl string".into());
    }
    let cleaned = preclean(curl_str);
    let tokens = shlex_split_strict(&cleaned)
        .map_err(|e| format!("could not parse curl — unclosed quote? ({e})"))?;
    if tokens.is_empty() {
        return Err("no tokens in curl string".into());
    }
    if tokens[0] != "curl" {
        return Err(format!(
            "expected a curl command; got {} as the first token",
            py_repr_str(&tokens[0])
        ));
    }

    let mut url: Option<String> = None;
    let mut method: Option<String> = None;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut data: Option<String> = None;

    let mut i = 1usize;
    let n = tokens.len();

    let need_arg = |flag: &str, idx: &mut usize| -> Result<String, String> {
        if *idx + 1 >= n {
            return Err(format!("{flag} expects an argument, got end of string"));
        }
        let val = tokens[*idx + 1].clone();
        *idx += 2;
        Ok(val)
    };

    while i < n {
        let t = tokens[i].as_str();
        if matches!(t, "-X" | "--request") {
            method = Some(need_arg(t, &mut i)?);
            continue;
        }
        if matches!(t, "-H" | "--header") {
            let header_val = need_arg(t, &mut i)?;
            let Some((k, v)) = header_val.split_once(':') else {
                return Err(format!(
                    "malformed header {} — expected `Name: value`",
                    py_repr_str(&header_val)
                ));
            };
            let k_str = k.trim().to_string();
            let v_str = v.trim().to_string();
            if let Some(pos) = headers.iter().position(|(ek, _)| ek == &k_str) {
                headers[pos].1 = v_str;
            } else {
                headers.push((k_str, v_str));
            }
            continue;
        }
        if matches!(
            t,
            "-d" | "--data" | "--data-raw" | "--data-ascii" | "--data-urlencode"
        ) {
            let val = need_arg(t, &mut i)?;
            if val.starts_with('@') {
                return Err(format!(
                    "{t} @file is not supported — paste the body inline instead"
                ));
            }
            data = Some(val);
            continue;
        }
        if t == "--data-binary" {
            let val = need_arg(t, &mut i)?;
            if val.starts_with('@') {
                return Err("--data-binary @file is not supported — paste the body inline".into());
            }
            data = Some(val);
            continue;
        }
        if matches!(t, "-u" | "--user") {
            return Err(
                "basic auth (-u/--user) is not supported — pre-encode the credential with `echo -n user:pass | base64` and paste the result into an Authorization header using ${VAR} substitution.".into(),
            );
        }
        if matches!(t, "-F" | "--form") {
            return Err(
                "multipart form data (-F/--form) is not supported — only JSON bodies with -d/--data/--data-raw".into(),
            );
        }
        if matches!(
            t,
            "-s" | "-S"
                | "--silent"
                | "--show-error"
                | "-v"
                | "--verbose"
                | "-i"
                | "--include"
                | "-L"
                | "--location"
                | "-k"
                | "--insecure"
                | "--compressed"
                | "--fail"
                | "-f"
        ) {
            i += 1;
            continue;
        }
        if matches!(
            t,
            "-o" | "--output"
                | "-A"
                | "--user-agent"
                | "-e"
                | "--referer"
                | "-b"
                | "--cookie"
                | "--connect-timeout"
                | "-m"
                | "--max-time"
                | "--cacert"
                | "--cert"
                | "--key"
                | "-w"
                | "--write-out"
        ) {
            let _ = need_arg(t, &mut i)?;
            continue;
        }
        if t.starts_with("--") && t.contains('=') {
            i += 1;
            continue;
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        if url.is_none() {
            url = Some(t.to_string());
        }
        i += 1;
    }

    let Some(url) = url else {
        return Err("no URL found in curl command".into());
    };
    let method = method.unwrap_or_else(|| {
        if data.is_some() {
            "POST".into()
        } else {
            "GET".into()
        }
    });
    if !method.eq_ignore_ascii_case("POST") {
        return Err(format!(
            "only POST is supported for chat endpoints (got {method}). Pass the body with -d/--data or use a different provider type."
        ));
    }

    Ok(ParsedCurl {
        url,
        method: "POST".into(),
        headers,
        data,
    })
}

const CREDENTIAL_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-access-token",
];

fn header_has_literal_credential(header_key: &str, header_val: &str) -> bool {
    if header_val.contains("${") {
        return false;
    }
    let lower = header_key.to_ascii_lowercase();
    CREDENTIAL_HEADERS.contains(&lower.as_str())
}

fn substitute_with_env(header_val: &str, env_var: &str) -> String {
    let re = pyre::re(r"(?i)^(bearer|basic|token)\b\s+(.+)$");
    if let Some(c) = pyre::search(&re, header_val) {
        let scheme = c.get(1).unwrap().as_str();
        return format!("{scheme} ${{{env_var}}}");
    }
    format!("${{{env_var}}}")
}

fn extract_hostname(url: &str) -> String {
    let rest = if let Some((_, r)) = url.split_once("://") {
        r
    } else {
        return String::new();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if host_port.starts_with('[') {
        host_port
            .split_once(']')
            .map(|(h, _)| &h[1..])
            .unwrap_or(host_port)
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    host.to_ascii_lowercase()
}

/// `curl_parser.py:suggest_env_name(url)`
pub fn suggest_env_name(url: &str) -> String {
    let host = extract_hostname(url);
    let parts: Vec<&str> = host
        .split('.')
        .filter(|p| {
            !p.is_empty()
                && *p != "api"
                && *p != "www"
                && !p.chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    if parts.is_empty() {
        return "API_KEY".into();
    }
    let candidate_parts = if parts.len() > 1 {
        &parts[..parts.len() - 1]
    } else {
        &parts[..]
    };
    for seg in candidate_parts {
        if seg.len() >= 3 {
            return format!("{}_API_KEY", seg.to_ascii_uppercase());
        }
    }
    format!("{}_API_KEY", candidate_parts[0].to_ascii_uppercase())
}

const SCALAR_PROMPT_KEYS: &[&str] = &["prompt", "input", "message", "query", "text"];

fn inject_prompt_placeholder(body: &Value) -> (Value, bool) {
    let Value::Object(map) = body else {
        return (body.clone(), false);
    };
    let mut new_map = map.clone();
    if let Some(Value::Array(messages)) = new_map.get_mut("messages") {
        if !messages.is_empty() {
            for msg in messages.iter_mut().rev() {
                if let Value::Object(m) = msg {
                    if m.get("role").and_then(Value::as_str) == Some("user") {
                        m.insert("content".into(), Value::String("{prompt}".into()));
                        return (Value::Object(new_map), true);
                    }
                }
            }
            if let Some(Value::Object(last)) = messages.last_mut() {
                if last.contains_key("content") {
                    last.insert("content".into(), Value::String("{prompt}".into()));
                    return (Value::Object(new_map), true);
                }
            }
        }
    }
    for &key in SCALAR_PROMPT_KEYS {
        if matches!(new_map.get(key), Some(Value::String(_))) {
            new_map.insert(key.into(), Value::String("{prompt}".into()));
            return (Value::Object(new_map), true);
        }
    }
    (Value::Object(new_map), false)
}

pub fn infer_response_path(url: &str, headers: &[(String, String)]) -> String {
    let host_lower = url.to_ascii_lowercase();
    let has_anthropic_ver = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("anthropic-version"));
    if host_lower.contains("anthropic") || has_anthropic_ver {
        return "content.0.text".into();
    }
    if host_lower.contains("cohere") {
        return "text".into();
    }
    "choices.0.message.content".into()
}

/// `curl_parser.py:curl_to_provider_entry`
pub fn curl_to_provider_entry(
    name: &str,
    parsed: &ParsedCurl,
    key_env: Option<&str>,
    response_path: Option<&str>,
    wrap: Option<&str>,
    timeout: Option<i64>,
    max_tokens: Option<i64>,
) -> Result<Value, String> {
    let url = &parsed.url;
    let mut headers = parsed.headers.clone();
    let data_raw = parsed.data.as_deref().unwrap_or("");
    if data_raw.is_empty() {
        return Err(
            "curl has no request body (-d/--data). A chat endpoint needs a JSON body to know what model to call and where the prompt goes.".into(),
        );
    }

    for (hkey, hval) in headers.iter_mut() {
        if header_has_literal_credential(hkey, hval) {
            if let Some(ke) = key_env.filter(|s| !s.is_empty()) {
                *hval = substitute_with_env(hval, ke);
            } else {
                let suggested = suggest_env_name(url);
                return Err(format!(
                    "Header {} in the pasted curl has a literal credential. Two ways to fix:\n  1. Before pasting, replace the credential with ${{{suggested}}} — e.g.      'Authorization: Bearer ${{{suggested}}}'.\n  2. Re-run with --key-env {suggested} (or your env var name) and lope will swap the literal for you.",
                    py_repr_str(hkey)
                ));
            }
        }
    }

    let (mut body, body_is_json) = match pyjson::loads(data_raw) {
        Ok(v) => (v, true),
        Err(_) => (Value::String(data_raw.to_string()), false),
    };

    let mut prompt_injected = false;
    if body_is_json {
        let (nb, inj) = inject_prompt_placeholder(&body);
        body = nb;
        prompt_injected = inj;
    }

    if !prompt_injected {
        let serialized = if body_is_json {
            pyjson::dumps(&body, &pyjson::Opts::default())
        } else {
            body.as_str().unwrap_or("").to_string()
        };
        if !serialized.contains("{prompt}") {
            return Err(
                "could not auto-detect where the user prompt belongs in the request body. Options:\n  1. Before pasting, replace the test prompt in the body with {prompt} — e.g. '{\"messages\":[{\"role\":\"user\",\"content\":\"{prompt}\"}]}'.\n  2. Use --body-json '<your JSON>' directly instead of --from-curl.".into(),
            );
        }
    }

    if let Some(mt) = max_tokens {
        if body_is_json {
            let serialized = pyjson::dumps(&body, &pyjson::Opts::default());
            if !serialized.contains("{max_tokens}") {
                if let Value::Object(ref mut m) = body {
                    m.insert("max_tokens".into(), Value::from(mt));
                }
            }
        }
    }

    let mut headers_map = Map::new();
    for (k, v) in &headers {
        headers_map.insert(k.clone(), Value::String(v.clone()));
    }

    let rp = response_path
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| infer_response_path(url, &headers));

    let mut entry = Map::new();
    entry.insert("name".into(), Value::String(name.to_string()));
    entry.insert("type".into(), Value::String("http".into()));
    entry.insert("url".into(), Value::String(url.clone()));
    entry.insert("headers".into(), Value::Object(headers_map));
    entry.insert("body".into(), body);
    entry.insert("response_path".into(), Value::String(rp));
    if let Some(w) = wrap.filter(|s| !s.is_empty()) {
        entry.insert("prompt_wrapper".into(), Value::String(w.to_string()));
    }
    if let Some(t) = timeout.filter(|&t| t != 0) {
        entry.insert("timeout".into(), Value::from(t));
    }
    if let Some(mt) = max_tokens {
        entry.insert("max_tokens".into(), Value::from(mt));
    }
    Ok(Value::Object(entry))
}

/// Convenience helper combining `parse_curl` + `curl_to_provider_entry`.
pub fn parse_curl_command(
    cmd_str: &str,
    name: &str,
    key_env: Option<&str>,
    response_path: Option<&str>,
    wrap: Option<&str>,
    timeout: Option<i64>,
    max_tokens: Option<i64>,
) -> Result<Value, String> {
    let parsed = parse_curl(cmd_str)?;
    curl_to_provider_entry(name, &parsed, key_env, response_path, wrap, timeout, max_tokens)
}
