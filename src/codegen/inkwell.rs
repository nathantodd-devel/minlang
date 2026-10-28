//! The inkwell backend, which will build modules through LLVM's C API and
//! emit object files directly. Only a skeleton for now: enabling the
//! `inkwell` feature compiles it, but emitting reports that it isn't
//! implemented. To implement it, add the `inkwell` dependency in `Cargo.toml`
//! and translate each `ir` item with inkwell's builder.

use super::ir::Module;
use super::{Backend, CodegenError};

pub struct InkwellBackend;

impl Backend for InkwellBackend {
    /// Object file bytes.
    type Output = Vec<u8>;

    fn emit(&self, _module: &Module) -> Result<Vec<u8>, CodegenError> {
        Err(CodegenError::Unsupported(
            "the inkwell backend is not implemented yet".to_owned(),
        ))
    }
}
