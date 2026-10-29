use std::fmt;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser as ArgParser, Subcommand, ValueEnum};

use minlang::codegen::{self, Backend, TextBackend, mangle};
use minlang::diagnostic::Renderer;
use minlang::modules::{self, Imported, Prelude};
use minlang::{Ast, CheckedProgram, Checker, GenericError, Parser, TokenStream, Tokenizer};

/// The compiler for minlang, a small C-like language.
#[derive(ArgParser)]
#[command(name = "minlang", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Print how long each compiler stage takes
    #[arg(long, global = true)]
    time: bool,

    /// When to color error messages
    #[arg(long, global = true, value_enum, default_value_t = ColorChoice::Auto)]
    color: ColorChoice,
}

#[derive(Subcommand)]
enum Command {
    /// Compile a program and run it
    Run {
        /// The `.pk` file to compile
        file: PathBuf,

        /// Optimization level
        #[arg(short = 'O', default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
        optimization: u8,

        /// Arguments passed to the program
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Compile a program, and the files it imports, to an executable
    Build {
        /// The `.pk` file to compile
        file: PathBuf,

        /// Compile only this file, to an object file, without linking
        #[arg(short = 'c')]
        object: bool,

        /// Where to write the executable [default: the file's name, without
        /// `.pk`, or with `.o` instead for `-c`]
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Optimization level
        #[arg(short = 'O', default_value_t = 0, value_parser = clap::value_parser!(u8).range(0..=3))]
        optimization: u8,
    },

    /// Check a program for errors without compiling it
    Check {
        /// The `.pk` file to check
        file: PathBuf,
    },

    /// Run the language server, for editors, on standard input and output
    Lsp,

    /// Print what one stage of the compiler produces
    Emit {
        #[arg(value_enum)]
        stage: Stage,

        /// The `.pk` file to compile
        file: PathBuf,

        /// Write to this file instead of standard output
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Stage {
    /// The tokens, as JSON
    Tokens,
    /// The syntax tree, as JSON
    Ast,
    /// The LLVM IR, as `.ll` text
    Llvm,
}

#[derive(Clone, Copy, ValueEnum)]
enum ColorChoice {
    /// Color when writing to a terminal, unless NO_COLOR is set
    Auto,
    Always,
    Never,
}

/// Compiling stopped; the reason has already been printed.
struct Failed;

/// A file that passed the checker, with the files it imports.
struct Checked {
    ast: Ast,
    program: CheckedProgram,
    imported: Imported,
}

/// One source file going through the compiler.
struct Session {
    path: PathBuf,
    file: String,
    source: String,
    color: bool,
    time: bool,
}

impl Session {
    fn open(path: &Path, cli: &Cli) -> Result<Self, Failed> {
        let color = match cli.color {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none()
            }
        };
        let file = path.display().to_string();

        match std::fs::read_to_string(path) {
            Ok(source) => Ok(Self {
                path: path.to_owned(),
                file,
                source,
                color,
                time: cli.time,
            }),
            Err(error) => {
                let renderer = Renderer::new("", color);
                eprintln!(
                    "{}",
                    renderer.header(&format!("could not read `{file}`: {error}"))
                );
                Err(Failed)
            }
        }
    }

    fn timed<T>(&self, stage: &str, run: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let result = run();
        if self.time {
            eprintln!("{stage}: {:.3} ms", start.elapsed().as_secs_f64() * 1000.0);
        }
        result
    }

    /// Prints each error with its source line, then a summary.
    fn report<K: fmt::Display>(&self, errors: &[GenericError<K>]) -> Failed {
        self.report_file(&self.file, &self.source, errors)
    }

    fn report_file<K: fmt::Display>(
        &self,
        file: &str,
        source: &str,
        errors: &[GenericError<K>],
    ) -> Failed {
        let renderer = Renderer::new(source, self.color);
        for error in errors {
            eprintln!("{}\n", renderer.render(error));
        }
        let count = match errors.len() {
            1 => "an error".to_owned(),
            count => format!("{count} errors"),
        };
        eprintln!(
            "{}",
            renderer.header(&format!("could not compile `{file}` due to {count}"))
        );
        Failed
    }

    fn fail(&self, message: &str) -> Failed {
        eprintln!("{}", Renderer::new("", self.color).header(message));
        Failed
    }

    fn tokens(&self) -> Result<TokenStream, Failed> {
        let mut tokenizer = Tokenizer::new(self.source.clone(), Some(&self.file));
        self.timed("Tokenizing", || tokenizer.tokenize())
            .map_err(|errors| self.report(&errors))
    }

    fn ast(&self) -> Result<Ast, Failed> {
        let mut parser = Parser::new(self.tokens()?, Some(&self.file));
        self.timed("Parsing", || parser.parse().cloned())
            .map_err(|errors| self.report(errors))
    }

    /// Checks the file after the files it imports. If an imported file has
    /// errors, they're reported instead.
    fn check(&self) -> Result<Checked, Failed> {
        let ast = self.ast()?;
        let imported = self.timed("Loading imports", || {
            modules::load_imports(&self.path, &ast)
        });
        let prelude = self.timed("Loading the standard library", modules::prelude);
        if prelude.directory.is_none() {
            return Err(self.fail(
                "could not find the standard library; set MINLANG_STD to its `std` directory",
            ));
        }
        let mut failed = None;
        for file in prelude.files.iter().chain(&imported.files) {
            if !file.errors.is_empty() {
                failed = Some(self.report_file(&file.name(), &file.source, &file.errors));
            }
        }
        if let Some(failed) = failed {
            return Err(failed);
        }

        let imports = imported.imports.clone();
        let program = self
            .timed("Checking", || {
                Checker::new(Some(&self.file))
                    .with_imports(imports)
                    .check(&ast)
            })
            .map_err(|errors| self.report(&errors))?;
        Ok(Checked {
            ast,
            program,
            imported,
        })
    }

    fn lower(&self, file: &str, ast: &Ast, program: &CheckedProgram) -> Result<String, Failed> {
        codegen::lower(ast, program, Some(file))
            .and_then(|module| TextBackend.emit(&module))
            .map_err(|error| self.fail(&error.to_string()))
    }

    /// The LLVM IR of this file alone.
    fn llvm(&self) -> Result<String, Failed> {
        let checked = self.check()?;
        self.timed("Generating code", || {
            self.lower(&self.file, &checked.ast, &checked.program)
        })
    }

    /// Builds an executable from this file and the files it imports, or with
    /// `object`, an object file from this file alone.
    fn build(&self, output: &Path, optimization: u8, object: bool) -> Result<(), Failed> {
        let checked = self.check()?;
        let module = mangle::module_name(Some(&self.file));
        if let Some((first, second)) = checked.imported.clashing_modules(&module) {
            let first = if first.is_empty() { &self.file } else { &first };
            return Err(self.fail(&format!(
                "`{first}` and `{second}` have the same module name, so their symbols \
                 would clash; rename one"
            )));
        }

        let modules = self.timed("Generating code", || {
            let mut modules = vec![(
                module.clone(),
                self.lower(&self.file, &checked.ast, &checked.program)?,
            )];
            if !object {
                let prelude = modules::prelude();
                let standard = prelude
                    .files
                    .iter()
                    .filter(|file| Prelude::defines_code(file));
                for file in standard.chain(&checked.imported.files) {
                    let ir = self.lower(&file.name(), &file.ast, &file.program)?;
                    modules.push((file.module.clone(), ir));
                }
            }
            Ok(modules)
        })?;

        self.timed("Building", || {
            if object {
                codegen::driver::build_object(&modules[0].1, output, optimization)
            } else {
                let modules: Vec<_> = modules
                    .iter()
                    .map(|(name, ir)| (name.as_str(), ir.as_str()))
                    .collect();
                codegen::driver::link_executable(&modules, output, optimization)
            }
        })
        .map_err(|error| self.fail(&error.to_string()))
    }
}

fn write_output(session: &Session, output: Option<&Path>, text: &str) -> Result<(), Failed> {
    match output {
        Some(path) => std::fs::write(path, text).map_err(|error| {
            session.fail(&format!("could not write `{}`: {error}", path.display()))
        }),
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

/// Builds the program into a temporary directory and runs it, returning its
/// exit code.
fn run(session: &Session, path: &Path, optimization: u8, args: &[String]) -> Result<u8, Failed> {
    let directory = std::env::temp_dir().join(format!("minlang-run-{}", std::process::id()));
    std::fs::create_dir_all(&directory).map_err(|error| {
        session.fail(&format!("could not create a temporary directory: {error}"))
    })?;
    let stem = path.file_stem().unwrap_or_default();
    let executable = directory.join(stem);

    let built = session.build(&executable, optimization, false);
    let status = built.and_then(|()| {
        std::process::Command::new(&executable)
            .args(args)
            .status()
            .map_err(|error| session.fail(&format!("could not run the program: {error}")))
    });
    let _ = std::fs::remove_dir_all(&directory);

    let status = status?;
    // A program killed by a signal exits like a shell would report it
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        return Ok(128u8.wrapping_add(signal as u8));
    }
    Ok(status.code().unwrap_or(1) as u8)
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match &cli.command {
        Command::Lsp => match minlang::lsp::run() {
            Ok(()) => Ok(ExitCode::SUCCESS),
            Err(error) => {
                eprintln!("minlang lsp: {error}");
                Err(Failed)
            }
        },
        Command::Run {
            file,
            optimization,
            args,
        } => Session::open(file, &cli)
            .and_then(|session| run(&session, file, *optimization, args))
            .map(ExitCode::from),
        Command::Build {
            file,
            object,
            output,
            optimization,
        } => Session::open(file, &cli).and_then(|session| {
            let output = output.clone().unwrap_or_else(|| {
                let mut stem = file.file_stem().unwrap_or_default().to_owned();
                if *object {
                    stem.push(".o");
                }
                PathBuf::from(stem)
            });
            session.build(&output, *optimization, *object)?;
            Ok(ExitCode::SUCCESS)
        }),
        Command::Check { file } => Session::open(file, &cli).and_then(|session| {
            session.check()?;
            Ok(ExitCode::SUCCESS)
        }),
        Command::Emit {
            stage,
            file,
            output,
        } => Session::open(file, &cli).and_then(|session| {
            let text = match stage {
                Stage::Tokens => {
                    let tokens = session.tokens()?;
                    serde_json::to_string_pretty(&tokens).expect("tokens serialize to JSON") + "\n"
                }
                Stage::Ast => {
                    let ast = session.ast()?;
                    serde_json::to_string_pretty(&ast).expect("the AST serializes to JSON") + "\n"
                }
                Stage::Llvm => session.llvm()?,
            };
            write_output(&session, output.as_deref(), &text)?;
            Ok(ExitCode::SUCCESS)
        }),
    };

    result.unwrap_or(ExitCode::FAILURE)
}
