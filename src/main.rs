use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use minlang::codegen::{self, Backend, TextBackend};
use minlang::{Checker, Parser, Tokenizer};

fn print_usage() {
    eprintln!("Usage: MEcc [options] <file>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --ast          Parse the file and print the AST as JSON");
    eprintln!("  --check        Parse and check the file, reporting any errors");
    eprintln!("  --emit-llvm    Compile the file and print its LLVM IR");
    eprintln!("  -o <file>      Compile the file to an executable (needs clang)");
    eprintln!("  -O<n>          Optimization level for -o (0-3, default 0)");
}

#[derive(Default)]
struct Options {
    ast: bool,
    check: bool,
    emit_llvm: bool,
    output: Option<PathBuf>,
    optimization: u8,
    file: Option<String>,
}

fn parse_args() -> Option<Options> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ast" => options.ast = true,
            "--check" => options.check = true,
            "--emit-llvm" => options.emit_llvm = true,
            "-o" => options.output = Some(PathBuf::from(args.next()?)),
            _ if arg.starts_with("-O") => {
                options.optimization = arg[2..].parse().ok().filter(|level| *level <= 3)?;
            }
            _ if arg.starts_with('-') || options.file.is_some() => return None,
            _ => options.file = Some(arg),
        }
    }

    options.file.is_some().then_some(options)
}

fn timed<T>(stage: &str, run: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = run();
    eprintln!("{stage}: {:.3} ms", start.elapsed().as_secs_f64() * 1000.0);
    result
}

fn main() -> ExitCode {
    let Some(options) = parse_args() else {
        print_usage();
        return ExitCode::FAILURE;
    };
    let file = options.file.as_deref().expect("parse_args requires a file");

    let contents = match std::fs::read_to_string(file) {
        Ok(contents) => contents,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut lexer = Tokenizer::new(contents, Some(file));
    let tokens = match timed("Tokenization", || lexer.tokenize()) {
        Ok(tokens) => tokens,
        Err(errors) => {
            for e in &errors {
                eprintln!("{}", e.format());
            }
            return ExitCode::FAILURE;
        }
    };

    let compile = options.emit_llvm || options.output.is_some();
    if !options.ast && !options.check && !compile {
        println!(
            "{}",
            serde_json::to_string(&tokens).expect("tokens serialize to JSON")
        );
        return ExitCode::SUCCESS;
    }

    let mut parser = Parser::new(tokens, Some(file));
    let ast = match timed("Parsing", || parser.parse()) {
        Ok(ast) => ast,
        Err(errors) => {
            for e in errors {
                eprintln!("{}", e.format());
            }
            return ExitCode::FAILURE;
        }
    };

    if options.ast {
        println!(
            "{}",
            serde_json::to_string(ast).expect("AST serializes to JSON")
        );
    }

    if !options.check && !compile {
        return ExitCode::SUCCESS;
    }

    let mut checker = Checker::new(Some(file));
    let program = match timed("Checking", || checker.check(ast)) {
        Ok(program) => program,
        Err(errors) => {
            for e in &errors {
                eprintln!("{}", e.format());
            }
            return ExitCode::FAILURE;
        }
    };

    if !compile {
        return ExitCode::SUCCESS;
    }

    let ir = timed("Code generation", || {
        codegen::lower(ast, &program, Some(file)).and_then(|module| TextBackend.emit(&module))
    });
    let ir = match ir {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::FAILURE;
        }
    };

    if options.emit_llvm {
        print!("{ir}");
    }

    if let Some(output) = &options.output {
        let built = timed("Building", || {
            codegen::driver::build_executable(&ir, output, options.optimization)
        });
        if let Err(e) = built {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}
