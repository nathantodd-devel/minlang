use std::fmt;

use num_bigint::BigUint;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Token {
    #[serde(flatten)]
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum TokenKind {
    Identifier {
        name: String,
    },
    Keyword {
        keyword: Keyword,
    },
    IntegerLiteral {
        base: IntegerBase,
        size: IntegerSize,
        signed: bool,
        #[serde(serialize_with = "serialize_bigint")]
        value: BigUint,
        /// Whether the literal had a size suffix like `u8`. Without one, `size`
        /// and `signed` are the defaults and its type comes from context.
        suffixed: bool,
    },
    Punctuation {
        symbol: Punctuation,
    },
    StringLiteral {
        value: String,
    },
    FloatLiteral {
        value: f64,
        size: FloatSize,
        /// Whether the literal had a size suffix like `f32`.
        suffixed: bool,
    },
    BooleanLiteral {
        value: bool,
    },
}

impl TokenKind {
    pub fn token_type(&self) -> TokenType {
        match self {
            Self::Identifier { .. } => TokenType::Identifier,
            Self::Keyword { .. } => TokenType::Keyword,
            Self::IntegerLiteral { .. } => TokenType::IntegerLiteral,
            Self::Punctuation { .. } => TokenType::Punctuation,
            Self::StringLiteral { .. } => TokenType::StringLiteral,
            Self::FloatLiteral { .. } => TokenType::FloatLiteral,
            Self::BooleanLiteral { .. } => TokenType::BooleanLiteral,
        }
    }
}

/// Integers are emitted as strings in JSON so arbitrarily large literals
/// survive the round trip.
pub(crate) fn serialize_bigint<S: Serializer>(
    value: &BigUint,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Identifier,
    Keyword,
    IntegerLiteral,
    Punctuation,
    StringLiteral,
    FloatLiteral,
    BooleanLiteral,
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

pub type IntegerBase = u8; // 2 | 8 | 10 | 16
pub type IntegerSize = u8; // 8 | 16 | 32 | 64
pub type FloatSize = u8; // 32 | 64

/// Defines a fieldless enum whose variants map to fixed source strings.
macro_rules! string_enum {
    ($name:ident { $($variant:ident = $text:literal),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant),*
        }

        impl $name {
            pub const ALL: &[$name] = &[$($name::$variant),*];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),*
                }
            }

            pub fn lookup(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|value| value.as_str() == text)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    };
}

string_enum!(Keyword {
    Let = "let",
    Fn = "fn",
    Import = "import",
    Break = "break",
    If = "if",
    For = "for",
    In = "in",
    Switch = "switch",
    Case = "case",
    Default = "default",
    Struct = "struct",
    Enum = "enum",
    Const = "const",
    Return = "return",
    Continue = "continue",
    Else = "else",
    Do = "do",
    While = "while",
    Loop = "loop",
    Yield = "yield",
    Iter = "iter",
    As = "as",
    Pub = "pub",
});

string_enum!(Punctuation {
    Namespace = "::",
    Colon = ":",
    Semi = ";",
    RBrace = "}",
    LBrace = "{",
    RParen = ")",
    LParen = "(",
    RBracket = "]",
    LBracket = "[",
    At = "@",
    Star = "*",
    Question = "?",
    Point = "->",
    Or = "|",
    LogicalOr = "||",
    Xor = "^",
    And = "&",
    LogicalAnd = "&&",
    Assign = "=",
    Eq = "==",
    Neq = "!=",
    Not = "!",
    Comma = ",",
    Dot = ".",
    Mod = "%",
    Div = "/",
    SubNeg = "-",
    Plus = "+",
    Le = "<=",
    Ge = ">=",
    Lt = "<",
    Gt = ">",
    Shl = "<<",
    Shr = ">>",
    PlusAssign = "+=",
    SubAssign = "-=",
    MulAssign = "*=",
    DivAssign = "/=",
    ModAssign = "%=",
    AndAssign = "&=",
    OrAssign = "|=",
    XorAssign = "^=",
    ShlAssign = "<<=",
    ShrAssign = ">>=",
    Increment = "++",
    Decrement = "--",
});

pub type TokenStream = Vec<Token>;
