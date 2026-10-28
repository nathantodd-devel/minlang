//! Lowering from the checked AST to the backend-neutral IR.

mod control;
mod expr;
mod generator;
mod pattern;

use std::collections::{HashMap, HashSet};

use num_bigint::BigInt;

use super::CodegenError;
use super::ir::{
    Block, Declaration, Function, Global, Instruction, IrType, Linkage, Module, Terminator,
    TypeDefinition, Typed, Value,
};
use super::layout::{self, NamedTypes};
use super::mangle;
use crate::checker::{CheckedProgram, IntType, Type, VariantShape};
use crate::parser::{Ast, Expression, ExpressionKind, TypeInfo};

/// How an enum is represented.
#[derive(Debug, Clone)]
enum EnumLayout {
    /// No variant carries data, so the enum is just its tag.
    Plain(IrType),
    /// `{ tag, payload }`, with each variant's fields stored in the payload as
    /// a struct of their types (`None` for variants without fields).
    Tagged {
        name: String,
        tag: IrType,
        payloads: Vec<Option<IrType>>,
    },
}

/// Where `break` and `continue` go inside a loop, `do` or `switch`.
struct LoopTarget {
    break_label: String,
    /// `None` for constructs `continue` can't target.
    continue_label: Option<String>,
}

/// The coroutine state of the generator being lowered.
struct GeneratorState {
    /// Where yielded values are stored for the consumer.
    promise: Value,
    item: Type,
    /// The `coro.id` token.
    id: Value,
    /// The coroutine handle, which is the `iter T` value.
    handle: Value,
    /// The block that runs the final suspend.
    finish_label: String,
    cleanup_label: String,
    suspend_label: String,
}

/// The function being built.
struct FunctionState {
    blocks: Vec<Block>,
    current: usize,
    allocas: Vec<Instruction>,
    next_temp: usize,
    next_label: usize,
    /// The allocas of local variables.
    variables: HashMap<usize, Value>,
    loops: Vec<LoopTarget>,
    return_type: Option<Type>,
    is_main: bool,
    generator: Option<GeneratorState>,
}

impl FunctionState {
    fn new(return_type: Option<Type>, is_main: bool) -> Self {
        Self {
            blocks: vec![Block {
                label: "entry".to_owned(),
                instructions: Vec::new(),
                terminator: None,
            }],
            current: 0,
            allocas: Vec::new(),
            next_temp: 0,
            next_label: 0,
            variables: HashMap::new(),
            loops: Vec::new(),
            return_type,
            is_main,
            generator: None,
        }
    }
}

pub(super) struct Lowerer<'a> {
    program: &'a CheckedProgram,
    module_name: String,
    module: Module,
    named: NamedTypes,
    struct_types: Vec<String>,
    enum_layouts: Vec<EnumLayout>,
    /// Discriminant of each variant of each enum.
    discriminants: Vec<Vec<i128>>,
    function_names: Vec<String>,
    /// Function ids by the id of their declaring expression.
    function_ids: HashMap<usize, usize>,
    /// Functions found while lowering, still to be lowered.
    pending: Vec<&'a Expression>,
    strings: HashMap<Vec<u8>, String>,
    declared: HashSet<String>,
    /// Globals whose values are set at startup: (variable, value, type).
    deferred_globals: Vec<(usize, Option<&'a Expression>, Option<&'a TypeInfo>)>,
    function: FunctionState,
}

/// Lowers a checked program to an IR module. `file` names the module.
pub fn lower(
    ast: &Ast,
    program: &CheckedProgram,
    file: Option<&str>,
) -> Result<Module, CodegenError> {
    let module_name = mangle::module_name(file);
    let mut lowerer = Lowerer::new(program, module_name, ast);
    lowerer.lower_module(ast);
    Ok(lowerer.module)
}

impl<'a> Lowerer<'a> {
    fn new(program: &'a CheckedProgram, module_name: String, ast: &Ast) -> Self {
        let mut lowerer = Self {
            program,
            module: Module {
                name: module_name.clone(),
                ..Module::default()
            },
            module_name,
            named: NamedTypes::new(),
            struct_types: Vec::new(),
            enum_layouts: Vec::new(),
            discriminants: Vec::new(),
            function_names: Vec::new(),
            function_ids: HashMap::new(),
            pending: Vec::new(),
            strings: HashMap::new(),
            declared: HashSet::new(),
            deferred_globals: Vec::new(),
            function: FunctionState::new(None, false),
        };
        lowerer.define_types();
        lowerer.discriminants = program
            .enums
            .iter()
            .zip(&lowerer.enum_layouts)
            .map(|(definition, layout)| {
                let bits = match layout {
                    EnumLayout::Plain(IrType::Int(bits))
                    | EnumLayout::Tagged {
                        tag: IrType::Int(bits),
                        ..
                    } => *bits,
                    _ => 32,
                };
                definition
                    .discriminants
                    .iter()
                    .map(|value| wrap_int(value, bits))
                    .collect()
            })
            .collect();
        lowerer.name_functions(ast);
        lowerer
    }

    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    /// Defines the named LLVM types of every struct and enum with data.
    fn define_types(&mut self) {
        let program = self.program;

        for definition in &program.structs {
            self.struct_types.push(mangle::type_name(
                &self.module_name,
                &definition.path,
                &definition.name,
            ));
        }

        for definition in &program.enums {
            let tag = match &definition.backing_type {
                Some(Type::Int(int)) => IrType::Int(u32::from(int_bits(*int))),
                _ => IrType::I32,
            };
            if !definition.has_data() {
                self.enum_layouts.push(EnumLayout::Plain(tag));
                continue;
            }
            let name = mangle::type_name(&self.module_name, &definition.path, &definition.name);
            self.enum_layouts.push(EnumLayout::Tagged {
                name,
                tag,
                payloads: Vec::new(),
            });
        }

        // Field types may name other structs, so all names exist first
        for (id, definition) in program.structs.iter().enumerate() {
            let fields: Vec<_> = definition
                .fields
                .iter()
                .map(|(_, ty)| self.ir_type(ty))
                .collect();
            let name = self.struct_types[id].clone();
            self.named.insert(name.clone(), fields.clone());
            self.module.types.push(TypeDefinition { name, fields });
        }

        for (id, definition) in program.enums.iter().enumerate() {
            let payloads: Vec<Option<IrType>> = definition
                .variants
                .iter()
                .map(|(_, shape)| match shape {
                    VariantShape::Unit => None,
                    VariantShape::Tuple(types) => Some(IrType::Struct(
                        types.iter().map(|ty| self.ir_type(ty)).collect(),
                    )),
                    VariantShape::Struct(fields) => Some(IrType::Struct(
                        fields.iter().map(|(_, ty)| self.ir_type(ty)).collect(),
                    )),
                })
                .collect();

            if let EnumLayout::Tagged {
                name,
                tag,
                payloads: slot,
            } = &mut self.enum_layouts[id]
            {
                let variants: Vec<_> = payloads.iter().flatten().cloned().collect();
                let fields = vec![tag.clone(), layout::payload_type(&variants, &self.named)];
                self.named.insert(name.clone(), fields.clone());
                self.module.types.push(TypeDefinition {
                    name: name.clone(),
                    fields,
                });
                *slot = payloads;
            }
        }
    }

    fn ir_type(&self, ty: &Type) -> IrType {
        match ty {
            Type::Int(int) => IrType::Int(u32::from(int_bits(*int))),
            Type::UntypedInt(_) => IrType::I32,
            Type::Float(32) => IrType::Float,
            Type::Float(_) | Type::UntypedFloat => IrType::Double,
            Type::Bool => IrType::BOOL,
            Type::Struct(id) => IrType::Named(self.struct_types[*id].clone()),
            Type::Enum(id) => match &self.enum_layouts[*id] {
                EnumLayout::Plain(tag) => tag.clone(),
                EnumLayout::Tagged { name, .. } => IrType::Named(name.clone()),
            },
            Type::Pointer(_) | Type::Reference(_) | Type::Iterator(_) => IrType::Ptr,
            Type::Array {
                element,
                size: Some(size),
            } => IrType::Array(*size, Box::new(self.ir_type(element))),
            Type::Array { size: None, .. } => IrType::Ptr,
            Type::Void | Type::Never | Type::Unknown => IrType::Void,
        }
    }

    // -------------------------------------------------------------------------
    // Module
    // -------------------------------------------------------------------------

    fn name_functions(&mut self, ast: &Ast) {
        let program = self.program;
        for (id, function) in program.functions.iter().enumerate() {
            self.function_ids.insert(function.declaration, id);
            let is_main = function.name == "main" && function.path.is_empty();
            let top_level = ast.iter().any(|item| item.id == function.declaration);
            let name = if is_main && top_level {
                "main".to_owned()
            } else {
                mangle::function(
                    &self.module_name,
                    &function.path,
                    &function.name,
                    &function.params,
                    program,
                )
            };
            self.function_names.push(name);
        }
    }

    fn lower_module(&mut self, ast: &'a Ast) {
        for item in ast {
            match &item.kind {
                ExpressionKind::FunctionDeclarationExpression { .. } => self.pending.push(item),
                ExpressionKind::LetExpression {
                    typeinfo, value, ..
                } => self.lower_global(item, typeinfo.as_ref(), value.as_deref()),
                _ => {}
            }
        }

        // In the order found: top-level functions in source order, then the
        // functions nested in each
        let mut next = 0;
        while let Some(&function) = self.pending.get(next) {
            next += 1;
            self.lower_function(function);
        }

        self.lower_global_initializer();
    }

    fn declare(&mut self, name: &str, return_type: IrType, params: Vec<IrType>, variadic: bool) {
        if self.declared.insert(name.to_owned()) {
            self.module.declarations.push(Declaration {
                name: name.to_owned(),
                return_type,
                params,
                variadic,
            });
        }
    }

    /// The global holding a string literal's bytes and terminating zero.
    fn string_global(&mut self, value: &str) -> Value {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);

        if let Some(name) = self.strings.get(&bytes) {
            return Value::Global(name.clone());
        }

        let name = format!(".str.{}", self.strings.len());
        self.module.globals.push(Global {
            name: name.clone(),
            linkage: Linkage::Private,
            constant: true,
            unnamed_addr: true,
            init: Typed::new(
                IrType::Array(bytes.len() as u64, Box::new(IrType::I8)),
                Value::Bytes(bytes.clone()),
            ),
        });
        self.strings.insert(bytes, name.clone());
        Value::Global(name)
    }

    fn global_name(&self, variable: usize) -> String {
        mangle::global(&self.module_name, &self.program.variables[variable].name)
    }

    fn lower_global(
        &mut self,
        item: &'a Expression,
        typeinfo: Option<&'a TypeInfo>,
        value: Option<&'a Expression>,
    ) {
        let Some(&variable) = self.program.declarations.get(&item.id) else {
            return;
        };
        let ty = self.program.variables[variable].ty.clone();
        let ir_type = self.ir_type(&ty);

        let fill = array_initializer(typeinfo);
        let constant = match (value, fill) {
            (Some(value), _) => self.constant(value, &ty),
            (None, Some(fill)) => self.constant_fill(fill, &ty),
            (None, None) => Some(Value::ZeroInitializer),
        };

        let init = match constant {
            Some(constant) => constant,
            None => {
                self.deferred_globals.push((variable, value, typeinfo));
                Value::ZeroInitializer
            }
        };

        let name = self.global_name(variable);
        self.module.globals.push(Global {
            name,
            linkage: Linkage::Internal,
            constant: false,
            unnamed_addr: false,
            init: Typed::new(ir_type, init),
        });
    }

    /// Sets globals whose values aren't constants, in a function LLVM runs
    /// before `main`.
    fn lower_global_initializer(&mut self) {
        if self.deferred_globals.is_empty() {
            return;
        }

        self.function = FunctionState::new(None, false);
        for (variable, value, typeinfo) in std::mem::take(&mut self.deferred_globals) {
            let ptr = Value::Global(self.global_name(variable));
            let ty = self.program.variables[variable].ty.clone();
            self.initialize(&ptr, &ty, value, array_initializer(typeinfo));
        }
        self.terminate(Terminator::Ret(None));

        let name = mangle::initializer(&self.module_name);
        let function = self.finish_function(name.clone(), IrType::Void, Vec::new(), Vec::new());
        self.module.functions.push(function);

        let entry = IrType::Struct(vec![IrType::I32, IrType::Ptr, IrType::Ptr]);
        self.module.globals.push(Global {
            name: "llvm.global_ctors".to_owned(),
            linkage: Linkage::Appending,
            constant: false,
            unnamed_addr: false,
            init: Typed::new(
                IrType::Array(1, Box::new(entry.clone())),
                Value::Aggregate(vec![Typed::new(
                    entry,
                    Value::Aggregate(vec![
                        Typed::int(IrType::I32, 65535),
                        Typed::ptr(Value::Global(name)),
                        Typed::ptr(Value::Null),
                    ]),
                )]),
            ),
        });
    }

    fn lower_function(&mut self, declaration: &'a Expression) {
        let ExpressionKind::FunctionDeclarationExpression { body, .. } = &declaration.kind else {
            return;
        };
        let Some(&id) = self.function_ids.get(&declaration.id) else {
            return;
        };
        let program = self.program;
        let function = &program.functions[id];
        let name = self.function_names[id].clone();
        let is_main = name == "main";

        self.function = FunctionState::new(function.return_type.clone(), is_main);

        let mut params = Vec::new();
        for (index, (ty, &variable)) in function
            .params
            .iter()
            .zip(&function.param_variables)
            .enumerate()
        {
            let ir_type = self.ir_type(ty);
            let param = format!("arg{index}");
            let slot = self.alloca(ir_type.clone());
            self.store(
                Typed::new(ir_type.clone(), Value::Local(param.clone())),
                &slot,
            );
            self.function.variables.insert(variable, slot);
            params.push((ir_type, param));
        }

        let mut attributes = Vec::new();
        let return_type = if let Some(item) = function.yield_type() {
            attributes.push("presplitcoroutine".to_owned());
            self.begin_generator(item.clone());
            IrType::Ptr
        } else if is_main {
            IrType::I32
        } else {
            function
                .return_type
                .as_ref()
                .map_or(IrType::Void, |ty| self.ir_type(ty))
        };

        let result = self.block_as(body, function.return_type.as_ref());

        if !self.is_terminated() {
            if self.function.generator.is_some() {
                self.finish_generator();
            } else {
                let ret = match (&function.return_type, result) {
                    (Some(_), Some(value)) if return_type != IrType::Void => Some(value),
                    _ if is_main => Some(Typed::int(IrType::I32, 0)),
                    _ => None,
                };
                self.terminate(Terminator::Ret(ret));
            }
        }

        if self.function.generator.is_some() {
            self.end_generator();
        }

        let linkage = if is_main {
            Linkage::External
        } else {
            Linkage::Internal
        };
        let mut function = self.finish_function(name, return_type, params, attributes);
        function.linkage = linkage;
        self.module.functions.push(function);
    }

    /// Takes the built function, gathering the allocas into its entry block.
    fn finish_function(
        &mut self,
        name: String,
        return_type: IrType,
        params: Vec<(IrType, String)>,
        attributes: Vec<String>,
    ) -> Function {
        let mut state = std::mem::replace(&mut self.function, FunctionState::new(None, false));
        let mut entry = std::mem::take(&mut state.allocas);
        entry.append(&mut state.blocks[0].instructions);
        state.blocks[0].instructions = entry;

        Function {
            name,
            linkage: Linkage::Internal,
            return_type,
            params,
            attributes,
            blocks: state.blocks,
        }
    }

    // -------------------------------------------------------------------------
    // Building instructions
    // -------------------------------------------------------------------------

    fn temp(&mut self) -> String {
        let name = format!("t{}", self.function.next_temp);
        self.function.next_temp += 1;
        name
    }

    fn label(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}{}", self.function.next_label);
        self.function.next_label += 1;
        name
    }

    fn current_label(&self) -> String {
        self.function.blocks[self.function.current].label.clone()
    }

    fn is_terminated(&self) -> bool {
        self.function.blocks[self.function.current]
            .terminator
            .is_some()
    }

    /// Starts block `label`, falling through into it from the current block
    /// if that hasn't ended.
    fn start_block(&mut self, label: String) {
        if !self.is_terminated() {
            self.terminate(Terminator::Br(label.clone()));
        }
        self.function.blocks.push(Block {
            label,
            instructions: Vec::new(),
            terminator: None,
        });
        self.function.current = self.function.blocks.len() - 1;
    }

    /// Code after a `return` or `break` can't run; it goes into a block
    /// nothing jumps to.
    fn ensure_open(&mut self) {
        if self.is_terminated() {
            let label = self.label("dead");
            self.start_block(label);
        }
    }

    fn push(&mut self, instruction: Instruction) {
        self.ensure_open();
        self.function.blocks[self.function.current]
            .instructions
            .push(instruction);
    }

    fn terminate(&mut self, terminator: Terminator) {
        self.ensure_open();
        self.function.blocks[self.function.current].terminator = Some(terminator);
    }

    fn branch(&mut self, label: &str) {
        self.terminate(Terminator::Br(label.to_owned()));
    }

    fn branch_if(&mut self, condition: Value, then: &str, otherwise: &str) {
        self.terminate(Terminator::CondBr {
            condition,
            then: then.to_owned(),
            otherwise: otherwise.to_owned(),
        });
    }

    fn alloca(&mut self, ty: IrType) -> Value {
        let result = self.temp();
        self.function.allocas.push(Instruction::Alloca {
            result: result.clone(),
            ty,
        });
        Value::Local(result)
    }

    fn load(&mut self, ty: IrType, ptr: &Value) -> Typed {
        let result = self.temp();
        self.push(Instruction::Load {
            result: result.clone(),
            ty: ty.clone(),
            ptr: ptr.clone(),
        });
        Typed::new(ty, Value::Local(result))
    }

    fn store(&mut self, value: Typed, ptr: &Value) {
        self.push(Instruction::Store {
            value,
            ptr: ptr.clone(),
        });
    }

    fn gep(&mut self, base: IrType, ptr: &Value, indices: Vec<Typed>) -> Value {
        let result = self.temp();
        self.push(Instruction::GetElementPtr {
            result: result.clone(),
            base,
            ptr: ptr.clone(),
            indices,
        });
        Value::Local(result)
    }

    /// The address of field `index` of a struct of type `base` at `ptr`.
    fn field_ptr(&mut self, base: IrType, ptr: &Value, index: u32) -> Value {
        self.gep(
            base,
            ptr,
            vec![
                Typed::int(IrType::I32, 0),
                Typed::int(IrType::I32, i128::from(index)),
            ],
        )
    }

    fn call(
        &mut self,
        return_type: IrType,
        callee: Value,
        args: Vec<Typed>,
        variadic_params: Option<Vec<IrType>>,
    ) -> Option<Typed> {
        let result = (return_type != IrType::Void).then(|| self.temp());
        self.push(Instruction::Call {
            result: result.clone(),
            return_type: return_type.clone(),
            variadic_params,
            callee,
            args,
        });
        result.map(|result| Typed::new(return_type, Value::Local(result)))
    }

    /// Calls an LLVM intrinsic or C function, declaring it on first use.
    fn call_extern(&mut self, name: &str, return_type: IrType, args: Vec<Typed>) -> Option<Typed> {
        let params = args.iter().map(|arg| arg.ty.clone()).collect();
        self.declare(name, return_type.clone(), params, false);
        self.call(return_type, Value::Global(name.to_owned()), args, None)
    }
}

pub(super) fn int_bits(int: IntType) -> u8 {
    if int.pointer_sized { 64 } else { int.bits }
}

/// `value` wrapped to a `bits`-wide two's complement integer.
pub(super) fn wrap_int(value: &BigInt, bits: u32) -> i128 {
    let modulus = BigInt::from(1) << bits;
    let mut wrapped = ((value % &modulus) + &modulus) % &modulus;
    if wrapped >= &modulus >> 1u32 && bits > 1 {
        wrapped -= &modulus;
    }
    i128::try_from(wrapped).unwrap_or(0)
}

/// The array initializer of a `let`'s type, like the `0` in `u8[4]{0}`.
fn array_initializer(typeinfo: Option<&TypeInfo>) -> Option<&Expression> {
    match typeinfo {
        Some(TypeInfo::ArrayType {
            initializer: Some(initializer),
            ..
        }) => Some(initializer),
        _ => None,
    }
}
