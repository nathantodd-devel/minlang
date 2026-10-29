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
    /// Unique among the expressions and patterns of one parse, so later
    /// passes can attach information to it.
    pub id: usize,
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

/// `name: value` inside a struct literal.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldInit {
    pub name: String,
    pub value: Expression,
}

/// A variant inside an enum declaration: `Name`, `Name(T, U)` or
/// `Name { a: T }`, optionally followed by `= value`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnumVariant {
    pub name: String,
    pub fields: VariantFields,
    pub value: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum VariantFields {
    /// `Empty`
    Unit,
    /// `Circle(f32)`
    Tuple { types: Vec<TypeInfo> },
    /// `Rect { w: f32, h: f32 }`
    Struct { fields: Vec<Param> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pattern {
    #[serde(flatten)]
    pub kind: PatternKind,
    /// Unique among the expressions and patterns of one parse.
    pub id: usize,
    pub position: Location,
    /// Indices into the parser's token stream that this pattern spans.
    pub tokens: Range<usize>,
}

/// What a `case` matches against. At the top of a `case` a bare name is a
/// value (`case LIMIT`); inside a variant pattern it binds that part of the
/// matched value (`case Shape::Circle(radius)`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum PatternKind {
    /// `_`
    WildcardPattern,
    /// `radius` in `Shape::Circle(radius)`
    BindingPattern { name: String },
    /// `1.5`, `-1`, `Color::Red`, `LIMIT + 1`
    ValuePattern { value: Expression },
    /// `Shape::Circle(r)`
    TupleVariantPattern {
        path: String,
        elements: Vec<Pattern>,
    },
    /// `Shape::Rect { w, h: height }`
    StructVariantPattern {
        path: String,
        fields: Vec<FieldPattern>,
    },
}

/// `name` (binds `name`) or `name: pattern` inside a struct variant pattern.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldPattern {
    pub name: String,
    pub pattern: Pattern,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Case {
    pub pattern: Pattern,
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
    Shl,
    Shr,
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
        /// Whether the literal had a size suffix like `u8`. Without one, `size`
        /// and `signed` are the defaults and its type comes from context.
        suffixed: bool,
    },
    FloatLiteralExpression {
        value: f64,
        size: FloatSize,
        /// Whether the literal had a size suffix like `f32`.
        suffixed: bool,
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
    /// `fn`, `let`, `const`, `struct` and `enum` take `public` from a leading
    /// `pub`, which lets other files import them.
    FunctionDeclarationExpression {
        public: bool,
        name: String,
        params: Vec<Param>,
        return_type: Option<TypeInfo>,
        body: Block,
    },
    /// `let x: T = v` or `const x: T = v`
    LetExpression {
        public: bool,
        constant: bool,
        name: String,
        typeinfo: Option<TypeInfo>,
        value: Option<Box<Expression>>,
    },
    StructDeclarationExpression {
        public: bool,
        name: String,
        fields: Vec<Param>,
    },
    /// `enum Name: u8 { A = 1, B }`, with the backing type and values
    /// optional.
    EnumDeclarationExpression {
        public: bool,
        name: String,
        backing_type: Option<TypeInfo>,
        variants: Vec<EnumVariant>,
    },
    /// `Point { x: 1, y: 2 }`
    StructLiteralExpression {
        name: String,
        fields: Vec<FieldInit>,
    },
    BlockExpression {
        block: Block,
    },
    /// `a = b`, or with `operator` set, a compound assignment like `a += b`.
    AssignmentExpression {
        operator: Option<BinaryOperator>,
        target: Box<Expression>,
        value: Box<Expression>,
    },
    /// `value as T`
    CastExpression {
        value: Box<Expression>,
        typeinfo: TypeInfo,
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
    /// Cases fall through to the next one unless they end in `break`, as in C.
    SwitchExpression {
        subject: Box<Expression>,
        cases: Vec<Case>,
        default: Option<Block>,
    },
    /// `while (cond) { ... }`
    WhileExpression {
        condition: Box<Expression>,
        body: Block,
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
    /// `for x in xs { ... }`
    ForExpression {
        binding: String,
        iterable: Box<Expression>,
        body: Block,
    },
    /// `for (init; condition; step; let item = ...) { ... }`. Each pass runs
    /// the condition, then `each` (if any), then the body, then the step. Any
    /// clause may be left out.
    ForClauseExpression {
        init: Option<Box<Expression>>,
        condition: Option<Box<Expression>>,
        step: Option<Box<Expression>>,
        /// A `let` whose variable is visible in the body.
        each: Option<Box<Expression>>,
        body: Block,
    },
    /// `x++` or `x--`, which are statements rather than expressions.
    IncrementExpression {
        target: Box<Expression>,
        decrement: bool,
    },
    ReturnExpression {
        value: Option<Box<Expression>>,
    },
    /// `yield value`: produces the next item of a generator, a function whose
    /// return type is `iter T`.
    YieldExpression {
        value: Box<Expression>,
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
                | Self::ForClauseExpression { .. }
                | Self::LoopExpression { .. }
                | Self::WhileExpression { .. }
                | Self::DoExpression {
                    condition: None,
                    ..
                }
        )
    }
}

pub type Ast = Vec<Expression>;
