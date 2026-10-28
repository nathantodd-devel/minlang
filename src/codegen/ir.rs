//! A backend-neutral representation of an LLVM module. Lowering builds one of
//! these; each backend (text, and later inkwell) only translates these
//! LLVM-shaped instructions, so the lowering logic is written once.

#[derive(Debug, Clone, PartialEq)]
pub enum IrType {
    Void,
    /// An integer of the given number of bits, `i1` to `i64`.
    Int(u32),
    Float,
    Double,
    Ptr,
    Array(u64, Box<IrType>),
    /// A literal struct type: `{ i32, ptr }`.
    Struct(Vec<IrType>),
    /// A named struct type defined in the module: `%"geo::Point"`.
    Named(String),
    /// LLVM's `token` type, used by coroutine intrinsics.
    Token,
}

impl IrType {
    pub const BOOL: IrType = IrType::Int(1);
    pub const I8: IrType = IrType::Int(8);
    pub const I32: IrType = IrType::Int(32);
    pub const I64: IrType = IrType::Int(64);
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A function-local value or parameter: `%name`.
    Local(String),
    /// A global variable or function: `@name`.
    Global(String),
    /// An integer constant, already wrapped to its type's width.
    Int(i128),
    Float(f64),
    Null,
    Poison,
    ZeroInitializer,
    /// A constant array of bytes: `c"..."`.
    Bytes(Vec<u8>),
    /// A constant array or struct of typed elements.
    Aggregate(Vec<Typed>),
    /// LLVM's `none` token.
    NoneToken,
}

/// A value together with its type, as LLVM operands are written.
#[derive(Debug, Clone, PartialEq)]
pub struct Typed {
    pub ty: IrType,
    pub value: Value,
}

impl Typed {
    pub fn new(ty: IrType, value: Value) -> Self {
        Self { ty, value }
    }

    pub fn int(ty: IrType, value: i128) -> Self {
        Self::new(ty, Value::Int(value))
    }

    pub fn bool(value: bool) -> Self {
        Self::int(IrType::BOOL, i128::from(value))
    }

    pub fn ptr(value: Value) -> Self {
        Self::new(IrType::Ptr, value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    SDiv,
    UDiv,
    SRem,
    URem,
    Shl,
    LShr,
    AShr,
    And,
    Or,
    Xor,
    FAdd,
    FSub,
    FMul,
    FDiv,
    FRem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntPredicate {
    Eq,
    Ne,
    Slt,
    Sgt,
    Sle,
    Sge,
    Ult,
    Ugt,
    Ule,
    Uge,
}

/// Float comparisons. The `O` ones are false if either side is NaN; `Une`
/// (used for `!=`) is true then, so NaN != NaN as in C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatPredicate {
    Oeq,
    Une,
    Olt,
    Ogt,
    Ole,
    Oge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastOp {
    Trunc,
    ZExt,
    SExt,
    FPTrunc,
    FPExt,
    UIToFP,
    SIToFP,
    PtrToInt,
    IntToPtr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// Allocas are gathered into the entry block by the function builder.
    Alloca {
        result: String,
        ty: IrType,
    },
    Load {
        result: String,
        ty: IrType,
        ptr: Value,
    },
    Store {
        value: Typed,
        ptr: Value,
    },
    Binary {
        result: String,
        op: BinaryOp,
        ty: IrType,
        lhs: Value,
        rhs: Value,
    },
    ICmp {
        result: String,
        predicate: IntPredicate,
        ty: IrType,
        lhs: Value,
        rhs: Value,
    },
    FCmp {
        result: String,
        predicate: FloatPredicate,
        ty: IrType,
        lhs: Value,
        rhs: Value,
    },
    Cast {
        result: String,
        op: CastOp,
        value: Typed,
        to: IrType,
    },
    /// `getelementptr inbounds`
    GetElementPtr {
        result: String,
        base: IrType,
        ptr: Value,
        indices: Vec<Typed>,
    },
    InsertValue {
        result: String,
        aggregate: Typed,
        value: Typed,
        index: u32,
    },
    Call {
        result: Option<String>,
        return_type: IrType,
        /// Set for calls to variadic functions, which must spell out the
        /// callee's parameter types.
        variadic_params: Option<Vec<IrType>>,
        callee: Value,
        args: Vec<Typed>,
    },
    Phi {
        result: String,
        ty: IrType,
        incoming: Vec<(Value, String)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    Ret(Option<Typed>),
    Br(String),
    CondBr {
        condition: Value,
        then: String,
        otherwise: String,
    },
    Switch {
        value: Typed,
        default: String,
        cases: Vec<(Typed, String)>,
    },
    Unreachable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub label: String,
    pub instructions: Vec<Instruction>,
    pub terminator: Option<Terminator>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Linkage {
    External,
    Internal,
    Private,
    Appending,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub linkage: Linkage,
    pub return_type: IrType,
    pub params: Vec<(IrType, String)>,
    /// Function attributes, like `presplitcoroutine`.
    pub attributes: Vec<String>,
    pub blocks: Vec<Block>,
}

/// An external function, like `printf`.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub return_type: IrType,
    pub params: Vec<IrType>,
    pub variadic: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Global {
    pub name: String,
    pub linkage: Linkage,
    /// Read-only, like string literals.
    pub constant: bool,
    /// Whether its address is insignificant, so equal constants may merge.
    pub unnamed_addr: bool,
    pub init: Typed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDefinition {
    pub name: String,
    pub fields: Vec<IrType>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Module {
    pub name: String,
    pub types: Vec<TypeDefinition>,
    pub globals: Vec<Global>,
    pub declarations: Vec<Declaration>,
    pub functions: Vec<Function>,
}
