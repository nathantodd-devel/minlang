use std::fmt;

use crate::error::GenericError;

pub type TokenizerError = GenericError<TokenizerErrorKind>;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenizerErrorKind {
    InvalidBitsize(String),
    InvalidEscapeSequence(String),
    UnexpectedCharacter(char),
    InvalidNumericLiteral(String),
    UnterminatedString,
    UnterminatedCharacter,
    /// A character literal that isn't one ASCII character, like `''` or `'ab'`.
    InvalidCharacter(String),
}

impl fmt::Display for TokenizerErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBitsize(bitsize) => write!(
                f,
                "Invalid bitsize '{bitsize}', expected one of [8, 16, 32, 64]"
            ),
            Self::InvalidEscapeSequence(sequence) => {
                write!(f, "Invalid escape sequence '{sequence}'")
            }
            Self::UnexpectedCharacter(char) => write!(f, "Unexpected character '{char}'"),
            Self::InvalidNumericLiteral(message) => f.write_str(message),
            Self::UnterminatedString => f.write_str("Unterminated string literal"),
            Self::UnterminatedCharacter => f.write_str("Unterminated character literal"),
            Self::InvalidCharacter(raw) => write!(
                f,
                "Invalid character literal {raw}: it must be one ASCII character or escape"
            ),
        }
    }
}
