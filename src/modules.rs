//! Finding, parsing and checking the files a program imports.
//!
//! `import geo::Point;` in `dir/main.pk` imports from `dir/geo.pk`, and
//! `import shapes::geo;` names the module in `dir/shapes/geo.pk`: paths are
//! relative to the importing file. Each file is checked once, after the files
//! it imports, and the files that import it see only its `pub` items.
//!
//! The standard library is also files: every `.pk` file in the `std`
//! directory shipped with the compiler. They're loaded once, and every file
//! sees the `pub` items in their namespaces, like `std::io::printf`.

use std::cell::OnceCell;
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

/// The standard library, loaded from [`std_directory`].
#[derive(Debug, Default)]
pub struct Prelude {
    /// Where it was found, or `None` if it wasn't.
    pub directory: Option<PathBuf>,
    /// Its files, and any they import, each after the files it imports.
    pub files: Vec<SourceFile>,
    /// What each of its files exports, for every file to merge into its
    /// namespaces.
    pub interfaces: Vec<Rc<Interface>>,
}

impl Prelude {
    /// Whether `file` defines anything, so it has code to compile and link.
    /// Files of only `extern` functions and types don't.
    pub fn defines_code(file: &SourceFile) -> bool {
        let mut defines = false;
        walk(&file.ast, &mut |node| {
            if let Node::Expression(expression) = node {
                defines |= matches!(
                    expression.kind,
                    ExpressionKind::FunctionDeclarationExpression { .. }
                        | ExpressionKind::LetExpression { .. }
                );
            }
        });
        defines
    }
}

/// The directory holding the standard library: `$MINLANG_STD` if set (and
/// `None` if that isn't a directory), else
/// `lib/minlang/std` beside the `bin` directory holding the compiler (as
/// installed), else `std` beside the compiler, else the source tree's (for
/// development builds).
pub fn std_directory() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("MINLANG_STD") {
        return Some(PathBuf::from(directory)).filter(|directory| directory.is_dir());
    }
    let mut candidates = Vec::new();
    if let Some(bin) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_owned))
    {
        candidates.push(bin.join("../lib/minlang/std"));
        candidates.push(bin.join("std"));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("std"));
    candidates.into_iter().find(|directory| directory.is_dir())
}

thread_local! {
    static PRELUDE: OnceCell<Rc<Prelude>> = const { OnceCell::new() };
}

/// The standard library, loaded on first use.
pub fn prelude() -> Rc<Prelude> {
    PRELUDE.with(|prelude| Rc::clone(prelude.get_or_init(|| Rc::new(load_prelude()))))
}

fn load_prelude() -> Prelude {
    let Some(directory) = std_directory() else {
        return Prelude::default();
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.extension().is_some_and(|extension| extension == "pk"))
                .collect()
        })
        .unwrap_or_default();
    paths.sort();

    let mut loader = Loader {
        standard: true,
        ..Loader::default()
    };
    let interfaces = paths
        .iter()
        .filter_map(|path| loader.load(path).ok())
        .collect();
    Prelude {
        directory: Some(directory),
        files: loader.files,
        interfaces,
    }
}

#[derive(Default)]
struct Loader {
    /// Whether it's loading the standard library, which is checked without
    /// itself as a prelude.
    standard: bool,
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
        let mut module = mangle::module_name(Some(&name));
        if self.standard {
            // So `std/io.pk` can't clash with a program's `io.pk`
            module = format!("std_{module}");
        }
        let mut file = SourceFile {
            path: path.to_owned(),
            source,
            module,
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
        if self.standard {
            checker = checker.without_prelude();
        }
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
