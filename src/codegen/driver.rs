//! Builds executables from LLVM IR text using clang.

use std::path::Path;
use std::process::Command;

use super::CodegenError;

/// Compiles LLVM IR text to an executable at `output` with
/// `clang -O<optimization>`.
pub fn build_executable(ir: &str, output: &Path, optimization: u8) -> Result<(), CodegenError> {
    let directory = std::env::temp_dir().join(format!("minlang-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let stem = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("out");
    let source = directory.join(format!("{stem}.ll"));
    std::fs::write(&source, ir)?;

    let result = Command::new("clang")
        .arg("-x")
        .arg("ir")
        .arg(&source)
        .arg("-o")
        .arg(output)
        .arg(format!("-O{optimization}"))
        // The module has no target triple, so clang supplies the host's
        .arg("-Wno-override-module")
        .output();
    let _ = std::fs::remove_file(&source);
    let _ = std::fs::remove_dir(&directory);

    let result = result.map_err(|error| CodegenError::Tool {
        tool: "clang".to_owned(),
        message: error.to_string(),
    })?;
    if !result.status.success() {
        return Err(CodegenError::Tool {
            tool: "clang".to_owned(),
            message: String::from_utf8_lossy(&result.stderr).into_owned(),
        });
    }
    Ok(())
}
