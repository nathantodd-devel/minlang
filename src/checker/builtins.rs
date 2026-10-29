//! Names the language provides without any declaration, including the
//! built-in module `types`. Other modules are files, including the standard
//! library's; see `crate::modules`.

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

/// A built-in module. `types` is the only one: the standard library is
/// written in minlang, in the `std` directory shipped with the compiler.
pub struct Module {
    pub name: &'static str,
}

impl Module {
    pub fn member(&self, name: &str) -> Option<Type> {
        primitive(name)
    }

    pub fn members(&self) -> Vec<&'static str> {
        PRIMITIVES.to_vec()
    }
}

const MODULES: &[Module] = &[Module { name: "types" }];

pub fn module(name: &str) -> Option<&'static Module> {
    MODULES.iter().find(|module| module.name == name)
}
