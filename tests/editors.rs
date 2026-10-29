//! Checks that the editor integrations in `editors/` stay in step with the
//! compiler, and runs their own tests when the tools are installed.

use std::path::Path;
use std::process::Command;

use minlang::Keyword;
use minlang::checker::module_members;

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn have(tool: &str) -> bool {
    let found = Command::new(tool).arg("--version").output().is_ok();
    if !found {
        eprintln!("skipping: {tool} not found");
    }
    found
}

/// Keywords that are spelled differently in grammars: `true`/`false` are
/// literals, not keywords, so every keyword is listed.
fn keywords() -> Vec<&'static str> {
    Keyword::ALL
        .iter()
        .map(|keyword| keyword.as_str())
        .collect()
}

fn primitives() -> Vec<&'static str> {
    module_members("types").unwrap()
}

#[test]
fn tree_sitter_grammar_knows_every_keyword_and_type() {
    let grammar = read("editors/tree-sitter-minlang/grammar.js");
    let highlights = read("editors/tree-sitter-minlang/queries/highlights.scm");
    for keyword in keywords() {
        assert!(
            grammar.contains(&format!("'{keyword}'")),
            "grammar.js lacks {keyword}"
        );
        // `break` and `continue` are highlighted through their nodes
        let quoted = format!("\"{keyword}\"");
        let node = format!("({keyword}_expression)");
        assert!(
            highlights.contains(&quoted) || highlights.contains(&node),
            "highlights.scm lacks {keyword}"
        );
    }
    for primitive in primitives() {
        assert!(
            grammar.contains(&format!("'{primitive}'")),
            "grammar.js lacks {primitive}"
        );
    }
}

#[test]
fn textmate_grammar_knows_every_keyword_and_type() {
    let text = read("editors/vscode/syntaxes/minlang.tmLanguage.json");
    let grammar: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(grammar["scopeName"], "source.minlang");

    let words = |text: &str| -> Vec<String> {
        text.split(|c: char| !c.is_ascii_alphanumeric())
            .map(str::to_owned)
            .collect()
    };
    // Regex word boundaries (`\\b` in the JSON) would glue onto the words
    let all = words(&text.replace("\\\\b", " "));
    for word in keywords().into_iter().chain(primitives()) {
        assert!(
            all.iter().any(|w| w == word),
            "the TextMate grammar lacks {word}"
        );
    }
}

#[test]
fn helix_highlights_match_the_shared_queries() {
    // The Helix copy only renames captures
    let mapping = [
        ("@number.float", "@constant.numeric.float"),
        ("@number", "@constant.numeric.integer"),
        ("@boolean", "@constant.builtin.boolean"),
        ("@string.escape", "@constant.character.escape"),
        ("@module", "@namespace"),
        ("@function.call", "@function"),
        ("@property", "@variable.other.member"),
        ("@keyword.type", "@keyword.storage.type"),
        ("@keyword.modifier", "@keyword.storage.modifier"),
        ("@keyword.import", "@keyword.control.import"),
        ("@keyword.conditional", "@keyword.control.conditional"),
        ("@keyword.repeat", "@keyword.control.repeat"),
        ("@keyword.return", "@keyword.control.return"),
    ];
    let patterns = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| !line.starts_with(';'))
            .map(str::to_owned)
            .collect()
    };

    let shared = patterns(&read("editors/tree-sitter-minlang/queries/highlights.scm"));
    let helix = patterns(&read("editors/helix/highlights.scm"));
    let renamed: Vec<String> = shared
        .iter()
        .map(|line| {
            let mut line = line.clone();
            for (from, to) in mapping {
                // Whole capture names only, so `@number` leaves `@number.float`
                let mut result = String::new();
                let mut rest = line.as_str();
                while let Some(start) = rest.find(from) {
                    let after = &rest[start + from.len()..];
                    let whole =
                        !after.starts_with(|c: char| c.is_alphanumeric() || c == '.' || c == '_');
                    result.push_str(&rest[..start]);
                    result.push_str(if whole { to } else { from });
                    rest = after;
                }
                result.push_str(rest);
                line = result;
            }
            line
        })
        .collect();
    assert_eq!(renamed, helix);
}

#[test]
fn shared_query_links_resolve() {
    for link in [
        "editors/zed/languages/minlang/highlights.scm",
        "editors/zed/languages/minlang/indents.scm",
        "editors/zed/languages/minlang/brackets.scm",
        "editors/neovim/queries/minlang/highlights.scm",
        "editors/neovim/queries/minlang/indents.scm",
    ] {
        let path = Path::new(link);
        assert!(
            path.is_symlink(),
            "{link} should link to the shared queries"
        );
        assert!(path.exists(), "{link} is a broken link");
    }
}

#[test]
fn semantic_token_legend_uses_standard_names() {
    use minlang::lsp::analysis::{TokenClass, modifiers};

    let standard = [
        "namespace",
        "type",
        "class",
        "enum",
        "interface",
        "struct",
        "typeParameter",
        "parameter",
        "variable",
        "property",
        "enumMember",
        "event",
        "function",
        "method",
        "macro",
        "keyword",
        "modifier",
        "comment",
        "string",
        "number",
        "regexp",
        "operator",
        "decorator",
    ];
    for class in TokenClass::ALL {
        assert!(
            standard.contains(&class.name()),
            "{} isn't a standard token type",
            class.name()
        );
    }
    assert_eq!(
        modifiers::NAMES,
        ["declaration", "readonly", "defaultLibrary"]
    );
}

#[test]
fn tree_sitter_tests_pass() {
    if !have("tree-sitter") {
        return;
    }
    let output = Command::new("tree-sitter")
        .arg("test")
        .current_dir("editors/tree-sitter-minlang")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "tree-sitter test failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tree_sitter_parses_every_valid_example() {
    if !have("tree-sitter") {
        return;
    }
    let mut files: Vec<_> = std::fs::read_dir("examples")
        .unwrap()
        .chain(std::fs::read_dir("tests/programs").unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pk"))
        // A catalog of deliberately invalid literals
        .filter(|path| !path.ends_with("errors.pk"))
        .collect();
    files.sort();

    for file in files {
        let output = Command::new("tree-sitter")
            .arg("parse")
            .arg(std::fs::canonicalize(&file).unwrap())
            .current_dir("editors/tree-sitter-minlang")
            .output()
            .unwrap();
        let tree = String::from_utf8_lossy(&output.stdout);
        assert!(
            !tree.contains("ERROR") && !tree.contains("MISSING"),
            "{} doesn't parse:\n{tree}",
            file.display()
        );
    }
}

#[test]
fn textmate_grammar_tests_pass() {
    if !have("node") || !Path::new("editors/vscode/node_modules").exists() {
        eprintln!("skipping: run `npm install` in editors/vscode first");
        return;
    }
    let output = Command::new("node")
        .arg("test/grammar.test.js")
        .current_dir("editors/vscode")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
