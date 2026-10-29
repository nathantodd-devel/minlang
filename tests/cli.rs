//! Tests of the `minlang` command-line tool.

use std::path::PathBuf;
use std::process::{Command, Output};

fn minlang(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_minlang"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn have_clang() -> bool {
    let found = Command::new("clang").arg("--version").output().is_ok();
    if !found {
        eprintln!("skipping: clang not found");
    }
    found
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("minlang-cli-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory.join(name)
}

fn write_program(name: &str, source: &str) -> String {
    let path = scratch(name);
    std::fs::write(&path, source).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn help_lists_the_commands() {
    let output = minlang(&["--help"]);
    assert!(output.status.success());
    for command in ["run", "build", "check", "emit"] {
        assert!(stdout(&output).contains(command), "{}", stdout(&output));
    }
    assert!(stdout(&minlang(&["--version"])).starts_with("minlang "));
}

#[test]
fn missing_arguments_are_usage_errors() {
    let output = minlang(&["run"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("Usage: minlang run"));
}

#[test]
fn check_is_silent_on_success() {
    let output = minlang(&["check", "examples/hello.pk"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output), "");
    assert_eq!(stderr(&output), "");
}

#[test]
fn errors_show_the_source_line() {
    let file = write_program(
        "bad.pk",
        "fn f() -> s32 {\n    let a = count + 1;\n    a\n}\n",
    );
    let output = minlang(&["check", &file]);
    assert_eq!(output.status.code(), Some(1));
    let expected = format!(
        "error: Undefined name 'count'\n \
         --> {file}:2:13\n  \
         |\n\
         2 |     let a = count + 1;\n  \
         |             ^^^^^\n\n\
         error: could not compile `{file}` due to an error\n"
    );
    assert_eq!(stderr(&output), expected);
}

#[test]
fn errors_from_every_stage_are_rendered() {
    let lexer = write_program("lex.pk", "let a = $;\n");
    assert!(stderr(&minlang(&["check", &lexer])).contains("Unexpected character '$'"));

    let parser = write_program("parse.pk", "let a = ;\nlet b = ;\n");
    let output = stderr(&minlang(&["check", &parser]));
    assert!(output.contains("2 | let b = ;"), "{output}");
    assert!(output.ends_with("due to 2 errors\n"), "{output}");
}

#[test]
fn colors_only_when_asked() {
    let file = write_program("color.pk", "let a = b;\n");
    assert!(!stderr(&minlang(&["check", &file])).contains('\x1b'));
    assert!(stderr(&minlang(&["--color", "always", "check", &file])).contains("\x1b[1;31merror"));
}

#[test]
fn missing_files_are_reported() {
    let output = minlang(&["check", "does-not-exist.pk"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with("error: could not read `does-not-exist.pk`"));
}

#[test]
fn emit_prints_each_stage() {
    let tokens = minlang(&["emit", "tokens", "examples/basic.pk"]);
    assert!(stdout(&tokens).contains("\"type\": \"Keyword\""));

    let ast = minlang(&["emit", "ast", "examples/basic.pk"]);
    assert!(stdout(&ast).contains("\"type\": \"FunctionDeclarationExpression\""));

    let llvm = minlang(&["emit", "llvm", "examples/basic.pk"]);
    assert!(stdout(&llvm).contains("define i32 @main("));

    let path = scratch("basic.ll");
    let output = minlang(&[
        "emit",
        "llvm",
        "examples/basic.pk",
        "-o",
        path.to_str().unwrap(),
    ]);
    assert_eq!(stdout(&output), "");
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("define i32 @main(")
    );
}

#[test]
fn timings_only_with_time() {
    assert_eq!(
        stderr(&minlang(&["emit", "tokens", "examples/basic.pk"])),
        ""
    );
    let output = stderr(&minlang(&["--time", "emit", "llvm", "examples/basic.pk"]));
    for stage in ["Tokenizing", "Parsing", "Checking", "Generating code"] {
        assert!(output.contains(stage), "{output}");
    }
}

#[test]
fn run_passes_arguments_and_exit_code() {
    if !have_clang() {
        return;
    }
    let output = minlang(&["run", "examples/hello.pk", "--", "a", "b"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("2 arguments\n"));

    let output = minlang(&["run", "-O2", "tests/programs/functions.pk"]);
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(stdout(&output), "2432902008176640000\n24\n");
}

#[test]
fn build_names_the_output_after_the_file() {
    if !have_clang() {
        return;
    }
    let directory = scratch("build");
    std::fs::create_dir_all(&directory).unwrap();
    let source = std::fs::canonicalize("examples/structs.pk").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_minlang"))
        .args(["build", source.to_str().unwrap()])
        .current_dir(&directory)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));

    let built = Command::new(directory.join("structs")).output().unwrap();
    assert!(stdout(&built).starts_with("area: 10.000000\n"));
}

/// Writes `files`, as (relative path, source), into a scratch directory.
fn write_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let directory = scratch(name);
    let _ = std::fs::remove_dir_all(&directory);
    for (path, source) in files {
        let path = directory.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    directory
}

const GEO: &str = "pub struct Point { x: s32, y: s32 }
pub fn add(a: Point, b: Point) -> Point { Point { x: a.x + b.x, y: a.y + b.y } }
";

const MAIN: &str = "import std::printf;
import shapes::geo;

fn main() -> s32 {
    let p: geo::Point = geo::add(geo::Point { x: 1, y: 2 }, geo::Point { x: 3, y: 4 });
    printf(\"%d %d\\n\", p.x, p.y);
    0
}
";

#[test]
fn run_links_imported_files() {
    if !have_clang() {
        return;
    }
    let directory = write_project("imports", &[("shapes/geo.pk", GEO), ("main.pk", MAIN)]);
    let output = minlang(&["run", directory.join("main.pk").to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "4 6\n");
}

#[test]
fn build_c_compiles_one_file_to_an_object() {
    if !have_clang() {
        return;
    }
    let directory = write_project("objects", &[("shapes/geo.pk", GEO), ("main.pk", MAIN)]);
    let build = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_minlang"))
            .args(args)
            .current_dir(&directory)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
    };
    // Named after the file, with `.o`, unless `-o` says otherwise
    build(&["build", "-c", "main.pk"]);
    build(&["build", "-c", "shapes/geo.pk", "-o", "geo-object.o"]);
    assert!(directory.join("main.o").is_file());

    let link = Command::new("clang")
        .args(["main.o", "geo-object.o", "-o", "linked"])
        .current_dir(&directory)
        .output()
        .unwrap();
    assert!(link.status.success(), "{}", stderr(&link));
    let output = Command::new(directory.join("linked")).output().unwrap();
    assert_eq!(stdout(&output), "4 6\n");
}

#[test]
fn errors_in_imported_files_show_their_source() {
    let directory = write_project(
        "import-errors",
        &[
            ("geo.pk", "pub fn f() -> s32 {\n    count\n}\n"),
            ("main.pk", "import geo::f;\n"),
        ],
    );
    let output = minlang(&["check", directory.join("main.pk").to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let geo = directory.join("geo.pk");
    let geo = geo.display();
    let expected = format!(
        "error: Undefined name 'count'\n \
         --> {geo}:2:5\n  \
         |\n\
         2 |     count\n  \
         |     ^^^^^\n\n\
         error: could not compile `{geo}` due to an error\n"
    );
    assert_eq!(stderr(&output), expected);
}

#[test]
fn modules_with_the_same_name_are_rejected() {
    let directory = write_project(
        "clash",
        &[
            ("a/geo.pk", "pub fn f() {}"),
            (
                "geo.pk",
                "import a::geo; pub fn main() -> s32 { geo::f(); 0 }",
            ),
        ],
    );
    let output = minlang(&["build", directory.join("geo.pk").to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("have the same module name"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn modules_example_produces_expected_output() {
    if !have_clang() {
        return;
    }
    let output = minlang(&["run", "examples/modules/main.pk"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let expected = std::fs::read_to_string("examples/modules/main.out").unwrap();
    assert_eq!(stdout(&output), expected);
}
