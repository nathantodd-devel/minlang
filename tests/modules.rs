//! Tests of importing other files: finding them, checking against what they
//! export, and the symbols generated for imported items.

use std::path::{Path, PathBuf};

use minlang::codegen::{self, Backend, TextBackend};
use minlang::modules::{Imported, load_imports};
use minlang::{Ast, CheckError, CheckedProgram, Checker, Parser, Tokenizer};

/// Writes `files`, as (relative path, source), to a fresh directory.
fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let directory = std::env::temp_dir()
        .join(format!("minlang-modules-{}", std::process::id()))
        .join(name);
    let _ = std::fs::remove_dir_all(&directory);
    for (path, source) in files {
        let path = directory.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    directory
}

fn parse(path: &Path) -> Ast {
    let source = std::fs::read_to_string(path).unwrap();
    let tokens = Tokenizer::new(source, None).tokenize().unwrap();
    Parser::new(tokens, None).parse().unwrap().clone()
}

/// Checks `main.pk` in `directory` with its imports.
fn check(directory: &Path) -> (Ast, Imported, Result<CheckedProgram, Vec<String>>) {
    let path = directory.join("main.pk");
    let ast = parse(&path);
    let imported = load_imports(&path, &ast);
    let result = Checker::new(Some("main.pk"))
        .with_imports(imported.imports.clone())
        .check(&ast)
        .map_err(|errors| {
            errors
                .iter()
                .map(|error: &CheckError| error.message())
                .collect()
        });
    (ast, imported, result)
}

fn errors(directory: &Path) -> Vec<String> {
    match check(directory).2 {
        Ok(_) => panic!("expected errors"),
        Err(errors) => errors,
    }
}

const GEO: &str = "
pub struct Point { x: s32, y: s32 }
pub enum Shape { Dot(Point), Empty }
pub let count: u32 = 0;
pub fn origin() -> Point { Point { x: 0, y: 0 } }
fn helper() {}
";

#[test]
fn imports_items_by_name_glob_and_module() {
    let directory = project(
        "items",
        &[
            ("geo.pk", GEO),
            (
                "main.pk",
                "import geo::{Point, origin};
                 fn a() -> Point { origin() }",
            ),
        ],
    );
    check(&directory).2.unwrap();

    let directory = project(
        "glob",
        &[
            ("geo.pk", GEO),
            (
                "main.pk",
                "import geo::*;
                 fn a() -> Shape { count += 1; Shape::Dot(origin()) }",
            ),
        ],
    );
    check(&directory).2.unwrap();

    let directory = project(
        "qualified",
        &[
            ("shapes/geo.pk", GEO),
            (
                "main.pk",
                "import shapes::geo;
                 fn a() -> geo::Shape {
                     let p: geo::Point = geo::Point { x: 1, y: 2 };
                     switch (geo::Shape::Dot(p)) {
                         case geo::Shape::Dot(q) { return geo::Shape::Dot(q); }
                         default {}
                     }
                     geo::Shape::Empty
                 }",
            ),
        ],
    );
    check(&directory).2.unwrap();
}

#[test]
fn only_pub_items_can_be_imported() {
    let directory = project(
        "private",
        &[
            ("geo.pk", GEO),
            (
                "main.pk",
                "import geo::{helper, nothing}; import geo; fn f() { geo::helper(); }",
            ),
        ],
    );
    assert_eq!(
        errors(&directory),
        [
            "'helper' in 'geo' isn't public; declare it with 'pub' to import it",
            "'geo' has no member 'nothing'",
            "'helper' in 'geo' isn't public; declare it with 'pub' to import it",
        ]
    );
}

#[test]
fn an_item_imported_twice_is_one_item() {
    // `main` gets `Point` both from `geo` and through `lines`
    let directory = project(
        "diamond",
        &[
            ("geo.pk", GEO),
            (
                "lines.pk",
                "import geo::Point; pub fn length(a: Point, b: Point) -> s32 { b.x - a.x }",
            ),
            (
                "main.pk",
                "import geo::{Point, origin}; import lines::length;
                 fn f() -> s32 { length(origin(), Point { x: 3, y: 0 }) }",
            ),
        ],
    );
    let (_, imported, program) = check(&directory);
    let program = program.unwrap();
    assert_eq!(program.structs.len(), 1);
    // `geo` is loaded once, before `lines`, which imports it
    let modules: Vec<_> = imported
        .files
        .iter()
        .map(|file| file.module.as_str())
        .collect();
    assert_eq!(modules, ["geo", "lines"]);
}

#[test]
fn missing_broken_and_cyclic_imports() {
    let directory = project("missing", &[("main.pk", "import geo::Point;")]);
    assert_eq!(
        errors(&directory),
        ["Unknown module 'geo': there's no file 'geo.pk' next to this one"]
    );

    let directory = project(
        "broken",
        &[
            ("geo.pk", "pub fn f() -> s32 { x }"),
            ("main.pk", "import geo::f;"),
        ],
    );
    let (_, imported, result) = check(&directory);
    assert_eq!(result.unwrap_err(), ["Module 'geo' has errors"]);
    assert_eq!(imported.files[0].errors[0].message(), "Undefined name 'x'");

    let directory = project(
        "cycle",
        &[
            ("main.pk", "import a::f;"),
            ("a.pk", "import b::g; pub fn f() {}"),
            ("b.pk", "import a::f; pub fn g() {}"),
        ],
    );
    let (_, imported, _) = check(&directory);
    let b = imported
        .files
        .iter()
        .find(|file| file.module == "b")
        .unwrap();
    assert_eq!(
        b.errors[0].message(),
        "Could not import 'a': the imports form a cycle: a.pk -> b.pk -> a.pk"
    );
}

#[test]
fn imported_items_are_external_symbols() {
    let directory = project(
        "symbols",
        &[
            ("geo.pk", GEO),
            (
                "main.pk",
                "import geo::*;
                 fn main() -> s32 { count += 1; let p: Point = origin(); p.x }",
            ),
        ],
    );
    let (ast, imported, program) = check(&directory);
    let ir = codegen::lower(&ast, &program.unwrap(), Some("main.pk"))
        .and_then(|module| TextBackend.emit(&module))
        .unwrap();
    assert!(ir.contains("%\"geo::Point\" = type { i32, i32 }"), "{ir}");
    assert!(ir.contains("@_ZN3geo5countE = external global i32"), "{ir}");
    assert!(
        ir.contains("declare %\"geo::Point\" @_ZN3geo6originEv()"),
        "{ir}"
    );

    // `geo`'s own code defines them with external linkage, and keeps the
    // rest internal
    let geo = &imported.files[0];
    let ir = codegen::lower(&geo.ast, &geo.program, Some("geo.pk"))
        .and_then(|module| TextBackend.emit(&module))
        .unwrap();
    assert!(ir.contains("@_ZN3geo5countE = global i32 0"), "{ir}");
    assert!(
        ir.contains("define %\"geo::Point\" @_ZN3geo6originEv()"),
        "{ir}"
    );
    assert!(
        ir.contains("define internal void @_ZN3geo6helperEv()"),
        "{ir}"
    );
}
