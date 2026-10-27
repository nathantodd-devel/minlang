use std::process::ExitCode;
use std::time::Instant;

use minlang::{Parser, Tokenizer};

fn print_usage() {
    eprintln!("Usage: MEcc [options] <file>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --ast    Parse the file and print the AST as JSON");
}

fn main() -> ExitCode {
    let mut ast = false;
    let mut file = None;

    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--ast" => ast = true,
            _ if arg.starts_with("--") || file.is_some() => {
                print_usage();
                return ExitCode::FAILURE;
            }
            _ => file = Some(arg),
        }
    }

    let Some(file) = file else {
        print_usage();
        return ExitCode::FAILURE;
    };

    let contents = match std::fs::read_to_string(&file) {
        Ok(contents) => contents,
        Err(e) => {
            eprintln!("{file}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut lexer = Tokenizer::new(contents, Some(&file));
    let start = Instant::now();
    let res = lexer.tokenize();
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    let tokens = match res {
        Ok(tokens) => tokens,
        Err(errors) => {
            for e in &errors {
                eprintln!("{}", e.format());
            }
            return ExitCode::FAILURE;
        }
    };

    eprintln!("Tokenization: {elapsed_ms:.3} ms");

    if !ast {
        println!(
            "{}",
            serde_json::to_string(&tokens).expect("tokens serialize to JSON")
        );
        return ExitCode::SUCCESS;
    }

    let mut parser = Parser::new(tokens, Some(&file));
    let start = Instant::now();
    let res = parser.parse();
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

    match res {
        Ok(node) => println!(
            "{}",
            serde_json::to_string(node).expect("AST serializes to JSON")
        ),
        Err(errors) => {
            for e in errors {
                eprintln!("{}", e.format());
            }
            return ExitCode::FAILURE;
        }
    }
    eprintln!("Parsing: {elapsed_ms:.3} ms");

    ExitCode::SUCCESS
}
