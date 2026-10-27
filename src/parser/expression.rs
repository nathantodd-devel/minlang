use std::ops::Range;

use num_bigint::BigUint;
use serde::Serialize;

use super::typeinfo::TypeInfo;
use crate::tokenizer::{FloatSize, IntegerBase, IntegerSize, serialize_bigint};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Location {
    pub start: Position,
    pub end: Position,
}

/// Every construct in the language is an expression, including declarations
/// and control flow.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Expression {
    #[serde(flatten)]
    pub kind: ExpressionKind,
    pub position: Location,
    /// Indices into the parser's token stream that this expression spans.
    pub tokens: Range<usize>,
}

/// `{ a; b; c }`: `body` holds the `;`-terminated expressions and `result` the
/// optional trailing expression that gives the block its value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Block {
    pub body: Vec<Expression>,
    pub result: Option<Box<Expression>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Param {
    pub name: String,
    pub r#type: TypeInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Case {
    pub value: Expression,
    pub body: Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BinaryOperator {
    LogicalOr,
    LogicalAnd,
    Or,
    Xor,
    And,
    Eq,
    Neq,
    Lt,
    Gt,
    Le,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum UnaryOperator {
    /// `-x`
    Neg,
    /// `!x`
    Not,
    /// `*x`
    Deref,
    /// `&x`
    AddressOf,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum ExpressionKind {
    /// `import std::{printf, strcat}` → module `["std"]`, imports
    /// `["printf", "strcat"]`. A glob import is `"*"`.
    ImportExpression {
        module: Vec<String>,
        imports: Vec<String>,
    },
    TypeExpression {
        typeinfo: TypeInfo,
    },
    StringLiteralExpression {
        value: String,
    },
    IntegerLiteralExpression {
        base: IntegerBase,
        size: IntegerSize,
        signed: bool,
        #[serde(serialize_with = "serialize_bigint")]
        value: BigUint,
    },
    FloatLiteralExpression {
        value: f64,
        size: FloatSize,
    },
    BooleanLiteralExpression {
        value: bool,
    },
    /// A name, possibly namespaced: `x`, `std::printf`.
    IdentifierExpression {
        name: String,
    },
    /// `@`: the value currently being initialized.
    AtExpression,
    FunctionDeclarationExpression {
        name: String,
        params: Vec<Param>,
        return_type: Option<TypeInfo>,
        body: Block,
    },
    /// `let x: T = v` or `const x: T = v`
    LetExpression {
        constant: bool,
        name: String,
        typeinfo: Option<TypeInfo>,
        value: Option<Box<Expression>>,
    },
    StructDeclarationExpression {
        name: String,
        fields: Vec<Param>,
    },
    EnumDeclarationExpression {
        name: String,
        variants: Vec<String>,
    },
    BlockExpression {
        block: Block,
    },
    AssignmentExpression {
        target: Box<Expression>,
        value: Box<Expression>,
    },
    BinaryExpression {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    UnaryExpression {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    CallExpression {
        callee: Box<Expression>,
        args: Vec<Expression>,
    },
    IndexExpression {
        target: Box<Expression>,
        index: Box<Expression>,
    },
    MemberExpression {
        target: Box<Expression>,
        member: String,
    },
    IfExpression {
        condition: Box<Expression>,
        then: Block,
        /// Either another `IfExpression` (`else if`) or a `BlockExpression`.
        otherwise: Option<Box<Expression>>,
    },
    SwitchExpression {
        subject: Box<Expression>,
        cases: Vec<Case>,
        default: Option<Block>,
    },
    /// `loop { ... }`: repeats until a `break`.
    LoopExpression {
        body: Block,
    },
    /// `do { ... }` runs its body once; `do { ... } while (cond)` repeats it
    /// while `condition` holds, checking after each pass.
    DoExpression {
        body: Block,
        condition: Option<Box<Expression>>,
    },
    ForExpression {
        binding: String,
        iterable: Box<Expression>,
        body: Block,
    },
    ReturnExpression {
        value: Option<Box<Expression>>,
    },
    BreakExpression,
    ContinueExpression,
}

impl ExpressionKind {
    /// Whether this expression ends in a block and so needs no trailing `;`
    /// when used as a statement.
    pub fn is_block_like(&self) -> bool {
        matches!(
            self,
            Self::FunctionDeclarationExpression { .. }
                | Self::StructDeclarationExpression { .. }
                | Self::EnumDeclarationExpression { .. }
                | Self::BlockExpression { .. }
                | Self::IfExpression { .. }
                | Self::SwitchExpression { .. }
                | Self::ForExpression { .. }
                | Self::LoopExpression { .. }
                | Self::DoExpression {
                    condition: None,
                    ..
                }
        )
    }
}

pub type Ast = Vec<Expression>;
