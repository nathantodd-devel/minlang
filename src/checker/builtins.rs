//! Names the language provides without any declaration. There are no module
//! files yet, so the modules a program can import are listed here.

use super::types::{IntType, Type};

pub const PRIMITIVES: &[&str] = &[
    "u8", "u16", "u32", "u64", "usize", "s8", "s16", "s32", "s64", "f32", "f64", "bool",
];

pub fn primitive(name: &str) -> Option<Type> {
    Some(match name {
        "u8" => Type::Int(IntType::new(false, 8)),
        "u16" => Type::Int(IntType::new(false, 16)),
        "u32" => Type::Int(IntType::new(false, 32)),
        "u64" => Type::Int(IntType::new(false, 64)),
        "usize" => Type::Int(IntType::USIZE),
        "s8" => Type::Int(IntType::new(true, 8)),
        "s16" => Type::Int(IntType::new(true, 16)),
        "s32" => Type::Int(IntType::new(true, 32)),
        "s64" => Type::Int(IntType::new(true, 64)),
        "f32" => Type::Float(32),
        "f64" => Type::Float(64),
        "bool" => Type::Bool,
        _ => return None,
    })
}

/// A C library function. Its symbol is its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinFunction {
    pub name: &'static str,
    pub params: Vec<Type>,
    /// Whether it takes further arguments after `params`, like `printf`.
    pub variadic: bool,
    pub return_type: Option<Type>,
}

fn builtin_function(name: &str) -> Option<BuiltinFunction> {
    let u8_pointer = || Type::Array {
        element: Box::new(Type::Int(IntType::new(false, 8))),
        size: None,
    };

    Some(match name {
        "printf" => BuiltinFunction {
            name: "printf",
            params: vec![u8_pointer()],
            variadic: true,
            return_type: Some(Type::Int(IntType::S32)),
        },
        "strcat" => BuiltinFunction {
            name: "strcat",
            params: vec![u8_pointer(), u8_pointer()],
            variadic: false,
            return_type: Some(u8_pointer()),
        },
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinMember {
    Function(BuiltinFunction),
    Type(Type),
}

pub struct Module {
    pub name: &'static str,
    functions: &'static [&'static str],
    /// Whether the module exports the built-in types, like `types`.
    exports_primitives: bool,
}

impl Module {
    pub fn member(&self, name: &str) -> Option<BuiltinMember> {
        if self.functions.contains(&name) {
            return builtin_function(name).map(BuiltinMember::Function);
        }
        if self.exports_primitives {
            return primitive(name).map(BuiltinMember::Type);
        }
        None
    }

    pub fn members(&self) -> Vec<&'static str> {
        let mut members = self.functions.to_vec();
        if self.exports_primitives {
            members.extend(PRIMITIVES);
        }
        members
    }
}

const MODULES: &[Module] = &[
    Module {
        name: "std",
        functions: &["printf", "strcat"],
        exports_primitives: false,
    },
    Module {
        name: "types",
        functions: &[],
        exports_primitives: true,
    },
];

pub fn module(name: &str) -> Option<&'static Module> {
    MODULES.iter().find(|module| module.name == name)
}
