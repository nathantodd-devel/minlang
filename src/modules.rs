//! Finding, parsing and checking the files a program imports.
//!
//! `import geo::Point;` in `dir/main.pk` imports from `dir/geo.pk`, and
//! `import shapes::geo;` names the module in `dir/shapes/geo.pk`: paths are
//! relative to the importing file. Each file is checked once, after the files
//! it imports, and the files that import it see only its `pub` items.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::checker::{CheckedProgram, Checker, Imports, Interface, module_members};
use crate::codegen::mangle;
use crate::error::GenericError;
use crate::parser::visit::{Node, walk};
use crate::parser::{Ast, ExpressionKind, Parser};
use crate::tokenizer::Tokenizer;

/// An error from any stage, with its message already formatted.
pub type FileError = GenericError<String>;

/// An imported file and what the compiler made of it.
#[derive(Debug)]
pub struct SourceFile {
    /// Where it was found: the importing file's directory joined with the
    /// import's path.
    pub path: PathBuf,
    pub source: String,
    /// Its module name, the first segment of its symbols.
    pub module: String,
    /// Empty if it couldn't be parsed.
    pub ast: Ast,
    pub program: CheckedProgram,
    /// The errors of the first stage that had any: tokenizing, parsing or
    /// checking.
    pub errors: Vec<FileError>,
}

impl SourceFile {
    pub fn name(&self) -> String {
        self.path.display().to_string()
    }
}

/// The files a file imports, directly or not.
#[derive(Debug, Default)]
pub struct Imported {
    /// What the file itself can import, for its checker.
    pub imports: Imports,
    /// Every file imported, each after the files it imports.
    pub files: Vec<SourceFile>,
}

impl Imported {
    /// Two files whose module names are the same, including the importing
    /// file's `module`. Their symbols would clash when linked.
    pub fn clashing_modules(&self, module: &str) -> Option<(String, String)> {
        let mut seen: HashMap<&str, String> = HashMap::new();
        seen.insert(module, String::new());
        for file in &self.files {
            if let Some(other) = seen.insert(&file.module, file.name()) {
                return Some((other, file.name()));
            }
        }
        None
    }
}

/// Loads the files that `ast`, parsed from the file at `path`, imports.
pub fn load_imports(path: &Path, ast: &Ast) -> Imported {
    let mut loader = Loader::default();
    let root = canonical(path);
    loader.stack.push(root);
    let imports = loader.imports_of(path, ast);
    Imported {
        imports,
        files: loader.files,
    }
}

#[derive(Default)]
struct Loader {
    files: Vec<SourceFile>,
    /// What each file loaded so far exports, by its canonical path.
    loaded: HashMap<PathBuf, Rc<Interface>>,
    /// The files being loaded, outermost first, for finding cycles.
    stack: Vec<PathBuf>,
}

impl Loader {
    /// Loads the imports of the file at `path`.
    fn imports_of(&mut self, path: &Path, ast: &Ast) -> Imports {
        let directory = path.parent().unwrap_or(Path::new(""));
        let mut imports = Imports::new();
        for key in import_paths(ast) {
            let file = directory.join(key.replace("::", "/") + ".pk");
            if !file.is_file() {
                continue;
            }
            let interface = self.load(&file);
            imports.insert(key, interface);
        }
        imports
    }

    fn load(&mut self, path: &Path) -> Result<Rc<Interface>, String> {
        let canonical = canonical(path);
        if let Some(interface) = self.loaded.get(&canonical) {
            return Ok(Rc::clone(interface));
        }
        if let Some(start) = self.stack.iter().position(|file| *file == canonical) {
            let cycle: Vec<String> = self.stack[start..]
                .iter()
                .chain([&canonical])
                .map(|file| file_name(file))
                .collect();
            return Err(format!("the imports form a cycle: {}", cycle.join(" -> ")));
        }
        let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;

        self.stack.push(canonical.clone());
        let file = self.check(path, source);
        self.stack.pop();

        let interface = Rc::new(Interface {
            module: file.module.clone(),
            program: file.program.clone(),
            has_errors: !file.errors.is_empty(),
        });
        self.files.push(file);
        self.loaded.insert(canonical, Rc::clone(&interface));
        Ok(interface)
    }

    /// Tokenizes, parses and checks a file, after loading its imports.
    fn check(&mut self, path: &Path, source: String) -> SourceFile {
        let name = path.display().to_string();
        let mut file = SourceFile {
            path: path.to_owned(),
            source,
            module: mangle::module_name(Some(&name)),
            ast: Ast::new(),
            program: CheckedProgram::default(),
            errors: Vec::new(),
        };

        let tokens = match Tokenizer::new(file.source.as_str(), Some(&name)).tokenize() {
            Ok(tokens) => tokens,
            Err(errors) => {
                file.errors = formatted(&errors);
                return file;
            }
        };
        let mut parser = Parser::new(tokens, Some(&name));
        file.ast = match parser.parse() {
            Ok(ast) => ast.clone(),
            Err(errors) => {
                file.errors = formatted(errors);
                return file;
            }
        };

        let imports = self.imports_of(path, &file.ast);
        let mut checker = Checker::new(Some(&name)).with_imports(imports);
        let (program, errors) = checker.check_partial(&file.ast);
        file.program = program;
        file.errors = formatted(&errors);
        file
    }
}

/// The module paths `ast` may import from files: `geo` for `import geo;` and
/// `import geo::Point;`, and also `shapes::geo` for `import shapes::geo;`,
/// which names either an item or a module.
fn import_paths(ast: &Ast) -> Vec<String> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    walk(ast, &mut |node| {
        let Node::Expression(expression) = node else {
            return;
        };
        let ExpressionKind::ImportExpression { module, imports } = &expression.kind else {
            return;
        };
        let mut candidates = Vec::new();
        if module.is_empty() {
            candidates.extend(imports.iter().cloned());
        } else {
            let module = module.join("::");
            if is_builtin(&module) {
                return;
            }
            if let [name] = imports.as_slice()
                && name != "*"
            {
                candidates.push(format!("{module}::{name}"));
            }
            candidates.push(module);
        }
        for candidate in candidates {
            if !is_builtin(&candidate) && seen.insert(candidate.clone()) {
                paths.push(candidate);
            }
        }
    });
    paths
}

fn is_builtin(module: &str) -> bool {
    module_members(module).is_some()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn formatted<K: fmt::Display>(errors: &[GenericError<K>]) -> Vec<FileError> {
    errors
        .iter()
        .map(|error| FileError {
            kind: error.kind.to_string(),
            line: error.line,
            column: error.column,
            file: error.file.clone(),
        })
        .collect()
}
