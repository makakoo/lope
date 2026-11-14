//! Minimal Graphviz DOT parser for flow graphs (`lope/flow/dot.py`, Sprint S7, `FR36`).

use crate::model::{normalize_kind, truthy, FlowConfigError, FlowEdge, FlowGraph, FlowNode};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Arrow,
    LBrace,
    RBrace,
    LBrack,
    RBrack,
    Comma,
    Semi,
    Eq,
    String,
    Ident,
    Number,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub value: String,
    pub line: usize,
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, FlowConfigError> {
    static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
    let re = TOKEN_RE.get_or_init(|| {
        Regex::new(
            r#"(?s)\A(?:(?P<ws>\s+)|(?P<comment>//[^\n]*|\#[^\n]*)|(?P<arrow>->)|(?P<lbrace>\{)|(?P<rbrace>\})|(?P<lbrack>\[)|(?P<rbrack>\])|(?P<comma>,)|(?P<semi>;)|(?P<eq>=)|(?P<string>"(?:\\.|[^"\\])*")|(?P<ident>[A-Za-z_][A-Za-z0-9_.]*)|(?P<number>-?\d+(?:\.\d+)?))"#,
        )
        .unwrap()
    });

    let mut tokens = Vec::new();
    let mut pos = 0usize;
    let mut line = 1usize;
    let bytes = src.as_bytes();

    while pos < bytes.len() {
        let rest = &src[pos..];
        let Some(caps) = re.captures(rest) else {
            let ch = rest.chars().next().unwrap_or('?');
            return Err(FlowConfigError(format!(
                "line {line}: unexpected character {ch:?}"
            )));
        };
        let m = caps.get(0).unwrap();
        let text = m.as_str();
        line += text.bytes().filter(|&b| b == b'\n').count();
        pos += m.end();

        if caps.name("ws").is_some() || caps.name("comment").is_some() {
            continue;
        }
        let kind = if caps.name("arrow").is_some() {
            TokenKind::Arrow
        } else if caps.name("lbrace").is_some() {
            TokenKind::LBrace
        } else if caps.name("rbrace").is_some() {
            TokenKind::RBrace
        } else if caps.name("lbrack").is_some() {
            TokenKind::LBrack
        } else if caps.name("rbrack").is_some() {
            TokenKind::RBrack
        } else if caps.name("comma").is_some() {
            TokenKind::Comma
        } else if caps.name("semi").is_some() {
            TokenKind::Semi
        } else if caps.name("eq").is_some() {
            TokenKind::Eq
        } else if caps.name("string").is_some() {
            TokenKind::String
        } else if caps.name("ident").is_some() {
            TokenKind::Ident
        } else {
            TokenKind::Number
        };
        tokens.push(Token {
            kind,
            value: text.to_string(),
            line,
        });
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        value: String::new(),
        line,
    });
    Ok(tokens)
}

fn unquote(val: &str) -> String {
    if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
        val[1..val.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        val.to_string()
    }
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
}

impl Parser {
    fn peek(&self, offset: usize) -> &Token {
        let idx = (self.i + offset).min(self.toks.len() - 1);
        &self.toks[idx]
    }

    fn next(&mut self) -> Token {
        let tok = self.toks[self.i].clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        tok
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token, FlowConfigError> {
        let tok = self.peek(0).clone();
        if tok.kind != kind {
            return Err(FlowConfigError(format!(
                "line {}: expected {:?}, got {:?} {:?}",
                tok.line, kind, tok.kind, tok.value
            )));
        }
        Ok(self.next())
    }

    fn parse(&mut self) -> Result<FlowGraph, FlowConfigError> {
        if self.peek(0).kind == TokenKind::Ident && self.peek(0).value == "strict" {
            self.next();
        }
        let head = self.peek(0).clone();
        if head.kind != TokenKind::Ident || (head.value != "digraph" && head.value != "graph") {
            return Err(FlowConfigError(format!(
                "line {}: expected 'digraph', got {:?}",
                head.line, head.value
            )));
        }
        self.next();

        let mut name = "flow".to_string();
        if matches!(self.peek(0).kind, TokenKind::Ident | TokenKind::String) {
            name = unquote(&self.next().value);
        }
        self.expect(TokenKind::LBrace)?;

        let mut node_order = Vec::new();
        let mut nodes: HashMap<String, FlowNode> = HashMap::new();
        let mut edges: Vec<FlowEdge> = Vec::new();
        let mut graph_attrs: HashMap<String, String> = HashMap::new();

        while self.peek(0).kind != TokenKind::RBrace {
            if self.peek(0).kind == TokenKind::Eof {
                return Err(FlowConfigError(format!(
                    "line {}: unexpected end of input (missing '}}')",
                    self.peek(0).line
                )));
            }
            self.statement(&mut node_order, &mut nodes, &mut edges, &mut graph_attrs)?;
            while matches!(self.peek(0).kind, TokenKind::Semi | TokenKind::Comma) {
                self.next();
            }
        }
        self.expect(TokenKind::RBrace)?;

        Ok(FlowGraph {
            name,
            node_order,
            nodes,
            edges,
            graph_attrs,
        })
    }

    fn statement(
        &mut self,
        node_order: &mut Vec<String>,
        nodes: &mut HashMap<String, FlowNode>,
        edges: &mut Vec<FlowEdge>,
        graph_attrs: &mut HashMap<String, String>,
    ) -> Result<(), FlowConfigError> {
        let tok = self.peek(0).clone();
        if tok.kind != TokenKind::Ident {
            return Err(FlowConfigError(format!(
                "line {}: expected a node/edge statement, got {:?} {:?}",
                tok.line, tok.kind, tok.value
            )));
        }

        if tok.value == "graph" && self.peek(1).kind == TokenKind::LBrack {
            self.next();
            graph_attrs.extend(self.attr_list()?);
            return Ok(());
        }
        if (tok.value == "node" || tok.value == "edge") && self.peek(1).kind == TokenKind::LBrack {
            self.next();
            let _ = self.attr_list()?;
            return Ok(());
        }

        let first = self.next().value;
        if self.peek(0).kind == TokenKind::Arrow {
            let mut chain = vec![first];
            while self.peek(0).kind == TokenKind::Arrow {
                self.next();
                let tgt = self.peek(0).clone();
                if !matches!(tgt.kind, TokenKind::Ident | TokenKind::String) {
                    return Err(FlowConfigError(format!(
                        "line {}: expected node id after '->', got {:?}",
                        tgt.line, tgt.value
                    )));
                }
                chain.push(unquote(&self.next().value));
            }
            let attrs = if self.peek(0).kind == TokenKind::LBrack {
                self.attr_list()?
            } else {
                HashMap::new()
            };
            for pair in chain.windows(2) {
                edges.push(FlowEdge {
                    source: pair[0].clone(),
                    target: pair[1].clone(),
                    condition: attrs
                        .get("condition")
                        .or_else(|| attrs.get("when"))
                        .cloned(),
                    label: attrs.get("label").cloned().unwrap_or_default(),
                    loop_restart: truthy(attrs.get("loop_restart").map(|s| s.as_str())),
                    line: tok.line,
                });
            }
            return Ok(());
        }

        let attrs = if self.peek(0).kind == TokenKind::LBrack {
            self.attr_list()?
        } else {
            HashMap::new()
        };
        let kind = normalize_kind(
            attrs.get("type").map(|s| s.as_str()).unwrap_or(""),
            attrs.get("shape").map(|s| s.as_str()).unwrap_or(""),
        )?;
        if let Some(existing) = nodes.get_mut(&first) {
            existing.attrs.extend(attrs);
        } else {
            node_order.push(first.clone());
            nodes.insert(
                first.clone(),
                FlowNode {
                    id: first,
                    kind,
                    attrs,
                    line: tok.line,
                },
            );
        }
        Ok(())
    }

    fn attr_list(&mut self) -> Result<HashMap<String, String>, FlowConfigError> {
        self.expect(TokenKind::LBrack)?;
        let mut attrs = HashMap::new();
        while self.peek(0).kind != TokenKind::RBrack {
            if self.peek(0).kind == TokenKind::Eof {
                return Err(FlowConfigError(format!(
                    "line {}: unexpected end of input in attribute list (missing ']')",
                    self.peek(0).line
                )));
            }
            let key_tok = self.peek(0).clone();
            if !matches!(key_tok.kind, TokenKind::Ident | TokenKind::String) {
                return Err(FlowConfigError(format!(
                    "line {}: expected attribute name, got {:?}",
                    key_tok.line, key_tok.value
                )));
            }
            let key = unquote(&self.next().value);
            self.expect(TokenKind::Eq)?;
            let val_tok = self.peek(0).clone();
            if !matches!(
                val_tok.kind,
                TokenKind::Ident | TokenKind::String | TokenKind::Number
            ) {
                return Err(FlowConfigError(format!(
                    "line {}: expected value for {key:?}, got {:?}",
                    val_tok.line, val_tok.value
                )));
            }
            attrs.insert(key, unquote(&self.next().value));
            while matches!(self.peek(0).kind, TokenKind::Comma | TokenKind::Semi) {
                self.next();
            }
        }
        self.expect(TokenKind::RBrack)?;
        Ok(attrs)
    }
}

pub fn parse_dot(src: &str, base_dir: Option<&Path>) -> Result<FlowGraph, FlowConfigError> {
    let mut graph = Parser {
        toks: tokenize(src)?,
        i: 0,
    }
    .parse()?;
    resolve_prompt_files(&mut graph, base_dir)?;
    Ok(graph)
}

pub fn load_flow_graph(path: &Path) -> Result<FlowGraph, FlowConfigError> {
    if !path.exists() {
        return Err(FlowConfigError(format!(
            "workflow file not found: {}",
            path.display()
        )));
    }
    let src = fs::read_to_string(path).map_err(|e| FlowConfigError(e.to_string()))?;
    parse_dot(&src, path.parent())
}

fn resolve_prompt_files(
    graph: &mut FlowGraph,
    base_dir: Option<&Path>,
) -> Result<(), FlowConfigError> {
    let Some(base_dir) = base_dir else {
        return Ok(());
    };
    let base = fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());
    for node in graph.nodes.values_mut() {
        let Some(prompt) = node.attrs.get("prompt").cloned() else {
            continue;
        };
        let Some(rel) = prompt.strip_prefix('@').map(str::trim) else {
            continue;
        };
        if rel.is_empty() {
            return Err(FlowConfigError(format!(
                "node {:?}: empty @file reference",
                node.id
            )));
        }
        let rel_path = Path::new(rel);
        if rel_path.is_absolute() {
            return Err(FlowConfigError(format!(
                "node {:?}: @file must be a relative path inside the graph directory, got absolute {rel:?}",
                node.id
            )));
        }
        let joined = base.join(rel_path);
        let target = fs::canonicalize(&joined).map_err(|_| {
            FlowConfigError(format!(
                "node {:?}: prompt file not found: {}",
                node.id,
                joined.display()
            ))
        })?;
        if target != base && !target.starts_with(&base) {
            return Err(FlowConfigError(format!(
                "node {:?}: @file path escapes the graph directory: {rel:?}",
                node.id
            )));
        }
        if !target.is_file() {
            return Err(FlowConfigError(format!(
                "node {:?}: prompt file not found: {}",
                node.id,
                target.display()
            )));
        }
        let contents = fs::read_to_string(&target).map_err(|e| FlowConfigError(e.to_string()))?;
        node.attrs.insert("prompt".to_string(), contents);
    }
    Ok(())
}
