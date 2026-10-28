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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltinMember {
    /// A function whose signature isn't modelled yet, so calls to it are not
    /// checked and return an unknown type.
    Function,
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
            return Some(BuiltinMember::Function);
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
