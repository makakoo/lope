//! Compiles the argparse capture (grammar/argparse-python-lope.json, written by
//! tools/parity/dump-python) into static tables. Anything this generator does
//! not understand fails the build, so a Python grammar change cannot slip in
//! unnoticed.

use serde_json::Value;
use std::fmt::Write as _;
use std::{env, fs, path::Path};

const GRAMMAR: &str = "grammar/argparse-python-lope.json";

fn main() {
    println!("cargo:rerun-if-changed={GRAMMAR}");
    let text = fs::read_to_string(GRAMMAR).expect("read grammar");
    let doc: Value = serde_json::from_str(&text).expect("parse grammar");
    assert_eq!(doc["schema"], 1, "unsupported grammar schema");

    let mut out = String::new();
    writeln!(
        out,
        "pub const GRAMMAR_LOPE_VERSION: &str = {:?};",
        s(&doc["lope_version"])
    )
    .unwrap();
    writeln!(
        out,
        "pub const GRAMMAR_FNV64: u64 = {:#018x};",
        fnv64(text.as_bytes())
    )
    .unwrap();
    out.push_str("pub static ROOT: CommandSpec = ");
    command(&mut out, "lope", &[], None, &doc["parser"]);
    out.push_str(";\n");

    let dest = Path::new(&env::var("OUT_DIR").unwrap()).join("grammar_gen.rs");
    fs::write(dest, out).unwrap();
}

fn fnv64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100000001b3)
    })
}

fn s(v: &Value) -> &str {
    v.as_str()
        .unwrap_or_else(|| panic!("expected string, got {v}"))
}

fn opt_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::String(x) => format!("Some({x:?})"),
        other => panic!("expected string or null, got {other}"),
    }
}

fn str_slice(items: impl IntoIterator<Item = String>) -> String {
    let parts: Vec<String> = items.into_iter().map(|x| format!("{x:?}")).collect();
    format!("&[{}]", parts.join(", "))
}

fn command(out: &mut String, name: &str, aliases: &[String], help: Option<&Value>, p: &Value) {
    assert_eq!(
        s(&p["prefix_chars"]),
        "-",
        "unsupported prefix_chars in {name}"
    );
    let (subs, subs_required) = match &p["subcommands"] {
        Value::Null => (Vec::new(), false),
        sc => (
            sc["commands"].as_array().unwrap().clone(),
            sc["required"].as_bool().unwrap(),
        ),
    };
    write!(
        out,
        "CommandSpec {{ name: {name:?}, aliases: {}, help: {}, description: {}, allow_abbrev: {}, subcommands_required: {subs_required}, options: &[",
        str_slice(aliases.iter().cloned()),
        help.map(opt_str).unwrap_or_else(|| "None".into()),
        opt_str(&p["description"]),
        p["allow_abbrev"].as_bool().unwrap(),
    )
    .unwrap();
    for a in p["options"].as_array().unwrap() {
        option(out, name, a);
        out.push_str(", ");
    }
    out.push_str("], positionals: &[");
    for a in p["positionals"].as_array().unwrap() {
        positional(out, name, a);
        out.push_str(", ");
    }
    out.push_str("], mutex_groups: &[");
    for g in p["mutex_groups"].as_array().unwrap() {
        let members = g["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| s(m).to_string());
        write!(
            out,
            "MutexSpec {{ required: {}, members: {} }}, ",
            g["required"].as_bool().unwrap(),
            str_slice(members)
        )
        .unwrap();
    }
    out.push_str("], subcommands: &[");
    for c in &subs {
        let aliases: Vec<String> = c["aliases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| s(a).to_string())
            .collect();
        command(out, s(&c["name"]), &aliases, Some(&c["help"]), &c["parser"]);
        out.push_str(", ");
    }
    out.push_str("] }");
}

fn kind(a: &Value, ctx: &str) -> &'static str {
    match s(&a["kind"]) {
        "store" => "Kind::Store",
        "store_true" => "Kind::StoreTrue",
        "store_false" => "Kind::StoreFalse",
        "store_const" => "Kind::StoreConst",
        "append" => "Kind::Append",
        "append_const" => "Kind::AppendConst",
        "count" => "Kind::Count",
        "help" => "Kind::Help",
        other => panic!("{ctx}: unsupported argparse action kind {other:?}; extend lope-compat"),
    }
}

fn nargs(a: &Value, ctx: &str) -> String {
    match &a["nargs"] {
        Value::Null => "Nargs::One".into(),
        Value::Number(n) => format!("Nargs::Exactly({})", n.as_u64().unwrap()),
        Value::String(x) => match x.as_str() {
            "?" => "Nargs::Optional".into(),
            "*" => "Nargs::ZeroOrMore".into(),
            "+" => "Nargs::OneOrMore".into(),
            other => panic!("{ctx}: unsupported nargs {other:?}; extend lope-compat"),
        },
        other => panic!("{ctx}: bad nargs {other}"),
    }
}

fn value_type(a: &Value, ctx: &str) -> &'static str {
    match &a["type"] {
        Value::Null => "ValueType::Str",
        Value::String(t) => match t.as_str() {
            "str" => "ValueType::Str",
            "int" => "ValueType::Int",
            "float" => "ValueType::Float",
            "_positive_int" => "ValueType::PositiveInt",
            other => panic!("{ctx}: unsupported argparse type {other:?}; extend lope-compat"),
        },
        other => panic!("{ctx}: bad type {other}"),
    }
}

fn choices(a: &Value, ctx: &str) -> String {
    match &a["choices"] {
        Value::Null => "None".into(),
        Value::Array(items) => {
            let items = items.iter().map(|c| match c {
                Value::String(x) => x.clone(),
                other => panic!("{ctx}: non-string choice {other}"),
            });
            format!("Some({})", str_slice(items))
        }
        other => panic!("{ctx}: bad choices {other}"),
    }
}

fn option(out: &mut String, cmd: &str, a: &Value) {
    let strings: Vec<String> = a["option_strings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| s(o).to_string())
        .collect();
    let ctx = format!("{cmd} {}", strings.join("/"));
    for o in &strings {
        let ok = (o.starts_with("--") && o.len() > 2) || (o.len() == 2 && o.starts_with('-'));
        assert!(
            ok,
            "{ctx}: option string {o:?} has no clap equivalent; extend lope-compat"
        );
    }
    write!(
        out,
        "OptSpec {{ dest: {:?}, option_strings: {}, kind: {}, nargs: {}, ty: {}, choices: {}, required: {}, default_json: {:?}, help: {}, metavar: {} }}",
        s(&a["dest"]),
        str_slice(strings.clone()),
        kind(a, &ctx),
        nargs(a, &ctx),
        value_type(a, &ctx),
        choices(a, &ctx),
        a["required"].as_bool().unwrap(),
        a["default"].to_string(),
        opt_str(&a["help"]),
        opt_str(&a["metavar"]),
    )
    .unwrap();
}

fn positional(out: &mut String, cmd: &str, a: &Value) {
    let ctx = format!("{cmd} <{}>", s(&a["dest"]));
    assert_eq!(s(&a["kind"]), "store", "{ctx}: unsupported positional kind");
    write!(
        out,
        "PosSpec {{ dest: {:?}, nargs: {}, ty: {}, choices: {}, default_json: {:?}, help: {}, metavar: {} }}",
        s(&a["dest"]),
        nargs(a, &ctx),
        value_type(a, &ctx),
        choices(a, &ctx),
        a["default"].to_string(),
        opt_str(&a["help"]),
        opt_str(&a["metavar"]),
    )
    .unwrap();
}
