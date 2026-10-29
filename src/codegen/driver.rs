//! Builds executables and object files from LLVM IR text using clang.

use std::path::Path;
use std::process::Command;

use super::CodegenError;

/// Compiles LLVM IR text to an executable at `output` with
/// `clang -O<optimization>`.
pub fn build_executable(ir: &str, output: &Path, optimization: u8) -> Result<(), CodegenError> {
    let stem = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("out");
    link_executable(&[(stem, ir)], output, optimization)
}

/// Compiles several modules, given as `(module name, LLVM IR text)`, each on
/// its own, and links them into an executable at `output`. The module names
/// must differ.
pub fn link_executable(
    modules: &[(&str, &str)],
    output: &Path,
    optimization: u8,
) -> Result<(), CodegenError> {
    clang(modules, output, optimization, false)
}

/// Compiles LLVM IR text to an object file at `output`, for linking later.
pub fn build_object(ir: &str, output: &Path, optimization: u8) -> Result<(), CodegenError> {
    let stem = output
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("out");
    clang(&[(stem, ir)], output, optimization, true)
}

fn clang(
    modules: &[(&str, &str)],
    output: &Path,
    optimization: u8,
    object: bool,
) -> Result<(), CodegenError> {
    let directory = std::env::temp_dir().join(format!("minlang-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let mut sources = Vec::new();
    for (name, ir) in modules {
        let source = directory.join(format!("{name}.ll"));
        sources.push(source.clone());
        if let Err(error) = std::fs::write(&source, ir) {
            remove(&sources, &directory);
            return Err(error.into());
        }
    }

    let mut command = Command::new("clang");
    command.arg("-x").arg("ir").args(&sources);
    if object {
        command.arg("-c");
    } else {
        // `std`'s math functions are in libm
        command.arg("-lm");
    }
    let result = command
        .arg("-o")
        .arg(output)
        .arg(format!("-O{optimization}"))
        // The module has no target triple, so clang supplies the host's
        .arg("-Wno-override-module")
        .output();
    remove(&sources, &directory);

    let result = result.map_err(|error| CodegenError::Tool {
        tool: "clang".to_owned(),
        message: if error.kind() == std::io::ErrorKind::NotFound {
            "clang was not found on the PATH; it's needed to build executables. \
             Install clang, or use `minlang emit llvm` to get the LLVM IR."
                .to_owned()
        } else {
            error.to_string()
        },
    })?;
    if !result.status.success() {
        return Err(CodegenError::Tool {
            tool: "clang".to_owned(),
            message: String::from_utf8_lossy(&result.stderr).into_owned(),
        });
    }
    Ok(())
}

fn remove(sources: &[std::path::PathBuf], directory: &Path) {
    for source in sources {
        let _ = std::fs::remove_file(source);
    }
    let _ = std::fs::remove_dir(directory);
}
