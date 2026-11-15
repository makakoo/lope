//! S3 provider acceptance tests (`A8`):
//! - Port of `tests/test_curl_parser.py`
//! - Port of `tests/test_http_deadline.py` (with 0 child processes spawned)
//! - Port of `tests/test_makakoo_adapter.py`

use lope_providers::builtin::{
    enumerate_registered_adapters_in, hydrate_makakoo_result, MakakooAdapter,
};
use lope_providers::curl::{
    curl_to_provider_entry, parse_curl, suggest_env_name, ParsedCurl,
};
use lope_providers::generic::{validate_provider_config, Http, Validator};
use lope_providers::models::VerdictStatus;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

// ─── test_curl_parser.py ────────────────────────────────────────────────────

#[test]
fn curl_basic_post_with_headers_and_data() {
    let curl = concat!(
        "curl https://api.openai.com/v1/chat/completions ",
        "-H 'Authorization: Bearer ${OPENAI_API_KEY}' ",
        "-H 'Content-Type: application/json' ",
        r#"-d '{"model":"gpt-4","messages":[{"role":"user","content":"hi"}]}'"#
    );
    let parsed = parse_curl(curl).unwrap();
    assert_eq!(parsed.url, "https://api.openai.com/v1/chat/completions");
    assert_eq!(parsed.method, "POST");
    assert!(parsed
        .headers
        .contains(&("Authorization".into(), "Bearer ${OPENAI_API_KEY}".into())));
    assert!(parsed
        .headers
        .contains(&("Content-Type".into(), "application/json".into())));
    let data: serde_json::Value =
        serde_json::from_str(parsed.data.as_deref().unwrap()).unwrap();
    assert_eq!(data["model"], "gpt-4");
}

#[test]
fn curl_line_continuation_and_variants() {
    let curl = "curl https://api.example.com/v1 \\\n            -H 'X-Auth: ${KEY}' \\\n            -d '{\"prompt\":\"hi\"}'";
    let parsed = parse_curl(curl).unwrap();
    assert_eq!(parsed.url, "https://api.example.com/v1");
    assert!(parsed.headers.contains(&("X-Auth".into(), "${KEY}".into())));

    for flag in ["-d", "--data", "--data-raw", "--data-ascii"] {
        let c = format!(
            "curl https://api.example.com -H 'A: ${{K}}' {flag} '{{\"prompt\":\"x\"}}'"
        );
        let p = parse_curl(&c).unwrap();
        assert_eq!(p.data.as_deref(), Some("{\"prompt\":\"x\"}"));
    }

    let c2 = concat!(
        "curl -o /tmp/out.json --connect-timeout 30 ",
        "https://api.example.com/v1 ",
        "-H 'X: ${K}' ",
        r#"-d '{"prompt":"hi"}'"#
    );
    let p2 = parse_curl(c2).unwrap();
    assert_eq!(p2.url, "https://api.example.com/v1");
}

#[test]
fn curl_refusals() {
    assert!(parse_curl("").is_err());
    assert!(parse_curl("wget https://example.com")
        .unwrap_err()
        .contains("expected a curl"));
    assert!(parse_curl("curl -H 'X: y' -d '{}'")
        .unwrap_err()
        .contains("no URL"));
    assert!(parse_curl("curl -X GET https://api.example.com/v1")
        .unwrap_err()
        .contains("only POST"));
    assert!(parse_curl("curl -u user:pass https://api.example.com/v1 -d '{\"x\":1}'")
        .unwrap_err()
        .contains("basic auth"));
    assert!(parse_curl("curl -F 'file=@foo.png' https://api.example.com/v1")
        .unwrap_err()
        .contains("multipart"));
    assert!(
        parse_curl("curl https://api.example.com/v1 --data-binary @body.json")
            .unwrap_err()
            .contains("not supported")
    );
    assert!(
        parse_curl("curl https://api.example.com/v1 -H 'no-colon-here' -d '{}'")
            .unwrap_err()
            .contains("malformed header")
    );
    assert!(parse_curl("curl https://api.example.com/v1 -d '{unclosed")
        .unwrap_err()
        .contains("unclosed quote"));
}

#[test]
fn curl_suggest_env_name() {
    assert_eq!(
        suggest_env_name("https://api.openai.com/v1/chat/completions"),
        "OPENAI_API_KEY"
    );
    assert_eq!(
        suggest_env_name("https://api.anthropic.com/v1/messages"),
        "ANTHROPIC_API_KEY"
    );
    assert_eq!(
        suggest_env_name("https://api.groq.com/openai/v1/chat/completions"),
        "GROQ_API_KEY"
    );
    assert_eq!(
        suggest_env_name("http://10.42.42.1:18080/v1/chat/completions"),
        "API_KEY"
    );
    assert_eq!(suggest_env_name(""), "API_KEY");
}

#[test]
fn curl_prompt_injection_and_credentials_and_response_path() {
    let body = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "system", "content": "Be concise."},
            {"role": "user", "content": "first"},
            {"role": "assistant", "content": "ok"},
            {"role": "user", "content": "second"},
        ],
    });
    let curl = format!(
        "curl https://api.example.com/v1 -H 'X-API-Key: ${{K}}' -H 'Content-Type: application/json' -d '{}'",
        serde_json::to_string(&body).unwrap()
    );
    let parsed = parse_curl(&curl).unwrap();
    let entry =
        curl_to_provider_entry("x", &parsed, None, None, None, None, None).unwrap();
    let msgs = entry["body"]["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["content"], "Be concise.");
    assert_eq!(msgs[1]["content"], "first");
    assert_eq!(msgs[2]["content"], "ok");
    assert_eq!(msgs[3]["content"], "{prompt}");

    // Cohere response_path + message injection
    let cohere_curl = concat!(
        "curl https://api.cohere.ai/v1/chat ",
        "-H 'Authorization: Bearer ${COHERE_API_KEY}' ",
        r#"-d '{"model":"command-r","message":"hi"}'"#
    );
    let cp = parse_curl(cohere_curl).unwrap();
    let ce =
        curl_to_provider_entry("cohere", &cp, None, None, None, None, None).unwrap();
    assert_eq!(ce["body"]["message"], "{prompt}");
    assert_eq!(ce["response_path"], "text");

    // Anthropic by version header
    let anthro_curl = concat!(
        "curl https://proxy.example.com/anthro ",
        "-H 'x-api-key: ${K}' ",
        "-H 'anthropic-version: 2023-06-01' ",
        r#"-d '{"model":"m","messages":[{"role":"user","content":"hi"}]}'"#
    );
    let ap = parse_curl(anthro_curl).unwrap();
    let ae = curl_to_provider_entry("x", &ap, None, None, None, None, None).unwrap();
    assert_eq!(ae["response_path"], "content.0.text");

    // Literal credential refused without key_env, swapped with key_env
    let lit_curl = concat!(
        "curl https://api.openai.com/v1/chat/completions ",
        "-H 'Authorization: Bearer sk-proj-ABCDEFG1234567890' ",
        r#"-d '{"model":"m","messages":[{"role":"user","content":"hi"}]}'"#
    );
    let lp = parse_curl(lit_curl).unwrap();
    let err =
        curl_to_provider_entry("openai", &lp, None, None, None, None, None).unwrap_err();
    assert!(err.contains("literal credential"));
    assert!(err.contains("OPENAI_API_KEY"));

    let swapped = curl_to_provider_entry(
        "openai",
        &lp,
        Some("OPENAI_API_KEY"),
        None,
        Some("Be terse: {prompt}"),
        Some(120),
        None,
    )
    .unwrap();
    assert_eq!(
        swapped["headers"]["Authorization"],
        "Bearer ${OPENAI_API_KEY}"
    );
    assert_eq!(swapped["prompt_wrapper"], "Be terse: {prompt}");
    assert_eq!(swapped["timeout"], 120);
    validate_provider_config(&swapped).unwrap();

    // No body error
    let no_body = ParsedCurl {
        url: "https://api.example.com/v1".into(),
        method: "POST".into(),
        headers: vec![("X-Auth".into(), "${K}".into())],
        data: None,
    };
    assert!(
        curl_to_provider_entry("x", &no_body, None, None, None, None, None)
            .unwrap_err()
            .contains("no request body")
    );
}

// ─── test_http_deadline.py (Tier 1 in-process HTTP, 0 child processes) ──────

#[test]
fn http_tier1_happy_path_and_response_path() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let body = r#"{"choices":[{"message":{"content":"hello from tier1"}}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let cfg = json!({
        "name": "mock-http",
        "type": "http",
        "url": format!("http://{addr}/v1/chat/completions"),
        "headers": {"Content-Type": "application/json"},
        "body": {"model": "m", "messages": [{"role": "user", "content": "{prompt}"}]},
        "response_path": "choices.0.message.content"
    });
    let h = Http::from_config(&cfg).unwrap();
    assert!(h.unsupported().is_none());
    let out = h.generate("ping", 5.0, None).unwrap();
    assert_eq!(out, "hello from tier1");
    let _ = handle.join();
}

#[test]
fn http_tier1_content_length_preflight_cap() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 99999999\r\nConnection: close\r\n\r\n{}";
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let cfg = json!({
        "name": "cap-http",
        "type": "http",
        "url": format!("http://{addr}/v1"),
        "body": {"prompt": "{prompt}"},
        "response_path": "text"
    });
    let h = Http::from_config(&cfg).unwrap();
    let err = h.generate("hi", 5.0, None).unwrap_err().to_string();
    assert!(
        err.contains("cap-http http error: ValueError: response Content-Length exceeds"),
        "unexpected error: {err}"
    );
    let _ = handle.join();
}

#[test]
fn http_tier1_wall_clock_deadline_enforced() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n",
            );
            std::thread::sleep(Duration::from_millis(1200));
        }
    });

    let cfg = json!({
        "name": "slow-http",
        "type": "http",
        "url": format!("http://{addr}/v1"),
        "body": {"prompt": "{prompt}"},
        "response_path": "a"
    });
    let h = Http::from_config(&cfg).unwrap();
    let t0 = Instant::now();
    let err = h.generate("hi", 0.3, None).unwrap_err().to_string();
    let elapsed = t0.elapsed();
    assert!(
        elapsed < Duration::from_millis(1000),
        "deadline not enforced in time: {elapsed:?}"
    );
    assert!(
        err.contains("slow-http http error: timed out after 0.3s"),
        "unexpected error: {err}"
    );
    let _ = handle.join();
}

// ─── test_makakoo_adapter.py ────────────────────────────────────────────────

#[test]
fn makakoo_enumerate_and_hydrate() {
    let dir = std::env::temp_dir().join(format!(
        "lope-makakoo-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let reg = dir.join("registered");
    std::fs::create_dir_all(&reg).unwrap();
    std::fs::write(reg.join("zeta.toml"), "name = 'zeta'\n").unwrap();
    std::fs::write(reg.join("alpha.toml"), "name = 'alpha'\n").unwrap();
    std::fs::write(reg.join("ignored.json"), "{}\n").unwrap();

    let names = enumerate_registered_adapters_in(Some(&dir), true);
    assert_eq!(names, vec!["alpha".to_string(), "zeta".to_string()]);
    assert!(enumerate_registered_adapters_in(Some(&dir), false).is_empty());
    let _ = std::fs::remove_dir_all(&dir);

    // Hydrate PASS with evidence
    let payload = json!({
        "validator_name": "alpha",
        "verdict": {
            "status": "PASS",
            "confidence": 0.92,
            "rationale": "All tests passed in src/main.rs:42",
            "required_fixes": [],
            "nice_to_have": ["doc tweak"],
            "duration_seconds": 1.25
        },
        "raw_response": "---VERDICT---\nstatus: PASS\n---END---",
        "error": "",
        "flag_error_hint": ""
    });
    let vr =
        hydrate_makakoo_result(&serde_json::to_string(&payload).unwrap(), "alpha", 2.0);
    assert_eq!(vr.validator_name, "alpha");
    assert_eq!(vr.verdict.status, VerdictStatus::Pass);
    assert!((vr.verdict.confidence - 0.92).abs() < 1e-9);
    assert_eq!(vr.verdict.nice_to_have, vec!["doc tweak".to_string()]);

    // Malformed payload rejected -> INFRA_ERROR
    let bad = hydrate_makakoo_result("not json", "alpha", 1.0);
    assert_eq!(bad.verdict.status, VerdictStatus::InfraError);
    assert!(bad.error.contains("invalid JSON"));

    let adapter = MakakooAdapter::new("alpha");
    assert_eq!(adapter.name(), "alpha");
}
