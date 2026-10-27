use serde::Serialize;

use super::expression::Expression;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum TypeInfo {
    /// `s32`, `types::Foo`
    NamedType { name: String },
    /// `*T`
    PointerType { pointee: Box<TypeInfo> },
    /// `&T`
    ReferenceType { referent: Box<TypeInfo> },
    /// `T[N]`, `T[?]`, optionally followed by an initializer: `u8[0x40]{0}`
    ArrayType {
        element: Box<TypeInfo>,
        size: Option<u64>,
        initializer: Option<Box<Expression>>,
    },
}
