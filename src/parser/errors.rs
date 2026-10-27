use std::fmt;

use crate::error::GenericError;

pub type ParserError = GenericError<ParserErrorKind>;

#[derive(Debug, Clone, PartialEq)]
pub enum ParserErrorKind {
    Eof { expected: String },
    UnexpectedToken { expected: String, actual: String },
    InvalidArraySize(String),
}

impl fmt::Display for ParserErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eof { expected } => write!(f, "Unexpected end of file, expected {expected}"),
            Self::UnexpectedToken { expected, actual } => {
                write!(f, "Expected {expected}, but got '{actual}'")
            }
            Self::InvalidArraySize(raw) => write!(f, "Invalid array size '{raw}'"),
        }
    }
}
