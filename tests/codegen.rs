//! End-to-end tests: compile programs to executables with clang, run them,
//! and compare their output. Skipped (with a message) if clang is missing.

use std::path::{Path, PathBuf};
use std::process::Command;

use minlang::codegen::{self, Backend, TextBackend};
use minlang::{Checker, Parser, Tokenizer};

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("minlang-test-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    directory.join(name)
}

fn compile_ir(path: &Path) -> String {
    let file = path.to_str().unwrap();
    let source = std::fs::read_to_string(path).unwrap();
    let tokens = Tokenizer::new(source, Some(file))
        .tokenize()
        .unwrap_or_else(|errors| panic!("{file}: tokenizer failed: {errors:?}"));
    let mut parser = Parser::new(tokens, Some(file));
    let ast = parser
        .parse()
        .unwrap_or_else(|errors| panic!("{file}: parser failed: {errors:?}"))
        .clone();
    let program = Checker::new(Some(file))
        .check(&ast)
        .unwrap_or_else(|errors| panic!("{file}: checker failed: {errors:?}"));
    let module = codegen::lower(&ast, &program, Some(file)).unwrap();
    TextBackend.emit(&module).unwrap()
}

/// Checks the IR with LLVM's verifier, if `opt` is installed.
fn verify(ir: &str, name: &str) {
    if !have("opt") {
        return;
    }
    let path = scratch(&format!("{name}.ll"));
    std::fs::write(&path, ir).unwrap();
    let output = Command::new("opt")
        .args(["-passes=verify", "-disable-output"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{name}: invalid IR:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Compiles and runs `path`, returning its output and exit code.
fn run(path: &Path, optimization: u8) -> (String, i32) {
    let name = path.file_stem().unwrap().to_str().unwrap();
    let ir = compile_ir(path);
    verify(&ir, name);

    let executable = scratch(&format!("{name}-O{optimization}"));
    codegen::driver::build_executable(&ir, &executable, optimization)
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    let output = Command::new(&executable).output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

/// The exit code a program expects, from a `// exit: N` first line.
fn expected_exit(path: &Path) -> i32 {
    let source = std::fs::read_to_string(path).unwrap();
    source
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("// exit: "))
        .map_or(0, |code| code.trim().parse().unwrap())
}

fn check_outputs(paths: &[PathBuf], optimization: u8) {
    if !have("clang") {
        eprintln!("skipping: clang not found");
        return;
    }
    for path in paths {
        let expected = std::fs::read_to_string(path.with_extension("out"))
            .unwrap_or_else(|_| panic!("{}: missing .out file", path.display()));
        let (output, code) = run(path, optimization);
        assert_eq!(output, expected, "{} at -O{optimization}", path.display());
        assert_eq!(code, expected_exit(path), "{} exit code", path.display());
    }
}

fn programs() -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir("tests/programs")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pk"))
        .collect();
    paths.sort();
    paths
}

#[test]
fn programs_produce_expected_output() {
    check_outputs(&programs(), 0);
}

#[test]
fn programs_behave_the_same_optimized() {
    check_outputs(&programs(), 2);
}

#[test]
fn examples_produce_expected_output() {
    let examples: Vec<_> = ["basic", "hello", "structs", "iterators"]
        .iter()
        .map(|name| PathBuf::from(format!("examples/{name}.pk")))
        .collect();
    check_outputs(&examples, 0);
}

#[test]
fn symbols_are_mangled() {
    let ir = compile_ir(Path::new("tests/programs/functions.pk"));
    for symbol in [
        "@main(",
        "@_ZN9functions9factorialEy(",
        "@_ZN9functions5outerEi(",
        "@_ZN9functions5outer6helperEi(",
        "@_ZN9functions5outer5twiceEi(",
        "@_ZN9functions5outer2$25twiceEi(",
    ] {
        assert!(ir.contains(symbol), "missing {symbol} in:\n{ir}");
    }
    // Everything but `main` is internal
    assert!(ir.contains("define i32 @main("));
    assert!(ir.contains("define internal i64 @_ZN9functions9factorialEy("));
}

#[test]
fn mangled_names_demangle() {
    if !have("c++filt") {
        eprintln!("skipping: c++filt not found");
        return;
    }
    let names = [
        (
            "_ZN9functions5outer2$25twiceEi",
            "functions::outer::$2::twice(int)",
        ),
        (
            "_ZN9iterators9bytes_getERN9iterators5BytesEm",
            "iterators::bytes_get(iterators::Bytes&, unsigned long)",
        ),
        (
            "_ZN9iterators7first_nE4iterIyEi",
            "iterators::first_n(iter<unsigned long long>, int)",
        ),
        (
            "_ZN9iterators5rangeEii.resume",
            "iterators::range(int, int) [clone .resume]",
        ),
    ];
    let input: Vec<_> = names.iter().map(|(mangled, _)| *mangled).collect();
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "printf '%s\\n' {} | c++filt",
            input
                .iter()
                .map(|name| format!("'{name}'"))
                .collect::<Vec<_>>()
                .join(" ")
        ))
        .output()
        .unwrap();
    let demangled = String::from_utf8_lossy(&output.stdout);
    let expected: Vec<_> = names.iter().map(|(_, demangled)| *demangled).collect();
    assert_eq!(demangled.lines().collect::<Vec<_>>(), expected);
}

#[test]
fn iterator_example_uses_expected_symbols() {
    let ir = compile_ir(Path::new("examples/iterators.pk"));
    for symbol in [
        "_ZN9iterators9bytes_getERN9iterators5BytesEm",
        "_ZN9iterators7first_nE4iterIyEi",
        "_ZN9iterators5rangeEii",
    ] {
        assert!(ir.contains(symbol), "missing {symbol}");
    }
}
