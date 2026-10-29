//! Symbol names, in an uncompressed subset of the Itanium C++ ABI so that
//! `c++filt`, gdb and perf can demangle them:
//!
//! - functions: `_ZN<segments>E<parameter types>`, e.g.
//!   `_ZN9iterators9bytes_getERN9iterators5BytesEm` is
//!   `iterators::bytes_get(iterators::Bytes&, unsigned long)`
//! - globals: `_ZN<segments>E`
//!
//! The segments are the module, the enclosing functions, and the name. A
//! segment like `$2` tells apart same-named items in sibling blocks; `$` can't
//! appear in identifiers, so it can't collide with a user's name.

use crate::checker::{CheckedProgram, IntType, Type};

/// `<length><text>`, an Itanium source name.
fn source_name(segment: &str) -> String {
    format!("{}{segment}", segment.len())
}

/// The module name used as the first segment: the file stem, with anything
/// that can't appear in an identifier replaced by `_`.
pub fn module_name(file: Option<&str>) -> String {
    let stem = file
        .and_then(|file| std::path::Path::new(file).file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or("main");
    let name: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.is_empty() {
        "main".to_owned()
    } else {
        name
    }
}

/// `N<segments>E` for an item at `path` in `module`.
fn nested_name(module: &str, path: &[String], name: &str) -> String {
    let mut text = String::from("N");
    text.push_str(&source_name(module));
    for segment in path {
        text.push_str(&source_name(segment));
    }
    text.push_str(&source_name(name));
    text.push('E');
    text
}

fn int_code(int: IntType) -> &'static str {
    if int.pointer_sized {
        return "m";
    }
    match (int.signed, int.bits) {
        (false, 8) => "h",
        (true, 8) => "a",
        (false, 16) => "t",
        (true, 16) => "s",
        (false, 32) => "j",
        (true, 32) => "i",
        (false, _) => "y",
        (true, _) => "x",
    }
}

/// The Itanium encoding of a parameter type. Structs and enums declared in
/// this file are named in `module`; imported ones in the module they're from.
pub fn type_code(ty: &Type, module: &str, program: &CheckedProgram) -> String {
    match ty {
        Type::Int(int) => int_code(*int).to_owned(),
        Type::UntypedInt(_) => "i".to_owned(),
        Type::Float(32) => "f".to_owned(),
        Type::Float(_) | Type::UntypedFloat => "d".to_owned(),
        Type::Bool => "b".to_owned(),
        Type::Pointer(inner) => format!("P{}", type_code(inner, module, program)),
        Type::Reference(inner) => format!("R{}", type_code(inner, module, program)),
        Type::Array {
            element,
            size: None,
        } => format!("P{}", type_code(element, module, program)),
        Type::Array {
            element,
            size: Some(size),
        } => format!("A{size}_{}", type_code(element, module, program)),
        Type::Struct(id) => {
            let definition = &program.structs[*id];
            let module = definition.module.as_deref().unwrap_or(module);
            nested_name(module, &definition.path, &definition.name)
        }
        Type::Enum(id) => {
            let definition = &program.enums[*id];
            let module = definition.module.as_deref().unwrap_or(module);
            nested_name(module, &definition.path, &definition.name)
        }
        Type::Iterator(item) => format!("4iterI{}E", type_code(item, module, program)),
        Type::Void | Type::Never | Type::Unknown => "v".to_owned(),
    }
}

pub fn function(
    module: &str,
    path: &[String],
    name: &str,
    params: &[Type],
    program: &CheckedProgram,
) -> String {
    let mut text = format!("_Z{}", nested_name(module, path, name));
    if params.is_empty() {
        text.push('v');
    }
    for param in params {
        text.push_str(&type_code(param, module, program));
    }
    text
}

pub fn global(module: &str, name: &str) -> String {
    format!("_Z{}", nested_name(module, &[], name))
}

/// The function run before `main` to set globals whose values aren't
/// constants.
pub fn initializer(module: &str) -> String {
    format!("_Z{}v", nested_name(module, &[], "$init"))
}

/// The readable name of a struct or enum's LLVM type, like `geo::Point`.
/// Types aren't linker symbols, so they needn't be mangled.
pub fn type_name(module: &str, path: &[String], name: &str) -> String {
    let mut segments = vec![module];
    segments.extend(path.iter().map(String::as_str));
    segments.push(name);
    segments.join("::")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::StructDef;

    fn code(ty: Type) -> String {
        let mut program = CheckedProgram::default();
        let start = crate::parser::Position { line: 0, col: 0 };
        program.structs.push(StructDef {
            name: "Point".to_owned(),
            path: vec!["outer".to_owned()],
            module: None,
            position: crate::parser::Location { start, end: start },
            fields: Vec::new(),
        });
        type_code(&ty, "geo", &program)
    }

    fn int(signed: bool, bits: u8) -> Type {
        Type::Int(IntType::new(signed, bits))
    }

    #[test]
    fn scalar_codes() {
        let codes: Vec<_> = [
            int(false, 8),
            int(true, 8),
            int(false, 16),
            int(true, 16),
            int(false, 32),
            int(true, 32),
            int(false, 64),
            int(true, 64),
            Type::Int(IntType::USIZE),
            Type::Bool,
            Type::Float(32),
            Type::Float(64),
        ]
        .into_iter()
        .map(code)
        .collect();
        assert_eq!(
            codes,
            ["h", "a", "t", "s", "j", "i", "y", "x", "m", "b", "f", "d"]
        );
    }

    #[test]
    fn compound_codes() {
        let byte = || Box::new(int(false, 8));
        assert_eq!(code(Type::Pointer(byte())), "Ph");
        assert_eq!(code(Type::Reference(byte())), "Rh");
        assert_eq!(
            code(Type::Array {
                element: byte(),
                size: None
            }),
            "Ph"
        );
        assert_eq!(
            code(Type::Array {
                element: byte(),
                size: Some(4)
            }),
            "A4_h"
        );
        assert_eq!(code(Type::Iterator(byte())), "4iterIhE");
        assert_eq!(code(Type::Struct(0)), "N3geo5outer5PointE");
    }

    #[test]
    fn item_names() {
        let program = CheckedProgram::default();
        assert_eq!(
            function("geo", &[], "origin", &[], &program),
            "_ZN3geo6originEv"
        );
        assert_eq!(
            function(
                "geo",
                &["f".to_owned(), "$2".to_owned()],
                "g",
                &[Type::Bool],
                &program
            ),
            "_ZN3geo1f2$21gEb"
        );
        assert_eq!(global("geo", "counter"), "_ZN3geo7counterE");
        assert_eq!(initializer("geo"), "_ZN3geo5$initEv");
        assert_eq!(
            type_name("geo", &["f".to_owned()], "Point"),
            "geo::f::Point"
        );
        assert_eq!(module_name(Some("dir/my-prog.pk")), "my_prog");
        assert_eq!(module_name(None), "main");
    }
}
