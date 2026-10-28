use std::fmt;

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
    /// `iter T`: what a generator returns. It takes the whole type to its
    /// right, so `iter u8[4]` yields arrays.
    IteratorType { item: Box<TypeInfo> },
    /// `T[N]`, `T[?]`, optionally followed by an initializer: `u8[0x40]{0}`
    ArrayType {
        element: Box<TypeInfo>,
        size: Option<u64>,
        initializer: Option<Box<Expression>>,
    },
}

impl fmt::Display for TypeInfo {
    /// Formats the type as written in source, without any array initializer.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NamedType { name } => f.write_str(name),
            Self::PointerType { pointee } => write!(f, "*{pointee}"),
            Self::ReferenceType { referent } => write!(f, "&{referent}"),
            Self::IteratorType { item } => write!(f, "iter {item}"),
            Self::ArrayType { .. } => {
                // Dimensions are written outermost first: `u8[4][8]`
                let mut dimensions = String::new();
                let mut element = self;
                while let Self::ArrayType {
                    element: inner,
                    size,
                    ..
                } = element
                {
                    match size {
                        Some(size) => dimensions.push_str(&format!("[{size}]")),
                        None => dimensions.push_str("[?]"),
                    }
                    element = inner;
                }
                write!(f, "{element}{dimensions}")
            }
        }
    }
}
