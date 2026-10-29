//! The text backend: writes a module as LLVM assembly (`.ll`).

use std::fmt::Write;

use super::ir::{
    BinaryOp, Block, CastOp, Declaration, FloatPredicate, Function, Global, Instruction,
    IntPredicate, IrType, Linkage, Module, Terminator, Typed, Value,
};
use super::{Backend, CodegenError};

pub struct TextBackend;

impl Backend for TextBackend {
    type Output = String;

    fn emit(&self, module: &Module) -> Result<String, CodegenError> {
        Ok(module_text(module))
    }
}

/// Writes `@name`/`%name`, quoting names that aren't plain LLVM identifiers.
fn name(sigil: char, name: &str) -> String {
    let plain = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '-'));
    if plain {
        format!("{sigil}{name}")
    } else {
        format!("{sigil}\"{name}\"")
    }
}

fn type_text(ty: &IrType) -> String {
    match ty {
        IrType::Void => "void".to_owned(),
        IrType::Int(bits) => format!("i{bits}"),
        IrType::Float => "float".to_owned(),
        IrType::Double => "double".to_owned(),
        IrType::Ptr => "ptr".to_owned(),
        IrType::Array(size, element) => format!("[{size} x {}]", type_text(element)),
        IrType::Struct(fields) if fields.is_empty() => "{}".to_owned(),
        IrType::Struct(fields) => {
            let fields: Vec<_> = fields.iter().map(type_text).collect();
            format!("{{ {} }}", fields.join(", "))
        }
        IrType::Named(type_name) => name('%', type_name),
        IrType::Token => "token".to_owned(),
    }
}

fn value_text(value: &Value, ty: &IrType) -> String {
    match value {
        Value::Local(local) => name('%', local),
        Value::Global(global) => name('@', global),
        Value::Int(int) => match ty {
            IrType::Int(1) => if *int == 0 { "false" } else { "true" }.to_owned(),
            _ => int.to_string(),
        },
        // LLVM wants floats as the hex bits of the equivalent double
        Value::Float(float) => {
            let float = if *ty == IrType::Float {
                f64::from(*float as f32)
            } else {
                *float
            };
            format!("0x{:016X}", float.to_bits())
        }
        Value::Null => "null".to_owned(),
        Value::Poison => "poison".to_owned(),
        Value::ZeroInitializer => "zeroinitializer".to_owned(),
        Value::Bytes(bytes) => {
            let mut text = String::from("c\"");
            for &byte in bytes {
                if byte.is_ascii_graphic() && byte != b'"' && byte != b'\\' || byte == b' ' {
                    text.push(byte as char);
                } else {
                    let _ = write!(text, "\\{byte:02X}");
                }
            }
            text.push('"');
            text
        }
        Value::Aggregate(elements) => {
            let elements: Vec<_> = elements.iter().map(typed_text).collect();
            match ty {
                IrType::Array(..) => format!("[{}]", elements.join(", ")),
                _ => format!("{{ {} }}", elements.join(", ")),
            }
        }
        Value::NoneToken => "none".to_owned(),
    }
}

fn typed_text(typed: &Typed) -> String {
    format!(
        "{} {}",
        type_text(&typed.ty),
        value_text(&typed.value, &typed.ty)
    )
}

fn linkage_text(linkage: Linkage) -> &'static str {
    match linkage {
        Linkage::External => "",
        Linkage::Internal => "internal ",
        Linkage::Private => "private ",
        Linkage::Appending => "appending ",
    }
}

fn binary_op_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::SDiv => "sdiv",
        BinaryOp::UDiv => "udiv",
        BinaryOp::SRem => "srem",
        BinaryOp::URem => "urem",
        BinaryOp::Shl => "shl",
        BinaryOp::LShr => "lshr",
        BinaryOp::AShr => "ashr",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
        BinaryOp::Xor => "xor",
        BinaryOp::FAdd => "fadd",
        BinaryOp::FSub => "fsub",
        BinaryOp::FMul => "fmul",
        BinaryOp::FDiv => "fdiv",
        BinaryOp::FRem => "frem",
    }
}

fn int_predicate_text(predicate: IntPredicate) -> &'static str {
    match predicate {
        IntPredicate::Eq => "eq",
        IntPredicate::Ne => "ne",
        IntPredicate::Slt => "slt",
        IntPredicate::Sgt => "sgt",
        IntPredicate::Sle => "sle",
        IntPredicate::Sge => "sge",
        IntPredicate::Ult => "ult",
        IntPredicate::Ugt => "ugt",
        IntPredicate::Ule => "ule",
        IntPredicate::Uge => "uge",
    }
}

fn float_predicate_text(predicate: FloatPredicate) -> &'static str {
    match predicate {
        FloatPredicate::Oeq => "oeq",
        FloatPredicate::Une => "une",
        FloatPredicate::Olt => "olt",
        FloatPredicate::Ogt => "ogt",
        FloatPredicate::Ole => "ole",
        FloatPredicate::Oge => "oge",
    }
}

fn cast_op_text(op: CastOp) -> &'static str {
    match op {
        CastOp::Trunc => "trunc",
        CastOp::ZExt => "zext",
        CastOp::SExt => "sext",
        CastOp::FPTrunc => "fptrunc",
        CastOp::FPExt => "fpext",
        CastOp::UIToFP => "uitofp",
        CastOp::SIToFP => "sitofp",
        CastOp::PtrToInt => "ptrtoint",
        CastOp::IntToPtr => "inttoptr",
    }
}

fn instruction_text(instruction: &Instruction) -> String {
    let local = |result: &str| name('%', result);

    match instruction {
        Instruction::Alloca { result, ty } => {
            format!("{} = alloca {}", local(result), type_text(ty))
        }
        Instruction::Load { result, ty, ptr } => format!(
            "{} = load {}, ptr {}",
            local(result),
            type_text(ty),
            value_text(ptr, &IrType::Ptr)
        ),
        Instruction::Store { value, ptr } => format!(
            "store {}, ptr {}",
            typed_text(value),
            value_text(ptr, &IrType::Ptr)
        ),
        Instruction::Binary {
            result,
            op,
            ty,
            lhs,
            rhs,
        } => format!(
            "{} = {} {} {}, {}",
            local(result),
            binary_op_text(*op),
            type_text(ty),
            value_text(lhs, ty),
            value_text(rhs, ty)
        ),
        Instruction::ICmp {
            result,
            predicate,
            ty,
            lhs,
            rhs,
        } => format!(
            "{} = icmp {} {} {}, {}",
            local(result),
            int_predicate_text(*predicate),
            type_text(ty),
            value_text(lhs, ty),
            value_text(rhs, ty)
        ),
        Instruction::FCmp {
            result,
            predicate,
            ty,
            lhs,
            rhs,
        } => format!(
            "{} = fcmp {} {} {}, {}",
            local(result),
            float_predicate_text(*predicate),
            type_text(ty),
            value_text(lhs, ty),
            value_text(rhs, ty)
        ),
        Instruction::Cast {
            result,
            op,
            value,
            to,
        } => format!(
            "{} = {} {} to {}",
            local(result),
            cast_op_text(*op),
            typed_text(value),
            type_text(to)
        ),
        Instruction::GetElementPtr {
            result,
            base,
            ptr,
            indices,
        } => {
            let mut text = format!(
                "{} = getelementptr inbounds {}, ptr {}",
                local(result),
                type_text(base),
                value_text(ptr, &IrType::Ptr)
            );
            for index in indices {
                text.push_str(", ");
                text.push_str(&typed_text(index));
            }
            text
        }
        Instruction::InsertValue {
            result,
            aggregate,
            value,
            index,
        } => format!(
            "{} = insertvalue {}, {}, {index}",
            local(result),
            typed_text(aggregate),
            typed_text(value)
        ),
        Instruction::Call {
            result,
            return_type,
            variadic_params,
            callee,
            args,
        } => {
            let mut text = String::new();
            if let Some(result) = result {
                text.push_str(&local(result));
                text.push_str(" = ");
            }
            text.push_str("call ");
            text.push_str(&type_text(return_type));
            if let Some(params) = variadic_params {
                let mut params: Vec<_> = params.iter().map(type_text).collect();
                params.push("...".to_owned());
                let _ = write!(text, " ({})", params.join(", "));
            }
            let args: Vec<_> = args.iter().map(typed_text).collect();
            let _ = write!(
                text,
                " {}({})",
                value_text(callee, &IrType::Ptr),
                args.join(", ")
            );
            text
        }
        Instruction::Phi {
            result,
            ty,
            incoming,
        } => {
            let incoming: Vec<_> = incoming
                .iter()
                .map(|(value, label)| {
                    format!("[ {}, {} ]", value_text(value, ty), name('%', label))
                })
                .collect();
            format!(
                "{} = phi {} {}",
                local(result),
                type_text(ty),
                incoming.join(", ")
            )
        }
    }
}

fn terminator_text(terminator: &Terminator) -> String {
    match terminator {
        Terminator::Ret(None) => "ret void".to_owned(),
        Terminator::Ret(Some(value)) => format!("ret {}", typed_text(value)),
        Terminator::Br(label) => format!("br label {}", name('%', label)),
        Terminator::CondBr {
            condition,
            then,
            otherwise,
        } => format!(
            "br i1 {}, label {}, label {}",
            value_text(condition, &IrType::BOOL),
            name('%', then),
            name('%', otherwise)
        ),
        Terminator::Switch {
            value,
            default,
            cases,
        } => {
            let mut text = format!(
                "switch {}, label {} [",
                typed_text(value),
                name('%', default)
            );
            for (case, label) in cases {
                let _ = write!(
                    text,
                    "\n    {}, label {}",
                    typed_text(case),
                    name('%', label)
                );
            }
            text.push_str("\n  ]");
            text
        }
        Terminator::Unreachable => "unreachable".to_owned(),
    }
}

fn block_text(block: &Block, text: &mut String) {
    let _ = writeln!(text, "{}:", name('%', &block.label).trim_start_matches('%'));
    for instruction in &block.instructions {
        let _ = writeln!(text, "  {}", instruction_text(instruction));
    }
    let terminator = block
        .terminator
        .as_ref()
        .unwrap_or(&Terminator::Unreachable);
    let _ = writeln!(text, "  {}", terminator_text(terminator));
}

fn function_text(function: &Function, text: &mut String) {
    let params: Vec<_> = function
        .params
        .iter()
        .map(|(ty, param)| format!("{} {}", type_text(ty), name('%', param)))
        .collect();
    let attributes = if function.attributes.is_empty() {
        String::new()
    } else {
        format!(" {}", function.attributes.join(" "))
    };
    let _ = writeln!(
        text,
        "define {}{} {}({}){attributes} {{",
        linkage_text(function.linkage),
        type_text(&function.return_type),
        name('@', &function.name),
        params.join(", ")
    );
    for (index, block) in function.blocks.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        block_text(block, text);
    }
    text.push_str("}\n");
}

fn declaration_text(declaration: &Declaration) -> String {
    let mut params: Vec<_> = declaration.params.iter().map(type_text).collect();
    if declaration.variadic {
        params.push("...".to_owned());
    }
    format!(
        "declare {} {}({})",
        type_text(&declaration.return_type),
        name('@', &declaration.name),
        params.join(", ")
    )
}

fn global_text(global: &Global) -> String {
    format!(
        "{} = {}{}{} {}",
        name('@', &global.name),
        linkage_text(global.linkage),
        if global.unnamed_addr {
            "unnamed_addr "
        } else {
            ""
        },
        if global.constant {
            "constant"
        } else {
            "global"
        },
        typed_text(&global.init)
    )
}

pub fn module_text(module: &Module) -> String {
    let mut text = format!("; ModuleID = '{}'\n", module.name);

    if !module.types.is_empty() {
        text.push('\n');
        for definition in &module.types {
            let fields: Vec<_> = definition.fields.iter().map(type_text).collect();
            let _ = writeln!(
                text,
                "{} = type {{ {} }}",
                name('%', &definition.name),
                fields.join(", ")
            );
        }
    }

    if !module.globals.is_empty() || !module.external_globals.is_empty() {
        text.push('\n');
        for global in &module.globals {
            let _ = writeln!(text, "{}", global_text(global));
        }
        for global in &module.external_globals {
            let _ = writeln!(
                text,
                "{} = external global {}",
                name('@', &global.name),
                type_text(&global.ty)
            );
        }
    }

    for function in &module.functions {
        text.push('\n');
        function_text(function, &mut text);
    }

    if !module.declarations.is_empty() {
        text.push('\n');
        for declaration in &module.declarations {
            let _ = writeln!(text, "{}", declaration_text(declaration));
        }
    }

    text
}
