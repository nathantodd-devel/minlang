use num_bigint::BigInt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntType {
    pub signed: bool,
    pub bits: u8,
    /// `usize`: unsigned and pointer sized, assumed to hold at least 32 and at
    /// most 64 bits.
    pub pointer_sized: bool,
}

impl IntType {
    pub const S32: Self = Self::new(true, 32);
    pub const USIZE: Self = Self {
        signed: false,
        bits: 64,
        pointer_sized: true,
    };

    pub const fn new(signed: bool, bits: u8) -> Self {
        Self {
            signed,
            bits,
            pointer_sized: false,
        }
    }

    pub fn name(self) -> String {
        if self.pointer_sized {
            return "usize".to_owned();
        }
        format!("{}{}", if self.signed { 's' } else { 'u' }, self.bits)
    }

    pub fn contains(self, value: &BigInt) -> bool {
        // usize is only guaranteed 32 bits
        let bits = if self.pointer_sized { 32 } else { self.bits };
        let (min, max) = if self.signed {
            let half = BigInt::from(1) << (bits - 1);
            (-half.clone(), half - 1)
        } else {
            (BigInt::from(0), (BigInt::from(1) << bits) - 1)
        };
        min <= *value && *value <= max
    }

    /// Whether every value of `self` is also a value of `to`, so it can be
    /// converted implicitly.
    pub fn widens_to(self, to: Self) -> bool {
        if self == to {
            return true;
        }
        if to.pointer_sized {
            return !self.signed && !self.pointer_sized && self.bits <= 32;
        }
        if self.pointer_sized {
            return to == Self::new(false, 64);
        }
        match (self.signed, to.signed) {
            (false, false) | (true, true) => to.bits >= self.bits,
            (false, true) => to.bits > self.bits,
            (true, false) => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int(IntType),
    /// 32 or 64 bits
    Float(u8),
    Bool,
    /// An integer literal without a suffix (or arithmetic on them), whose type
    /// comes from where it's used. `value` is known for literals, so they can
    /// be checked against the range of that type.
    UntypedInt(Option<BigInt>),
    /// A float literal without a suffix.
    UntypedFloat,
    /// Index into the checker's struct table.
    Struct(usize),
    /// Index into the checker's enum table.
    Enum(usize),
    Pointer(Box<Type>),
    Reference(Box<Type>),
    Iterator(Box<Type>),
    Array {
        element: Box<Type>,
        size: Option<u64>,
    },
    /// The "value" of something that produces none, like a call to a function
    /// without a return type.
    Void,
    /// The type of `return`, `break` and other expressions that never finish.
    Never,
    /// The type of something whose real type isn't known: an expression that
    /// already had an error reported, or a call to a built-in function. It is
    /// compatible with everything, so it causes no further errors.
    Unknown,
}

impl Type {
    /// C's `void *`, which libc functions take and return. Any pointer,
    /// reference or array converts to it, and it converts to any pointer or
    /// `T[?]`. It can't be written in source.
    pub fn void_pointer() -> Self {
        Self::Pointer(Box::new(Self::Void))
    }

    pub fn is_void_pointer(&self) -> bool {
        matches!(self, Self::Pointer(pointee) if **pointee == Self::Void)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::Int(_) | Self::Float(_) | Self::UntypedInt(_) | Self::UntypedFloat
        )
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Self::Int(_) | Self::UntypedInt(_))
    }

    pub fn is_untyped(&self) -> bool {
        matches!(self, Self::UntypedInt(_) | Self::UntypedFloat)
    }

    /// The type an untyped literal gets when nothing says otherwise, matching
    /// the lexer's defaults.
    pub fn concrete(self) -> Self {
        match self {
            Self::UntypedInt(_) => Self::Int(IntType::S32),
            Self::UntypedFloat => Self::Float(64),
            other => other,
        }
    }

    /// The innermost element type of a (possibly nested) array.
    pub fn base_element(&self) -> &Self {
        match self {
            Self::Array { element, .. } => element.base_element(),
            other => other,
        }
    }
}

/// Whether arrays of `from` can be used where arrays of `to` are expected.
/// `T[?]` is a pointer to the first element, so any size can be passed where
/// the outermost size is unknown, but the elements must match exactly:
/// `u8[4][8]` can be a `u8[?][8]`, but not a `u8[?][?]`, which is a pointer
/// to pointers.
pub fn array_compatible(from: &Type, to: &Type) -> bool {
    match (from, to) {
        (
            Type::Array {
                element: from_element,
                size: from_size,
            },
            Type::Array {
                element: to_element,
                size: to_size,
            },
        ) => (to_size.is_none() || from_size == to_size) && from_element == to_element,
        _ => from == to,
    }
}

/// The type two numbers are converted to when combined, if there is one. An
/// untyped literal takes the other side's type (whether its value fits is
/// checked separately); otherwise the narrower side must widen to the other.
pub fn common_numeric(left: &Type, right: &Type) -> Option<Type> {
    match (left, right) {
        (Type::UntypedInt(_), Type::UntypedInt(_)) => Some(Type::UntypedInt(None)),
        (a, b) if a.is_untyped() && b.is_untyped() => Some(Type::UntypedFloat),
        (Type::UntypedInt(_), Type::Int(_))
        | (Type::UntypedInt(_) | Type::UntypedFloat, Type::Float(_)) => Some(right.clone()),
        (Type::Int(_), Type::UntypedInt(_))
        | (Type::Float(_), Type::UntypedInt(_) | Type::UntypedFloat) => Some(left.clone()),
        (Type::Int(a), Type::Int(b)) => {
            if a.widens_to(*b) {
                Some(right.clone())
            } else if b.widens_to(*a) {
                Some(left.clone())
            } else {
                None
            }
        }
        (Type::Float(a), Type::Float(b)) => Some(Type::Float(*a.max(b))),
        _ => None,
    }
}
