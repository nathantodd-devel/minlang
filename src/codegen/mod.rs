//! Code generation: lowering a checked program to LLVM IR, then emitting it
//! through a backend.
//!
//! Lowering produces a backend-neutral [`ir::Module`]. The text backend writes
//! it as `.ll`, which [`driver`] compiles with clang. An inkwell backend can
//! be added behind the `inkwell` feature without changing the lowering.

pub mod driver;
pub mod ir;
pub mod layout;
mod lower;
pub mod mangle;
pub mod text;

#[cfg(feature = "inkwell")]
pub mod inkwell;

use std::fmt;

pub use lower::lower;
pub use text::TextBackend;

/// Turns an IR module into some output, like `.ll` text or an object file.
pub trait Backend {
    type Output;

    fn emit(&self, module: &ir::Module) -> Result<Self::Output, CodegenError>;
}

#[derive(Debug)]
pub enum CodegenError {
    /// A construct code generation can't handle yet.
    Unsupported(String),
    /// An external tool, like clang, failed.
    Tool {
        tool: String,
        message: String,
    },
    Io(std::io::Error),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) => f.write_str(message),
            Self::Tool { tool, message } => write!(f, "{tool} failed:\n{message}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CodegenError {}

impl From<std::io::Error> for CodegenError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
