//! Tests of the language server: its analysis directly, and the protocol by
//! running `minlang lsp`.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

use minlang::lsp::analysis::{Analysis, CompletionKind, SymbolKind, TokenClass, analyze};
use serde_json::{Value, json};

const SOURCE: &str = "import std::printf;

struct Point {
    x: s32,
    y: s32,
}

enum Shape {
    Circle(f32),
    Empty,
}

const LIMIT = 10;

fn area(shape: Shape, scale: f32) -> f32 {
    switch shape {
        case Shape::Circle(radius) {
            return radius * scale;
        }
        default {
            return 0.0f32;
        }
    }
}

fn main() -> s32 {
    let p = Point { x: 1, y: 2 };
    let total: s32 = p.x + LIMIT;
    printf(\"%d\\n\", total);
    0
}
";

/// The `(line, column)` of the `nth` occurrence of `needle` on the line
/// containing `line_text`.
fn find(source: &str, line_text: &str, needle: &str) -> (usize, usize) {
    let (line, text) = source
        .lines()
        .enumerate()
        .find(|(_, text)| text.contains(line_text))
        .unwrap_or_else(|| panic!("no line containing {line_text:?}"));
    let byte = text.find(needle).unwrap();
    (line, text[..byte].chars().count())
}

fn class_at(analysis: &Analysis, (line, column): (usize, usize)) -> Option<(TokenClass, u32)> {
    analysis
        .semantic_tokens()
        .into_iter()
        .find(|token| token.line == line && token.column == column)
        .map(|token| (token.class, token.modifiers))
}

#[test]
fn clean_documents_have_no_problems() {
    assert_eq!(analyze(SOURCE, None).problems, Vec::new());
}

#[test]
fn problems_from_every_stage() {
    let analysis = analyze("fn f() -> s32 {\n    let a = count;\n    a\n}\n", None);
    let messages: Vec<_> = analysis
        .problems
        .iter()
        .map(|problem| problem.message.as_str())
        .collect();
    assert_eq!(messages, ["Undefined name 'count'"]);
    let problem = &analysis.problems[0];
    assert_eq!((problem.line, problem.column, problem.length), (1, 12, 5));

    let lexer = analyze("let a = $;\n", None);
    assert_eq!(lexer.problems[0].message, "Unexpected character '$'");
}

#[test]
fn checker_problems_are_hidden_while_the_syntax_is_broken() {
    // The broken `let` is skipped, so `b` would look undefined
    let analysis = analyze("fn f() {\n    let b = ;\n    let c = b;\n}\n", None);
    let messages: Vec<_> = analysis
        .problems
        .iter()
        .map(|problem| problem.message.as_str())
        .collect();
    assert_eq!(messages, ["Expected expression, but got ';'"]);
}

#[test]
fn semantic_token_classes() {
    use minlang::lsp::analysis::modifiers::{DECLARATION, DEFAULT_LIBRARY, READONLY};

    let analysis = analyze(SOURCE, None);
    let at = |line_text, needle| class_at(&analysis, find(SOURCE, line_text, needle));

    assert_eq!(at("import std", "import"), Some((TokenClass::Keyword, 0)));
    assert_eq!(at("import std", "std"), Some((TokenClass::Namespace, 0)));
    assert_eq!(
        at("import std", "printf"),
        Some((TokenClass::Function, DEFAULT_LIBRARY))
    );
    assert_eq!(
        at("struct Point", "Point"),
        Some((TokenClass::Struct, DECLARATION))
    );
    assert_eq!(at("x: s32", "x"), Some((TokenClass::Property, DECLARATION)));
    assert_eq!(
        at("x: s32", "s32"),
        Some((TokenClass::Type, DEFAULT_LIBRARY))
    );
    assert_eq!(
        at("enum Shape", "Shape"),
        Some((TokenClass::Enum, DECLARATION))
    );
    assert_eq!(
        at("Circle(f32),", "Circle"),
        Some((TokenClass::EnumMember, DECLARATION))
    );
    assert_eq!(
        at("const LIMIT", "LIMIT"),
        Some((TokenClass::Variable, DECLARATION | READONLY))
    );
    assert_eq!(
        at("fn area", "area"),
        Some((TokenClass::Function, DECLARATION))
    );
    assert_eq!(
        at("fn area", "scale"),
        Some((TokenClass::Parameter, DECLARATION))
    );
    assert_eq!(at("fn area", "Shape"), Some((TokenClass::Enum, 0)));
    assert_eq!(
        at("case Shape::Circle", "Shape"),
        Some((TokenClass::Enum, 0))
    );
    assert_eq!(
        at("case Shape::Circle", "Circle"),
        Some((TokenClass::EnumMember, 0))
    );
    assert_eq!(
        at("case Shape::Circle", "radius"),
        Some((TokenClass::Variable, DECLARATION))
    );
    assert_eq!(
        at("return radius", "radius"),
        Some((TokenClass::Variable, 0))
    );
    assert_eq!(
        at("return radius", "scale"),
        Some((TokenClass::Parameter, 0))
    );
    assert_eq!(at("return radius", "*"), Some((TokenClass::Operator, 0)));
    assert_eq!(at("let p = Point", "Point"), Some((TokenClass::Struct, 0)));
    assert_eq!(at("let p = Point", "x"), Some((TokenClass::Property, 0)));
    assert_eq!(at("let p = Point", "1"), Some((TokenClass::Number, 0)));
    assert_eq!(at("let total", "x"), Some((TokenClass::Property, 0)));
    assert_eq!(
        at("let total", "LIMIT"),
        Some((TokenClass::Variable, READONLY))
    );
    assert_eq!(
        at("printf(\"", "printf"),
        Some((TokenClass::Function, DEFAULT_LIBRARY))
    );
    assert_eq!(at("printf(\"", "\""), Some((TokenClass::String, 0)));
}

#[test]
fn hover() {
    let analysis = analyze(SOURCE, None);
    let hover = |line_text, needle| {
        let (line, column) = find(SOURCE, line_text, needle);
        analysis.hover(line, column + 1)
    };
    let code = |text: &str| format!("```minlang\n{text}\n```");

    assert_eq!(hover("let p = Point", "p "), Some(code("let p: Point")));
    assert_eq!(
        hover("fn area", "area"),
        Some(code("fn area(shape: Shape, scale: f32) -> f32"))
    );
    assert_eq!(
        hover("return radius", "radius"),
        Some(code("let radius: f32"))
    );
    assert_eq!(hover("let total", "x +"), Some(code("Point.x: s32")));
    assert_eq!(
        hover("const LIMIT", "LIMIT"),
        Some(code("const LIMIT: s32"))
    );
    assert_eq!(
        hover("case Shape::Circle", "Circle"),
        Some(code("Shape::Circle(f32) = 0"))
    );
    assert_eq!(hover("x: s32", "s32"), Some(code("s32 (built-in type)")));
    assert_eq!(
        hover("printf(\"", "printf"),
        Some(code("fn printf(u8[?], ...) -> s32"))
    );
    assert_eq!(
        hover("struct Point", "Point"),
        Some(code("struct Point {\n    x: s32,\n    y: s32,\n}"))
    );
    // Not a name: the type of the expression around it
    let (line, column) = find(SOURCE, "let total", "+");
    assert_eq!(analysis.hover(line, column), Some(code("s32")));
}

#[test]
fn definitions_and_references() {
    let analysis = analyze(SOURCE, None);
    let declaration = find(SOURCE, "const LIMIT", "LIMIT");
    let (line, column) = find(SOURCE, "let total", "LIMIT");

    let definition = analysis.definition(line, column).unwrap();
    assert_eq!(
        definition,
        (declaration, (declaration.0, declaration.1 + 5))
    );

    let references: Vec<_> = analysis
        .references(line, column)
        .into_iter()
        .map(|(start, _)| start)
        .collect();
    assert_eq!(references, [declaration, (line, column)]);

    // Fields are found through member access and struct literals
    let field = find(SOURCE, "x: s32", "x");
    let (line, column) = find(SOURCE, "let total", "x +");
    assert_eq!(analysis.definition(line, column).unwrap().0, field);
    assert_eq!(analysis.references(line, column).len(), 3);

    // Built-ins have no declaration to go to or rename
    let (line, column) = find(SOURCE, "printf(\"", "printf");
    assert_eq!(analysis.definition(line, column), None);
    assert!(!analysis.can_rename(line, column));
}

#[test]
fn document_symbols() {
    let analysis = analyze(SOURCE, None);
    let symbols = analysis.symbols();
    let names: Vec<_> = symbols
        .iter()
        .map(|symbol| (symbol.name.as_str(), symbol.kind, symbol.children.len()))
        .collect();
    assert_eq!(
        names,
        [
            ("Point", SymbolKind::Struct, 2),
            ("Shape", SymbolKind::Enum, 2),
            ("LIMIT", SymbolKind::Constant, 0),
            ("area", SymbolKind::Function, 0),
            ("main", SymbolKind::Function, 0),
        ]
    );
    assert_eq!(symbols[1].children[0].name, "Circle");
    assert_eq!(symbols[1].children[0].kind, SymbolKind::EnumMember);
}

#[test]
fn completions() {
    let source =
        format!("{SOURCE}\nfn later() {{\n    let p = Point {{ x: 1, y: 2 }};\n    p.\n}}\n");
    let analysis = analyze(&source, None);

    // After `value.`: the struct's fields
    let (line, column) = find(&source, "    p.", ".");
    let labels: Vec<_> = analysis
        .completions(line, column + 1)
        .into_iter()
        .map(|completion| completion.label)
        .collect();
    assert_eq!(labels, ["x", "y"]);

    // After `Enum::`: its variants, and after `module::` its members
    let (line, column) = find(SOURCE, "case Shape::Circle", "Circle");
    let variants: Vec<_> = analysis
        .completions(line, column)
        .into_iter()
        .map(|completion| (completion.label, completion.kind))
        .collect();
    assert_eq!(
        variants,
        [
            ("Circle".to_owned(), CompletionKind::EnumMember),
            ("Empty".to_owned(), CompletionKind::EnumMember)
        ]
    );
    let (line, column) = find(SOURCE, "import std", "printf");
    assert!(
        analysis
            .completions(line, column)
            .iter()
            .any(|c| c.label == "strcat")
    );

    // Elsewhere: keywords, types, functions and the variables in scope
    let (line, column) = find(SOURCE, "    0", "0");
    let everything = analysis.completions(line, column);
    let has = |label: &str, kind| {
        everything
            .iter()
            .any(|c| c.label == label && c.kind == kind)
    };
    assert!(has("while", CompletionKind::Keyword));
    assert!(has("u8", CompletionKind::Type));
    assert!(has("Point", CompletionKind::Type));
    assert!(has("area", CompletionKind::Function));
    assert!(has("total", CompletionKind::Variable));
    assert!(has("LIMIT", CompletionKind::Variable));
    // `radius` belongs to another function
    assert!(!has("radius", CompletionKind::Variable));
}

// -----------------------------------------------------------------------------
// The protocol
// -----------------------------------------------------------------------------

struct Client {
    process: std::process::Child,
    output: BufReader<std::process::ChildStdout>,
    next_id: i64,
}

impl Client {
    fn start(capabilities: Value) -> Self {
        let mut process = Command::new(env!("CARGO_BIN_EXE_minlang"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let output = BufReader::new(process.stdout.take().unwrap());
        let mut client = Self {
            process,
            output,
            next_id: 0,
        };
        let result = client.request("initialize", json!({ "capabilities": capabilities }));
        assert!(result["capabilities"]["hoverProvider"].as_bool().unwrap());
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, message: Value) {
        let body = message.to_string();
        let stdin = self.process.stdin.as_mut().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        stdin.flush().unwrap();
    }

    fn receive(&mut self) -> Value {
        let mut length = 0;
        loop {
            let mut line = String::new();
            self.output.read_line(&mut line).unwrap();
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        self.output.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let message = self.receive();
            if message["id"] == id {
                return message["result"].clone();
            }
        }
    }

    fn finish(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        assert!(self.process.wait().unwrap().success());
    }
}

const URI: &str = "file:///project/demo.pk";

#[test]
fn protocol_round_trip() {
    let mut client = Client::start(json!({}));

    // `🦀` takes two UTF-16 units, which shifts the columns after it
    let text = "fn main() -> s32 {\n    let x = \"🦀\"; nope + x\n}\n";
    client.notify(
        "textDocument/didOpen",
        json!({ "textDocument": { "uri": URI, "languageId": "minlang", "version": 1, "text": text } }),
    );
    let diagnostics = client.receive();
    assert_eq!(diagnostics["method"], "textDocument/publishDiagnostics");
    let diagnostic = &diagnostics["params"]["diagnostics"][0];
    assert_eq!(diagnostic["message"], "Undefined name 'nope'");
    assert_eq!(
        diagnostic["range"],
        json!({ "start": { "line": 1, "character": 18 }, "end": { "line": 1, "character": 22 } })
    );

    let hover = client.request(
        "textDocument/hover",
        json!({ "textDocument": { "uri": URI }, "position": { "line": 1, "character": 25 } }),
    );
    // A string literal is a null-terminated byte array; `🦀` is four bytes
    assert_eq!(hover["contents"]["value"], "```minlang\nlet x: u8[5]\n```");

    let definition = client.request(
        "textDocument/definition",
        json!({ "textDocument": { "uri": URI }, "position": { "line": 1, "character": 25 } }),
    );
    assert_eq!(
        definition["range"]["start"],
        json!({ "line": 1, "character": 8 })
    );

    let rename = client.request(
        "textDocument/rename",
        json!({ "textDocument": { "uri": URI }, "position": { "line": 1, "character": 8 }, "newName": "value" }),
    );
    assert_eq!(rename["changes"][URI].as_array().unwrap().len(), 2);

    let tokens = client.request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": URI } }),
    );
    assert_eq!(tokens["data"].as_array().unwrap().len() % 5, 0);

    // Fixing the document clears the problems
    client.notify(
        "textDocument/didChange",
        json!({
            "textDocument": { "uri": URI, "version": 2 },
            "contentChanges": [{ "text": "fn main() -> s32 { 0 }\n" }]
        }),
    );
    let diagnostics = client.receive();
    assert_eq!(diagnostics["params"]["diagnostics"], json!([]));

    client.finish();
}

#[test]
fn positions_use_utf16_unless_the_client_offers_utf32() {
    // `🦀` is one character but two UTF-16 units, so the end of the error
    // on it differs between the encodings
    let text = "fn main() { let 🦀x = 1; }\n";

    for (capabilities, column) in [
        (json!({}), 18),
        (
            json!({ "general": { "positionEncodings": ["utf-32"] } }),
            17,
        ),
    ] {
        let mut client = Client::start(capabilities.clone());
        client.notify(
            "textDocument/didOpen",
            json!({ "textDocument": { "uri": URI, "languageId": "minlang", "version": 1, "text": text } }),
        );
        let diagnostics = client.receive();
        // `🦀` isn't a valid identifier character
        assert_eq!(
            diagnostics["params"]["diagnostics"][0]["range"]["start"]["character"],
            16
        );
        let after = &diagnostics["params"]["diagnostics"][0]["range"]["end"]["character"];
        assert_eq!(*after, json!(column), "{capabilities}");
        client.finish();
    }
}
