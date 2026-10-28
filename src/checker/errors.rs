use std::fmt;

use crate::error::GenericError;

pub type CheckError = GenericError<CheckErrorKind>;

/// How an enum variant carries its data, for explaining how to use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantStyle {
    Unit,
    Tuple,
    Struct,
}

/// A construct that is only valid in certain places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Construct {
    Return,
    Yield,
    Break,
    Continue,
    At,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckErrorKind {
    UndefinedName(String),
    UnknownModule(String),
    /// `container` has no variant or member called `member`.
    UnknownMember {
        container: String,
        member: String,
    },
    /// `name::x` where `name` is neither an enum nor a module.
    NoMembers(String),
    NotAType(String),
    NotAValue(String),
    NotAStruct(String),
    NotAVariant(String),
    Redeclared(String),
    Duplicate {
        what: &'static str,
        name: String,
    },
    UnknownField {
        container: String,
        field: String,
    },
    MissingFields {
        container: String,
        fields: Vec<String>,
    },
    ArgumentCount {
        name: String,
        expected: usize,
        found: usize,
    },
    /// A tuple variant pattern with the wrong number of elements.
    FieldCount {
        name: String,
        expected: usize,
        found: usize,
    },
    /// A variant used in a way its fields don't allow, like calling a unit
    /// variant.
    VariantStyle {
        path: String,
        style: VariantStyle,
    },
    InvalidBackingType(String),
    Misplaced(Construct),
    TopLevelStatement,
    ConstantWithoutValue(String),
    AssignToConstant(String),
    InvalidAssignmentTarget,
    GeneratorReturnValue,
    MissingReturnValue {
        function: String,
        return_type: String,
    },
    UnexpectedReturnValue(String),
    FallthroughIntoBindings,
    TypeMismatch {
        expected: String,
        found: String,
        /// Both are numbers, so an `as` cast could convert between them.
        cast_hint: bool,
    },
    LiteralOutOfRange {
        value: String,
        expected: String,
    },
    InvalidOperands {
        operator: &'static str,
        left: String,
        right: String,
    },
    InvalidOperand {
        operator: &'static str,
        operand: String,
    },
    InvalidCast {
        from: String,
        to: String,
    },
    NotCallable(String),
    NotIndexable(String),
    NotIterable(String),
    FunctionAsValue(String),
    CannotInfer(String),
    NoValue(String),
    MaybeUnassigned(String),
    Captured(String),
    AtNeedsType,
    MissingReturn {
        function: String,
        return_type: String,
    },
    PatternMismatch {
        path: String,
        subject: String,
    },
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("{count} {word}")
    } else {
        format!("{count} {word}s")
    }
}

impl fmt::Display for CheckErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndefinedName(name) => write!(f, "Undefined name '{name}'"),
            Self::UnknownModule(name) => write!(f, "Unknown module '{name}'"),
            Self::UnknownMember { container, member } => {
                write!(f, "'{container}' has no member '{member}'")
            }
            Self::NoMembers(name) => write!(f, "'{name}' is not an enum or module"),
            Self::NotAType(name) => write!(f, "'{name}' is not a type"),
            Self::NotAValue(name) => write!(f, "'{name}' is a type, not a value"),
            Self::NotAStruct(name) => write!(f, "'{name}' is not a struct"),
            Self::NotAVariant(name) => write!(f, "'{name}' is not an enum variant"),
            Self::Redeclared(name) => write!(f, "'{name}' is already declared in this scope"),
            Self::Duplicate { what, name } => write!(f, "Duplicate {what} '{name}'"),
            Self::UnknownField { container, field } => {
                write!(f, "'{container}' has no field '{field}'")
            }
            Self::MissingFields { container, fields } => {
                write!(f, "Missing fields in '{container}': {}", fields.join(", "))
            }
            Self::ArgumentCount {
                name,
                expected,
                found,
            } => write!(
                f,
                "'{name}' takes {}, but {} given",
                plural(*expected, "argument"),
                if *found == 1 {
                    "1 was".to_owned()
                } else {
                    format!("{found} were")
                }
            ),
            Self::FieldCount {
                name,
                expected,
                found,
            } => write!(
                f,
                "'{name}' has {}, but the pattern has {found}",
                plural(*expected, "field")
            ),
            Self::VariantStyle { path, style } => match style {
                VariantStyle::Unit => write!(f, "'{path}' has no fields: write '{path}'"),
                VariantStyle::Tuple => {
                    write!(f, "'{path}' has positional fields: write '{path}(...)'")
                }
                VariantStyle::Struct => {
                    write!(f, "'{path}' has named fields: write '{path} {{ ... }}'")
                }
            },
            Self::InvalidBackingType(name) => write!(
                f,
                "Enum backing type must be an integer type, not '{name}'"
            ),
            Self::Misplaced(construct) => f.write_str(match construct {
                Construct::Return => "'return' outside of a function",
                Construct::Yield => "'yield' outside of a generator (a function returning 'iter T')",
                Construct::Break => "'break' outside of a loop, 'do' or 'switch'",
                Construct::Continue => "'continue' outside of a loop",
                Construct::At => "'@' outside of a 'let' initializer",
            }),
            Self::TopLevelStatement => f.write_str(
                "Only declarations (import, fn, struct, enum, let, const) are allowed at the top level",
            ),
            Self::ConstantWithoutValue(name) => write!(f, "Constant '{name}' needs a value"),
            Self::AssignToConstant(name) => write!(f, "Cannot assign to constant '{name}'"),
            Self::InvalidAssignmentTarget => f.write_str("Invalid assignment target"),
            Self::GeneratorReturnValue => f.write_str(
                "Generators finish with a bare 'return'; use 'yield' to produce values",
            ),
            Self::MissingReturnValue {
                function,
                return_type,
            } => write!(
                f,
                "Missing return value: '{function}' returns '{return_type}'"
            ),
            Self::UnexpectedReturnValue(function) => write!(
                f,
                "'{function}' has no return type, so 'return' cannot take a value"
            ),
            Self::FallthroughIntoBindings => f.write_str(
                "The previous case falls through into this one, but this case binds names; end the previous case with 'break' or 'return'",
            ),
            Self::TypeMismatch {
                expected,
                found,
                cast_hint,
            } => {
                write!(f, "Expected '{expected}', found '{found}'")?;
                if *cast_hint {
                    write!(f, "; use 'as {expected}' to convert")?;
                }
                Ok(())
            }
            Self::LiteralOutOfRange { value, expected } => {
                write!(f, "{value} does not fit in '{expected}'")
            }
            Self::InvalidOperands {
                operator,
                left,
                right,
            } => write!(f, "Cannot apply '{operator}' to '{left}' and '{right}'"),
            Self::InvalidOperand { operator, operand } => {
                write!(f, "Cannot apply '{operator}' to '{operand}'")
            }
            Self::InvalidCast { from, to } => write!(f, "Cannot cast '{from}' to '{to}'"),
            Self::NotCallable(found) => write!(f, "Cannot call a value of type '{found}'"),
            Self::NotIndexable(found) => write!(f, "Cannot index into '{found}'"),
            Self::NotIterable(found) => write!(
                f,
                "Cannot loop over '{found}'; 'for' needs an iterator ('iter T')"
            ),
            Self::FunctionAsValue(name) => write!(
                f,
                "Function '{name}' cannot be used as a value; call it instead"
            ),
            Self::CannotInfer(name) => write!(
                f,
                "Cannot infer a type for '{name}'; give it a type or a value"
            ),
            Self::NoValue(name) => write!(f, "The value given to '{name}' has no type"),
            Self::MaybeUnassigned(name) => {
                write!(f, "'{name}' may be used before it has been assigned")
            }
            Self::Captured(name) => write!(
                f,
                "'{name}' belongs to an enclosing function; functions cannot capture variables"
            ),
            Self::AtNeedsType => f.write_str("'@' needs its 'let' to declare a type"),
            Self::MissingReturn {
                function,
                return_type,
            } => write!(
                f,
                "'{function}' must return '{return_type}' on every path"
            ),
            Self::PatternMismatch { path, subject } => write!(
                f,
                "'{path}' cannot match a value of type '{subject}'"
            ),
        }
    }
}
