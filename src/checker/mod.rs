//! Semantic checks after parsing: every name refers to something that exists,
//! every expression has a valid type, variables are assigned before they're
//! read, and constructs like `break` and `yield` only appear where they make
//! sense.

mod builtins;
mod errors;
mod types;

pub use errors::*;
pub use types::{IntType, Type};

use std::collections::{HashMap, HashSet};

use num_bigint::BigInt;

use builtins::{BuiltinMember, PRIMITIVES};
use types::{array_compatible, common_numeric};

use crate::parser::{
    Ast, BinaryOperator, Block, Expression, ExpressionKind, Location, Param, Pattern, PatternKind,
    TypeInfo, UnaryOperator, VariantFields,
};

#[derive(Debug, Clone)]
struct StructDef {
    name: String,
    fields: Vec<(String, Type)>,
}

#[derive(Debug, Clone)]
enum VariantShape {
    Unit,
    Tuple(Vec<Type>),
    Struct(Vec<(String, Type)>),
}

impl VariantShape {
    fn style(&self) -> VariantStyle {
        match self {
            Self::Unit => VariantStyle::Unit,
            Self::Tuple(_) => VariantStyle::Tuple,
            Self::Struct(_) => VariantStyle::Struct,
        }
    }
}

#[derive(Debug, Clone)]
struct EnumDef {
    name: String,
    variants: Vec<(String, VariantShape)>,
}

#[derive(Debug, Clone)]
struct FunctionDef {
    params: Vec<Type>,
    return_type: Option<Type>,
}

#[derive(Debug, Clone)]
struct Variable {
    id: usize,
    ty: Type,
    constant: bool,
    /// How many functions deep it was declared; 0 for globals.
    depth: usize,
}

#[derive(Debug, Clone)]
enum Symbol {
    Variable(Variable),
    Function(usize),
    /// A function from a built-in module, whose signature isn't known yet.
    BuiltinFunction,
    Struct(usize),
    Enum(usize),
    Primitive(Type),
}

impl From<BuiltinMember> for Symbol {
    fn from(member: BuiltinMember) -> Self {
        match member {
            BuiltinMember::Function => Symbol::BuiltinFunction,
            BuiltinMember::Type(ty) => Symbol::Primitive(ty),
        }
    }
}

/// What a name or `a::b` path refers to.
enum Resolved {
    Symbol(Symbol),
    Variant {
        path: String,
        enum_id: usize,
        shape: VariantShape,
    },
}

/// The variables definitely assigned at some point in the code, or `None` if
/// that point can't be reached (it's after a `return`, `break` and so on).
type Assigned = Option<HashSet<usize>>;

/// The variables assigned on both of two paths that join.
fn meet(a: Assigned, b: Assigned) -> Assigned {
    match (a, b) {
        (None, other) | (other, None) => other,
        (Some(a), Some(b)) => Some(a.intersection(&b).copied().collect()),
    }
}

/// Something `break` can leave: a loop, `do` or `switch`.
#[derive(Debug, Default)]
struct Frame {
    /// Whether `continue` can target it.
    repeats: bool,
    /// What's assigned at the `break`s that leave it.
    breaks: Assigned,
    /// What's assigned at the `continue`s that target it.
    continues: Assigned,
}

#[derive(Debug, Clone)]
struct FunctionContext {
    name: String,
    return_type: Option<Type>,
    /// The item type, if this is a generator.
    yield_type: Option<Type>,
    depth: usize,
}

/// Where the checker currently is, for constructs only valid in some places.
#[derive(Debug, Default)]
struct Context {
    function: Option<FunctionContext>,
    frames: Vec<Frame>,
    /// The declared types of the enclosing `let`s being initialized, for `@`.
    initializers: Vec<Option<Type>>,
}

pub struct Checker {
    filename: Option<String>,
    scopes: Vec<HashMap<String, Symbol>>,
    context: Context,
    assigned: Assigned,
    structs: Vec<StructDef>,
    enums: Vec<EnumDef>,
    functions: Vec<FunctionDef>,
    /// Function ids by the first token index of their declaration.
    function_ids: HashMap<usize, usize>,
    next_variable: usize,
    errors: Vec<CheckError>,
}

impl Checker {
    pub fn new(filename: Option<&str>) -> Self {
        let prelude = PRIMITIVES
            .iter()
            .map(|&name| {
                let ty = builtins::primitive(name).expect("primitives have types");
                (name.to_owned(), Symbol::Primitive(ty))
            })
            .collect();

        Self {
            filename: filename.map(str::to_owned),
            scopes: vec![prelude],
            context: Context::default(),
            assigned: Some(HashSet::new()),
            structs: Vec::new(),
            enums: Vec::new(),
            functions: Vec::new(),
            function_ids: HashMap::new(),
            next_variable: 0,
            errors: Vec::new(),
        }
    }

    pub fn check(&mut self, ast: &Ast) -> Result<(), Vec<CheckError>> {
        let filename = self.filename.take();
        *self = Self::new(filename.as_deref());

        self.scopes.push(HashMap::new());
        self.declare_items(ast);

        for expression in ast {
            if is_declaration(&expression.kind) {
                self.check_expression(expression);
            } else {
                self.error(CheckErrorKind::TopLevelStatement, expression.position);
            }
        }

        if !self.errors.is_empty() {
            return Err(std::mem::take(&mut self.errors));
        }
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Scopes
    // -------------------------------------------------------------------------

    fn error(&mut self, kind: CheckErrorKind, position: Location) {
        self.errors.push(CheckError::new(
            kind,
            position.start.line,
            position.start.col,
            self.filename.as_deref(),
        ));
    }

    fn declare(&mut self, name: &str, symbol: Symbol, position: Location) {
        let scope = self.scopes.last().expect("a scope is always open");
        if scope.contains_key(name) {
            self.error(CheckErrorKind::Redeclared(name.to_owned()), position);
            return;
        }
        self.scopes
            .last_mut()
            .expect("a scope is always open")
            .insert(name.to_owned(), symbol);
    }

    fn lookup(&self, name: &str) -> Option<Symbol> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .cloned()
    }

    fn with_scope<T>(&mut self, check: impl FnOnce(&mut Self) -> T) -> T {
        self.scopes.push(HashMap::new());
        let result = check(self);
        self.scopes.pop();
        result
    }

    fn function_depth(&self) -> usize {
        self.context
            .function
            .as_ref()
            .map_or(0, |function| function.depth)
    }

    /// Declares the functions, structs, enums and imports among `items` up
    /// front, so they can be used before the point where they're declared.
    /// Names are declared first, so declarations can refer to each other.
    fn declare_items<'a>(&mut self, items: impl IntoIterator<Item = &'a Expression> + Clone) {
        let mut structs = Vec::new();
        let mut enums = Vec::new();
        let mut functions = Vec::new();

        for item in items.clone() {
            let position = item.position;
            match &item.kind {
                ExpressionKind::FunctionDeclarationExpression { name, .. } => {
                    let id = self.functions.len();
                    self.functions.push(FunctionDef {
                        params: Vec::new(),
                        return_type: None,
                    });
                    self.function_ids.insert(item.tokens.start, id);
                    self.declare(name, Symbol::Function(id), position);
                    functions.push((item, id));
                }
                ExpressionKind::StructDeclarationExpression { name, .. } => {
                    let id = self.structs.len();
                    self.structs.push(StructDef {
                        name: name.clone(),
                        fields: Vec::new(),
                    });
                    self.declare(name, Symbol::Struct(id), position);
                    structs.push((item, id));
                }
                ExpressionKind::EnumDeclarationExpression { name, .. } => {
                    let id = self.enums.len();
                    self.enums.push(EnumDef {
                        name: name.clone(),
                        variants: Vec::new(),
                    });
                    self.declare(name, Symbol::Enum(id), position);
                    enums.push((item, id));
                }
                ExpressionKind::ImportExpression { module, imports } => {
                    self.declare_import(module, imports, position);
                }
                _ => {}
            }
        }

        for (item, id) in structs {
            if let ExpressionKind::StructDeclarationExpression { fields, .. } = &item.kind {
                self.structs[id].fields = self.resolve_fields(fields, item.position);
            }
        }

        for (item, id) in enums {
            if let ExpressionKind::EnumDeclarationExpression {
                backing_type,
                variants,
                ..
            } = &item.kind
            {
                self.declare_enum(id, backing_type.as_ref(), variants, item.position);
            }
        }

        for (item, id) in functions {
            if let ExpressionKind::FunctionDeclarationExpression {
                params,
                return_type,
                ..
            } = &item.kind
            {
                let params = params
                    .iter()
                    .map(|param| self.resolve_type(&param.r#type, item.position))
                    .collect();
                let return_type = return_type
                    .as_ref()
                    .map(|return_type| self.resolve_type(return_type, item.position));
                self.functions[id] = FunctionDef {
                    params,
                    return_type,
                };
            }
        }
    }

    fn declare_enum(
        &mut self,
        id: usize,
        backing_type: Option<&TypeInfo>,
        variants: &[crate::parser::EnumVariant],
        position: Location,
    ) {
        let backing = backing_type.map(|backing_type| {
            let ty = self.resolve_type(backing_type, position);
            if !matches!(ty, Type::Int(_) | Type::Unknown) {
                let name = self.type_name(&ty);
                self.error(CheckErrorKind::InvalidBackingType(name), position);
            }
            ty
        });

        let mut seen = HashSet::new();
        let mut shapes = Vec::new();

        for variant in variants {
            if !seen.insert(variant.name.as_str()) {
                self.duplicate("variant", &variant.name, position);
            }

            let shape = match &variant.fields {
                VariantFields::Unit => VariantShape::Unit,
                VariantFields::Tuple { types } => VariantShape::Tuple(
                    types
                        .iter()
                        .map(|typeinfo| self.resolve_type(typeinfo, position))
                        .collect(),
                ),
                VariantFields::Struct { fields } => {
                    VariantShape::Struct(self.resolve_fields(fields, position))
                }
            };
            shapes.push((variant.name.clone(), shape));

            if let Some(value) = &variant.value {
                let found = self.check_expression(value);
                match &backing {
                    Some(backing) => self.coerce(&found, backing, value.position),
                    None if !found.is_integer() && found != Type::Unknown => {
                        self.mismatch("integer", &found, value.position);
                    }
                    None => {}
                }
            }
        }

        self.enums[id].variants = shapes;
    }

    fn declare_import(&mut self, module: &[String], imports: &[String], position: Location) {
        // `import std;` names a module, which is always visible anyway
        if module.is_empty() {
            for name in imports {
                if builtins::module(name).is_none() {
                    self.error(CheckErrorKind::UnknownModule(name.clone()), position);
                }
            }
            return;
        }

        let module_name = module.join("::");
        let Some(module) = builtins::module(&module_name) else {
            self.error(CheckErrorKind::UnknownModule(module_name), position);
            return;
        };

        for name in imports {
            let names = if name == "*" {
                module.members()
            } else {
                vec![name.as_str()]
            };

            for name in names {
                match module.member(name) {
                    Some(member) => self.declare(name, member.into(), position),
                    None => self.unknown_member(&module_name, name, position),
                }
            }
        }
    }

    /// Resolves `name`, `Enum::Variant` or `module::member`, reporting an
    /// error and returning `None` if it doesn't exist.
    fn resolve_path(&mut self, path: &str, position: Location) -> Option<Resolved> {
        let segments: Vec<&str> = path.split("::").collect();

        match segments.as_slice() {
            [name] => match self.lookup(name) {
                Some(symbol) => Some(Resolved::Symbol(symbol)),
                None => {
                    self.error(CheckErrorKind::UndefinedName(path.to_owned()), position);
                    None
                }
            },
            [container, member] => match self.lookup(container) {
                Some(Symbol::Enum(enum_id)) => {
                    let variant = self.enums[enum_id]
                        .variants
                        .iter()
                        .find(|(name, _)| name == member)
                        .map(|(_, shape)| shape.clone());
                    match variant {
                        Some(shape) => Some(Resolved::Variant {
                            path: path.to_owned(),
                            enum_id,
                            shape,
                        }),
                        None => {
                            self.unknown_member(container, member, position);
                            None
                        }
                    }
                }
                Some(_) => {
                    self.error(CheckErrorKind::NoMembers((*container).to_owned()), position);
                    None
                }
                None => match builtins::module(container) {
                    Some(module) => match module.member(member) {
                        Some(member) => Some(Resolved::Symbol(member.into())),
                        None => {
                            self.unknown_member(container, member, position);
                            None
                        }
                    },
                    None => {
                        let name = (*container).to_owned();
                        self.error(CheckErrorKind::UndefinedName(name), position);
                        None
                    }
                },
            },
            _ => {
                self.error(CheckErrorKind::UndefinedName(path.to_owned()), position);
                None
            }
        }
    }

    fn unknown_member(&mut self, container: &str, member: &str, position: Location) {
        self.error(
            CheckErrorKind::UnknownMember {
                container: container.to_owned(),
                member: member.to_owned(),
            },
            position,
        );
    }

    fn duplicate(&mut self, what: &'static str, name: &str, position: Location) {
        let name = name.to_owned();
        self.error(CheckErrorKind::Duplicate { what, name }, position);
    }

    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    /// Types carry no position of their own, so errors point at `position`,
    /// the declaration the type appears in.
    fn resolve_type(&mut self, typeinfo: &TypeInfo, position: Location) -> Type {
        match typeinfo {
            TypeInfo::NamedType { name } => match self.resolve_path(name, position) {
                Some(Resolved::Symbol(Symbol::Primitive(ty))) => ty,
                Some(Resolved::Symbol(Symbol::Struct(id))) => Type::Struct(id),
                Some(Resolved::Symbol(Symbol::Enum(id))) => Type::Enum(id),
                None => Type::Unknown,
                Some(_) => {
                    self.error(CheckErrorKind::NotAType(name.clone()), position);
                    Type::Unknown
                }
            },
            TypeInfo::PointerType { pointee } => {
                Type::Pointer(Box::new(self.resolve_type(pointee, position)))
            }
            TypeInfo::ReferenceType { referent } => {
                Type::Reference(Box::new(self.resolve_type(referent, position)))
            }
            TypeInfo::IteratorType { item } => {
                Type::Iterator(Box::new(self.resolve_type(item, position)))
            }
            TypeInfo::ArrayType {
                element,
                size,
                initializer,
            } => {
                let ty = Type::Array {
                    element: Box::new(self.resolve_type(element, position)),
                    size: *size,
                };
                // `u8[4]{0}` fills every element with 0
                if let Some(initializer) = initializer {
                    let element = ty.base_element().clone();
                    self.check_coerced(initializer, &element);
                }
                ty
            }
        }
    }

    /// Resolves struct or variant fields, checking for duplicates.
    fn resolve_fields(&mut self, fields: &[Param], position: Location) -> Vec<(String, Type)> {
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();
        for field in fields {
            if !seen.insert(field.name.as_str()) {
                self.duplicate("field", &field.name, position);
            }
            let ty = self.resolve_type(&field.r#type, position);
            resolved.push((field.name.clone(), ty));
        }
        resolved
    }

    fn type_name(&self, ty: &Type) -> String {
        match ty {
            Type::Int(int) => int.name(),
            Type::Float(bits) => format!("f{bits}"),
            Type::Bool => "bool".to_owned(),
            Type::UntypedInt(_) => "integer".to_owned(),
            Type::UntypedFloat => "float".to_owned(),
            Type::Struct(id) => self.structs[*id].name.clone(),
            Type::Enum(id) => self.enums[*id].name.clone(),
            Type::Pointer(pointee) => format!("*{}", self.type_name(pointee)),
            Type::Reference(referent) => format!("&{}", self.type_name(referent)),
            Type::Iterator(item) => format!("iter {}", self.type_name(item)),
            Type::Array { .. } => {
                // Dimensions are written outermost first: `u8[4][8]`
                let mut dimensions = String::new();
                let mut element = ty;
                while let Type::Array {
                    element: inner,
                    size,
                } = element
                {
                    match size {
                        Some(size) => dimensions.push_str(&format!("[{size}]")),
                        None => dimensions.push_str("[?]"),
                    }
                    element = inner;
                }
                format!("{}{dimensions}", self.type_name(element))
            }
            Type::Void => "void".to_owned(),
            Type::Never => "never".to_owned(),
            Type::Unknown => "unknown".to_owned(),
        }
    }

    /// Whether a value of type `found` can be used where `expected` is wanted
    /// without a cast: the same type, a lossless widening, an untyped literal
    /// that fits, an array passed where its size is unknown, or `&T` where `*T`
    /// is wanted.
    fn assignable(&self, found: &Type, expected: &Type) -> bool {
        match (found, expected) {
            (Type::Unknown, _) | (_, Type::Unknown) | (Type::Never, _) => true,
            (Type::Int(from), Type::Int(to)) => from.widens_to(*to),
            (Type::Float(from), Type::Float(to)) => from <= to,
            (Type::UntypedInt(value), Type::Int(to)) => {
                value.as_ref().is_none_or(|value| to.contains(value))
            }
            (Type::UntypedInt(_) | Type::UntypedFloat, Type::Float(_)) => true,
            (Type::Array { .. }, Type::Array { .. }) => array_compatible(found, expected),
            (Type::Reference(from), Type::Pointer(to)) => from == to,
            _ => found == expected,
        }
    }

    /// Reports an error unless `found` can be used where `expected` is wanted.
    fn coerce(&mut self, found: &Type, expected: &Type, position: Location) {
        if let (Type::UntypedInt(Some(value)), Type::Int(int)) = (found, expected)
            && !int.contains(value)
        {
            self.error(
                CheckErrorKind::LiteralOutOfRange {
                    value: value.to_string(),
                    expected: int.name(),
                },
                position,
            );
            return;
        }

        if !self.assignable(found, expected) {
            self.error(
                CheckErrorKind::TypeMismatch {
                    expected: self.type_name(expected),
                    found: self.type_name(found),
                    cast_hint: found.is_numeric() && expected.is_numeric(),
                },
                position,
            );
        }
    }

    fn mismatch(&mut self, expected: &str, found: &Type, position: Location) {
        self.error(
            CheckErrorKind::TypeMismatch {
                expected: expected.to_owned(),
                found: self.type_name(found),
                cast_hint: false,
            },
            position,
        );
    }

    fn check_coerced(&mut self, expression: &Expression, expected: &Type) {
        let found = self.check_expression(expression);
        self.coerce(&found, expected, expression.position);
    }

    fn check_condition(&mut self, condition: &Expression) {
        self.check_coerced(condition, &Type::Bool);
    }

    /// Whether `value as to` is allowed: any implicit conversion, plus
    /// conversions between numbers, from `bool` or a field-less enum to an
    /// integer, between pointers, and between pointers and `usize`.
    fn castable(&self, from: &Type, to: &Type) -> bool {
        if self.assignable(from, to) || (from.is_numeric() && to.is_numeric()) {
            return true;
        }
        match (from, to) {
            (Type::Bool, Type::Int(_)) => true,
            (Type::Enum(id), Type::Int(_)) => self.enums[*id]
                .variants
                .iter()
                .all(|(_, shape)| matches!(shape, VariantShape::Unit)),
            (Type::Pointer(_) | Type::Reference(_), Type::Pointer(_)) => true,
            (Type::Pointer(_), Type::Int(int)) | (Type::Int(int), Type::Pointer(_)) => {
                int.pointer_sized
            }
            _ => false,
        }
    }

    /// The type both operands of an arithmetic or comparison operator are
    /// converted to, reporting untyped literals that don't fit it.
    fn unify_numeric(
        &mut self,
        left: &Type,
        right: &Type,
        left_position: Location,
        right_position: Location,
    ) -> Option<Type> {
        let common = common_numeric(left, right)?;
        if !common.is_untyped() {
            if left.is_untyped() {
                self.coerce(left, &common, left_position);
            }
            if right.is_untyped() {
                self.coerce(right, &common, right_position);
            }
        }
        Some(common)
    }

    // -------------------------------------------------------------------------
    // Variables and flow
    // -------------------------------------------------------------------------

    fn declare_variable(
        &mut self,
        name: &str,
        ty: Type,
        constant: bool,
        assigned: bool,
        position: Location,
    ) {
        let id = self.next_variable;
        self.next_variable += 1;

        let variable = Variable {
            id,
            ty,
            constant,
            depth: self.function_depth(),
        };
        self.declare(name, Symbol::Variable(variable), position);

        if assigned {
            self.mark_assigned(id);
        }
    }

    fn mark_assigned(&mut self, id: usize) {
        if let Some(assigned) = &mut self.assigned {
            assigned.insert(id);
        }
    }

    /// Reports an error if `variable` belongs to an enclosing function.
    fn check_capture(&mut self, name: &str, variable: &Variable, position: Location) -> bool {
        let captured = variable.depth != 0 && variable.depth != self.function_depth();
        if captured {
            self.error(CheckErrorKind::Captured(name.to_owned()), position);
        }
        !captured
    }

    fn read_variable(&mut self, name: &str, variable: &Variable, position: Location) -> Type {
        if !self.check_capture(name, variable, position) {
            return variable.ty.clone();
        }

        // Globals start out zeroed, so only locals are tracked
        let unassigned = variable.depth != 0
            && self
                .assigned
                .as_ref()
                .is_some_and(|assigned| !assigned.contains(&variable.id));
        if unassigned {
            self.error(CheckErrorKind::MaybeUnassigned(name.to_owned()), position);
            // Report it once, not at every later read
            self.mark_assigned(variable.id);
        }

        variable.ty.clone()
    }

    fn push_frame(&mut self, repeats: bool) {
        self.context.frames.push(Frame {
            repeats,
            ..Frame::default()
        });
    }

    fn pop_frame(&mut self) -> Frame {
        self.context.frames.pop().expect("frames are balanced")
    }

    // -------------------------------------------------------------------------
    // Expressions
    // -------------------------------------------------------------------------

    fn check_block(&mut self, block: &Block) -> Type {
        self.with_scope(|checker| checker.check_block_contents(block))
    }

    /// Checks a block's statements in the current scope, for blocks that share
    /// a scope with something declared just outside them, like a function's
    /// parameters.
    fn check_block_contents(&mut self, block: &Block) -> Type {
        self.declare_items(block.body.iter().chain(block.result.as_deref()));
        for expression in &block.body {
            self.check_expression(expression);
        }
        match &block.result {
            Some(result) => self.check_expression(result),
            None if self.assigned.is_none() => Type::Never,
            None => Type::Void,
        }
    }

    fn check_expression(&mut self, expression: &Expression) -> Type {
        let position = expression.position;

        match &expression.kind {
            // Declared up front by `declare_items`
            ExpressionKind::ImportExpression { .. }
            | ExpressionKind::StructDeclarationExpression { .. }
            | ExpressionKind::EnumDeclarationExpression { .. } => Type::Void,
            ExpressionKind::TypeExpression { typeinfo } => {
                self.resolve_type(typeinfo, position);
                Type::Unknown
            }
            ExpressionKind::StringLiteralExpression { value } => Type::Array {
                // Null-terminated
                element: Box::new(Type::Int(IntType::new(false, 8))),
                size: Some(value.len() as u64 + 1),
            },
            ExpressionKind::IntegerLiteralExpression {
                size,
                signed,
                value,
                suffixed,
                ..
            } => {
                let value = BigInt::from(value.clone());
                if !suffixed {
                    return Type::UntypedInt(Some(value));
                }
                let int = IntType::new(*signed, *size);
                if !int.contains(&value) {
                    self.error(
                        CheckErrorKind::LiteralOutOfRange {
                            value: value.to_string(),
                            expected: int.name(),
                        },
                        position,
                    );
                }
                Type::Int(int)
            }
            ExpressionKind::FloatLiteralExpression { size, suffixed, .. } => {
                if *suffixed {
                    Type::Float(*size)
                } else {
                    Type::UntypedFloat
                }
            }
            ExpressionKind::BooleanLiteralExpression { .. } => Type::Bool,
            ExpressionKind::IdentifierExpression { name } => self.check_value_path(name, position),
            ExpressionKind::AtExpression => match self.context.initializers.last() {
                None => {
                    self.misplaced(Construct::At, position);
                    Type::Unknown
                }
                Some(None) => {
                    self.error(CheckErrorKind::AtNeedsType, position);
                    Type::Unknown
                }
                Some(Some(ty)) => ty.clone(),
            },
            ExpressionKind::FunctionDeclarationExpression {
                name, params, body, ..
            } => {
                if let Some(&id) = self.function_ids.get(&expression.tokens.start) {
                    self.check_function(id, name, params, body, position);
                }
                Type::Void
            }
            ExpressionKind::LetExpression {
                constant,
                name,
                typeinfo,
                value,
            } => {
                self.check_let(
                    *constant,
                    name,
                    typeinfo.as_ref(),
                    value.as_deref(),
                    position,
                );
                Type::Void
            }
            ExpressionKind::StructLiteralExpression { name, fields } => {
                let (expected, ty) = match self.resolve_path(name, position) {
                    Some(Resolved::Symbol(Symbol::Struct(id))) => {
                        (Some(self.structs[id].fields.clone()), Type::Struct(id))
                    }
                    Some(Resolved::Variant {
                        enum_id,
                        shape: VariantShape::Struct(fields),
                        ..
                    }) => (Some(fields), Type::Enum(enum_id)),
                    Some(Resolved::Variant { path, shape, .. }) => {
                        let style = shape.style();
                        self.error(CheckErrorKind::VariantStyle { path, style }, position);
                        (None, Type::Unknown)
                    }
                    Some(Resolved::Symbol(_)) => {
                        self.error(CheckErrorKind::NotAStruct(name.clone()), position);
                        (None, Type::Unknown)
                    }
                    None => (None, Type::Unknown),
                };

                let given: Vec<_> = fields
                    .iter()
                    .map(|field| (field.name.as_str(), field.value.position))
                    .collect();
                let names: Option<Vec<String>> = expected
                    .as_ref()
                    .map(|fields| fields.iter().map(|(name, _)| name.clone()).collect());
                self.check_field_names(name, &given, names.as_deref(), true, position);

                for field in fields {
                    let field_type = expected.as_ref().and_then(|expected| {
                        expected
                            .iter()
                            .find(|(name, _)| *name == field.name)
                            .map(|(_, ty)| ty.clone())
                    });
                    match field_type {
                        Some(field_type) => self.check_coerced(&field.value, &field_type),
                        None => {
                            self.check_expression(&field.value);
                        }
                    }
                }

                ty
            }
            ExpressionKind::BlockExpression { block } => self.check_block(block),
            ExpressionKind::AssignmentExpression {
                operator,
                target,
                value,
            } => {
                // A plain `x = ...` writes `x` without reading it
                let (target_type, variable) = self.check_place(target, operator.is_some());
                let value_type = self.check_expression(value);

                match operator {
                    None => self.coerce(&value_type, &target_type, value.position),
                    Some(operator) => {
                        let result = self.binary_result(
                            *operator,
                            &target_type,
                            &value_type,
                            target.position,
                            value.position,
                            position,
                        );
                        self.coerce(&result, &target_type, value.position);
                    }
                }

                if let Some(id) = variable {
                    self.mark_assigned(id);
                }
                Type::Void
            }
            ExpressionKind::CastExpression { value, typeinfo } => {
                let from = self.check_expression(value);
                let to = self.resolve_type(typeinfo, position);
                if !self.castable(&from, &to) {
                    self.error(
                        CheckErrorKind::InvalidCast {
                            from: self.type_name(&from),
                            to: self.type_name(&to),
                        },
                        position,
                    );
                }
                to
            }
            ExpressionKind::BinaryExpression {
                operator,
                left,
                right,
            } => {
                let left_type = self.check_expression(left);
                let right_type = self.check_expression(right);
                self.binary_result(
                    *operator,
                    &left_type,
                    &right_type,
                    left.position,
                    right.position,
                    position,
                )
            }
            ExpressionKind::UnaryExpression { operator, operand } => {
                let ty = self.check_expression(operand);
                self.unary_result(*operator, ty, position)
            }
            ExpressionKind::CallExpression { callee, args } => {
                self.check_call(callee, args, position)
            }
            ExpressionKind::IndexExpression { target, index } => {
                let target_type = self.check_expression(target);
                let index_type = self.check_expression(index);
                if !index_type.is_integer() && index_type != Type::Unknown {
                    self.mismatch("integer", &index_type, index.position);
                }

                match strip_references(target_type) {
                    Type::Array { element, .. } | Type::Pointer(element) => *element,
                    Type::Unknown => Type::Unknown,
                    other => {
                        let name = self.type_name(&other);
                        self.error(CheckErrorKind::NotIndexable(name), position);
                        Type::Unknown
                    }
                }
            }
            ExpressionKind::MemberExpression { target, member } => {
                let target_type = strip_references(self.check_expression(target));
                let field = match &target_type {
                    Type::Struct(id) => self.structs[*id]
                        .fields
                        .iter()
                        .find(|(name, _)| name == member)
                        .map(|(_, ty)| ty.clone()),
                    Type::Unknown => return Type::Unknown,
                    _ => None,
                };

                field.unwrap_or_else(|| {
                    self.error(
                        CheckErrorKind::UnknownField {
                            container: self.type_name(&target_type),
                            field: member.clone(),
                        },
                        position,
                    );
                    Type::Unknown
                })
            }
            ExpressionKind::IfExpression {
                condition,
                then,
                otherwise,
            } => {
                self.check_condition(condition);

                let before = self.assigned.clone();
                let then_type = self.check_block(then);
                let after_then = std::mem::replace(&mut self.assigned, before);

                match otherwise {
                    Some(otherwise) => {
                        let otherwise_type = self.check_expression(otherwise);
                        self.assigned = meet(after_then, self.assigned.take());
                        self.join_branches(then_type, otherwise_type)
                    }
                    None => {
                        self.assigned = meet(after_then, self.assigned.take());
                        Type::Void
                    }
                }
            }
            ExpressionKind::SwitchExpression {
                subject,
                cases,
                default,
            } => {
                self.check_switch(subject, cases, default.as_ref());
                Type::Void
            }
            ExpressionKind::WhileExpression { condition, body } => {
                self.check_condition(condition);

                // The body may not run at all
                let before = self.assigned.clone();
                self.push_frame(true);
                self.check_block(body);
                self.pop_frame();
                self.assigned = before;
                Type::Void
            }
            ExpressionKind::LoopExpression { body } => {
                self.push_frame(true);
                self.check_block(body);
                let frame = self.pop_frame();

                // The only way out is a `break`
                self.assigned = frame.breaks;
                if self.assigned.is_none() {
                    Type::Never
                } else {
                    Type::Void
                }
            }
            ExpressionKind::DoExpression { body, condition } => {
                // Only `do ... while` repeats, so a bare `do` can be left with
                // `break` but not continued
                self.push_frame(condition.is_some());
                self.check_block(body);
                let frame = self.pop_frame();

                if let Some(condition) = condition {
                    self.assigned = meet(self.assigned.take(), frame.continues);
                    self.check_condition(condition);
                }
                self.assigned = meet(self.assigned.take(), frame.breaks);
                Type::Void
            }
            ExpressionKind::ForExpression {
                binding,
                iterable,
                body,
            } => {
                let item = match self.check_expression(iterable) {
                    Type::Iterator(item) => *item,
                    Type::Unknown => Type::Unknown,
                    other => {
                        let name = self.type_name(&other);
                        self.error(CheckErrorKind::NotIterable(name), iterable.position);
                        Type::Unknown
                    }
                };

                // The body may not run at all
                let before = self.assigned.clone();
                self.with_scope(|checker| {
                    checker.declare_variable(binding, item, false, true, position);
                    checker.push_frame(true);
                    checker.check_block_contents(body);
                    checker.pop_frame();
                });
                self.assigned = before;
                Type::Void
            }
            ExpressionKind::ForClauseExpression {
                init,
                condition,
                step,
                each,
                body,
            } => self.with_scope(|checker| {
                if let Some(init) = init {
                    checker.check_expression(init);
                }
                if let Some(condition) = condition {
                    checker.check_condition(condition);
                }

                // Leaving the loop happens when the condition fails, which may
                // be before the body ever runs
                let after_condition = checker.assigned.clone();
                checker.push_frame(true);

                checker.with_scope(|checker| {
                    if let Some(each) = each {
                        checker.check_expression(each);
                    }
                    checker.check_block_contents(body);
                });

                let frame = checker.pop_frame();
                checker.assigned = meet(checker.assigned.take(), frame.continues);
                if let Some(step) = step {
                    checker.check_expression(step);
                }

                // Without a condition, only `break` leaves the loop
                checker.assigned = match condition {
                    Some(_) => after_condition,
                    None => frame.breaks,
                };
                if checker.assigned.is_none() {
                    Type::Never
                } else {
                    Type::Void
                }
            }),
            ExpressionKind::IncrementExpression { target, decrement } => {
                let (ty, _) = self.check_place(target, true);
                if !ty.is_integer() && ty != Type::Unknown {
                    self.error(
                        CheckErrorKind::InvalidOperand {
                            operator: if *decrement { "--" } else { "++" },
                            operand: self.type_name(&ty),
                        },
                        position,
                    );
                }
                Type::Void
            }
            ExpressionKind::ReturnExpression { value } => {
                self.check_return(value.as_deref(), position);
                self.assigned = None;
                Type::Never
            }
            ExpressionKind::YieldExpression { value } => {
                let yield_type = self
                    .context
                    .function
                    .as_ref()
                    .and_then(|function| function.yield_type.clone());
                match yield_type {
                    Some(yield_type) => self.check_coerced(value, &yield_type),
                    None => {
                        self.misplaced(Construct::Yield, position);
                        self.check_expression(value);
                    }
                }
                Type::Void
            }
            ExpressionKind::BreakExpression => {
                let assigned = self.assigned.take();
                match self.context.frames.last_mut() {
                    Some(frame) => frame.breaks = meet(frame.breaks.take(), assigned),
                    None => self.misplaced(Construct::Break, position),
                }
                Type::Never
            }
            ExpressionKind::ContinueExpression => {
                let assigned = self.assigned.take();
                match self
                    .context
                    .frames
                    .iter_mut()
                    .rev()
                    .find(|frame| frame.repeats)
                {
                    Some(frame) => frame.continues = meet(frame.continues.take(), assigned),
                    None => self.misplaced(Construct::Continue, position),
                }
                Type::Never
            }
        }
    }

    fn misplaced(&mut self, construct: Construct, position: Location) {
        self.error(CheckErrorKind::Misplaced(construct), position);
    }

    /// The type of an `if` with an `else`: the branches' common type, or void
    /// if they have none (which is fine when the `if` is used as a statement).
    fn join_branches(&self, a: Type, b: Type) -> Type {
        match (&a, &b) {
            (Type::Never, _) => b,
            (_, Type::Never) => a,
            _ if a.is_numeric() && b.is_numeric() => common_numeric(&a, &b).unwrap_or(Type::Void),
            _ if self.assignable(&a, &b) => b,
            _ if self.assignable(&b, &a) => a,
            _ => Type::Void,
        }
    }

    fn check_let(
        &mut self,
        constant: bool,
        name: &str,
        typeinfo: Option<&TypeInfo>,
        value: Option<&Expression>,
        position: Location,
    ) {
        let declared = typeinfo.map(|typeinfo| self.resolve_type(typeinfo, position));
        // `let a: u8[4]{0};` is filled by its type's initializer
        let initialized = matches!(
            typeinfo,
            Some(TypeInfo::ArrayType {
                initializer: Some(_),
                ..
            })
        );

        let ty = match value {
            Some(value) => {
                self.context.initializers.push(declared.clone());
                let found = self.check_expression(value);
                self.context.initializers.pop();

                match declared {
                    Some(declared) => {
                        self.coerce(&found, &declared, value.position);
                        declared
                    }
                    None => match found {
                        Type::Void => {
                            self.error(CheckErrorKind::NoValue(name.to_owned()), value.position);
                            Type::Unknown
                        }
                        Type::Never => Type::Unknown,
                        found => found.concrete(),
                    },
                }
            }
            None => {
                if constant {
                    self.error(
                        CheckErrorKind::ConstantWithoutValue(name.to_owned()),
                        position,
                    );
                }
                declared.unwrap_or_else(|| {
                    self.error(CheckErrorKind::CannotInfer(name.to_owned()), position);
                    Type::Unknown
                })
            }
        };

        let assigned = value.is_some() || initialized;
        self.declare_variable(name, ty, constant, assigned, position);
    }

    /// A name used as a value: it must be a variable, a constant or a field-less
    /// enum variant.
    fn check_value_path(&mut self, name: &str, position: Location) -> Type {
        match self.resolve_path(name, position) {
            Some(Resolved::Symbol(Symbol::Variable(variable))) => {
                self.read_variable(name, &variable, position)
            }
            Some(Resolved::Symbol(Symbol::Function(_) | Symbol::BuiltinFunction)) => {
                self.error(CheckErrorKind::FunctionAsValue(name.to_owned()), position);
                Type::Unknown
            }
            Some(Resolved::Symbol(Symbol::Struct(_) | Symbol::Enum(_) | Symbol::Primitive(_))) => {
                self.error(CheckErrorKind::NotAValue(name.to_owned()), position);
                Type::Unknown
            }
            Some(Resolved::Variant {
                enum_id,
                shape: VariantShape::Unit,
                ..
            }) => Type::Enum(enum_id),
            Some(Resolved::Variant { path, shape, .. }) => {
                let style = shape.style();
                self.error(CheckErrorKind::VariantStyle { path, style }, position);
                Type::Unknown
            }
            None => Type::Unknown,
        }
    }

    /// Checks something being assigned to, returning its type and, if it's a
    /// variable, the variable's id. `read` is set for compound assignments,
    /// which read the old value.
    fn check_place(&mut self, target: &Expression, read: bool) -> (Type, Option<usize>) {
        match &target.kind {
            ExpressionKind::IdentifierExpression { name } => {
                match self.resolve_path(name, target.position) {
                    Some(Resolved::Symbol(Symbol::Variable(variable))) => {
                        if variable.constant {
                            self.error(
                                CheckErrorKind::AssignToConstant(name.clone()),
                                target.position,
                            );
                        }
                        if read {
                            self.read_variable(name, &variable, target.position);
                        } else if !self.check_capture(name, &variable, target.position) {
                            return (variable.ty, None);
                        }
                        (variable.ty, Some(variable.id))
                    }
                    None => (Type::Unknown, None),
                    Some(_) => {
                        self.error(CheckErrorKind::InvalidAssignmentTarget, target.position);
                        (Type::Unknown, None)
                    }
                }
            }
            ExpressionKind::IndexExpression { .. }
            | ExpressionKind::MemberExpression { .. }
            | ExpressionKind::UnaryExpression {
                operator: UnaryOperator::Deref,
                ..
            } => (self.check_expression(target), None),
            _ => {
                self.error(CheckErrorKind::InvalidAssignmentTarget, target.position);
                self.check_expression(target);
                (Type::Unknown, None)
            }
        }
    }

    fn binary_result(
        &mut self,
        operator: BinaryOperator,
        left: &Type,
        right: &Type,
        left_position: Location,
        right_position: Location,
        position: Location,
    ) -> Type {
        use BinaryOperator as B;

        let comparison = matches!(
            operator,
            B::Eq | B::Neq | B::Lt | B::Gt | B::Le | B::Ge | B::LogicalAnd | B::LogicalOr
        );
        if *left == Type::Unknown || *right == Type::Unknown {
            return if comparison {
                Type::Bool
            } else {
                Type::Unknown
            };
        }

        let valid = match operator {
            B::LogicalAnd | B::LogicalOr => {
                (*left == Type::Bool && *right == Type::Bool).then_some(Type::Bool)
            }
            B::Eq | B::Neq => {
                if left.is_numeric() && right.is_numeric() {
                    self.unify_numeric(left, right, left_position, right_position)
                        .map(|_| Type::Bool)
                } else {
                    let comparable = matches!(
                        left,
                        Type::Bool | Type::Enum(_) | Type::Pointer(_) | Type::Reference(_)
                    );
                    (comparable && left == right).then_some(Type::Bool)
                }
            }
            B::Lt | B::Gt | B::Le | B::Ge => self
                .unify_numeric(left, right, left_position, right_position)
                .map(|_| Type::Bool),
            // The result has the left side's type, whatever the shift amount's
            B::Shl | B::Shr => (left.is_integer() && right.is_integer()).then(|| left.clone()),
            B::And | B::Or | B::Xor => {
                if left.is_integer() && right.is_integer() {
                    self.unify_numeric(left, right, left_position, right_position)
                } else {
                    None
                }
            }
            B::Add | B::Sub | B::Mul | B::Div | B::Mod => {
                self.unify_numeric(left, right, left_position, right_position)
            }
        };

        valid.unwrap_or_else(|| {
            self.error(
                CheckErrorKind::InvalidOperands {
                    operator: binary_symbol(operator),
                    left: self.type_name(left),
                    right: self.type_name(right),
                },
                position,
            );
            if comparison {
                Type::Bool
            } else {
                Type::Unknown
            }
        })
    }

    fn unary_result(&mut self, operator: UnaryOperator, operand: Type, position: Location) -> Type {
        let result = match (operator, operand.clone()) {
            (_, Type::Unknown) => Some(Type::Unknown),
            (UnaryOperator::Neg, Type::UntypedInt(value)) => {
                Some(Type::UntypedInt(value.map(|value| -value)))
            }
            (UnaryOperator::Neg, ty @ (Type::Float(_) | Type::UntypedFloat)) => Some(ty),
            (UnaryOperator::Neg, Type::Int(int)) if int.signed => Some(Type::Int(int)),
            (UnaryOperator::Not, Type::Bool) => Some(Type::Bool),
            (UnaryOperator::Deref, Type::Pointer(inner) | Type::Reference(inner)) => Some(*inner),
            (UnaryOperator::AddressOf, ty) => Some(Type::Reference(Box::new(ty.concrete()))),
            _ => None,
        };

        result.unwrap_or_else(|| {
            let symbol = match operator {
                UnaryOperator::Neg => "-",
                UnaryOperator::Not => "!",
                UnaryOperator::Deref => "*",
                UnaryOperator::AddressOf => "&",
            };
            self.error(
                CheckErrorKind::InvalidOperand {
                    operator: symbol,
                    operand: self.type_name(&operand),
                },
                position,
            );
            Type::Unknown
        })
    }

    fn check_call(&mut self, callee: &Expression, args: &[Expression], position: Location) -> Type {
        // What's called decides the parameter types (if known) and result
        let (params, result) = match &callee.kind {
            ExpressionKind::IdentifierExpression { name } => {
                match self.resolve_path(name, callee.position) {
                    Some(Resolved::Symbol(Symbol::Function(id))) => {
                        let function = self.functions[id].clone();
                        let result = function.return_type.unwrap_or(Type::Void);
                        (Some((name.clone(), function.params)), result)
                    }
                    Some(Resolved::Variant {
                        enum_id,
                        shape: VariantShape::Tuple(types),
                        ..
                    }) => (Some((name.clone(), types)), Type::Enum(enum_id)),
                    Some(Resolved::Variant { path, shape, .. }) => {
                        let style = shape.style();
                        self.error(
                            CheckErrorKind::VariantStyle { path, style },
                            callee.position,
                        );
                        (None, Type::Unknown)
                    }
                    Some(Resolved::Symbol(
                        Symbol::Struct(_) | Symbol::Enum(_) | Symbol::Primitive(_),
                    )) => {
                        self.error(CheckErrorKind::NotAValue(name.clone()), callee.position);
                        (None, Type::Unknown)
                    }
                    Some(Resolved::Symbol(Symbol::Variable(variable))) => {
                        let ty = self.read_variable(name, &variable, callee.position);
                        self.not_callable(&ty, callee.position);
                        (None, Type::Unknown)
                    }
                    Some(Resolved::Symbol(Symbol::BuiltinFunction)) | None => (None, Type::Unknown),
                }
            }
            _ => {
                let ty = self.check_expression(callee);
                self.not_callable(&ty, callee.position);
                (None, Type::Unknown)
            }
        };

        let Some((name, params)) = params else {
            for arg in args {
                self.check_expression(arg);
            }
            return result;
        };

        if params.len() != args.len() {
            self.error(
                CheckErrorKind::ArgumentCount {
                    name,
                    expected: params.len(),
                    found: args.len(),
                },
                position,
            );
        }

        for (index, arg) in args.iter().enumerate() {
            match params.get(index) {
                Some(param) => self.check_coerced(arg, param),
                None => {
                    self.check_expression(arg);
                }
            }
        }

        result
    }

    fn not_callable(&mut self, ty: &Type, position: Location) {
        if *ty != Type::Unknown {
            let name = self.type_name(ty);
            self.error(CheckErrorKind::NotCallable(name), position);
        }
    }

    /// Checks named fields given for `container` against the `expected` ones
    /// (if known): no duplicates, no unknown names and, if `complete`, none
    /// missing.
    fn check_field_names(
        &mut self,
        container: &str,
        given: &[(&str, Location)],
        expected: Option<&[String]>,
        complete: bool,
        position: Location,
    ) {
        let mut seen = HashSet::new();
        for &(name, field_position) in given {
            if !seen.insert(name) {
                self.duplicate("field", name, field_position);
            }
        }

        let Some(expected) = expected else {
            return;
        };

        for &(name, field_position) in given {
            if !expected.iter().any(|field| field == name) {
                self.error(
                    CheckErrorKind::UnknownField {
                        container: container.to_owned(),
                        field: name.to_owned(),
                    },
                    field_position,
                );
            }
        }

        let missing: Vec<_> = expected
            .iter()
            .filter(|field| !seen.contains(field.as_str()))
            .cloned()
            .collect();
        if complete && !missing.is_empty() {
            self.error(
                CheckErrorKind::MissingFields {
                    container: container.to_owned(),
                    fields: missing,
                },
                position,
            );
        }
    }

    fn check_function(
        &mut self,
        id: usize,
        name: &str,
        params: &[Param],
        body: &Block,
        position: Location,
    ) {
        let function = self.functions[id].clone();
        let yield_type = match &function.return_type {
            Some(Type::Iterator(item)) => Some((**item).clone()),
            _ => None,
        };
        let generator = yield_type.is_some();

        // A nested function can't `break` out of, `yield` for, or use the
        // variables of the code around it
        let context = Context {
            function: Some(FunctionContext {
                name: name.to_owned(),
                return_type: function.return_type.clone(),
                yield_type,
                depth: self.function_depth() + 1,
            }),
            ..Context::default()
        };
        let outer_context = std::mem::replace(&mut self.context, context);
        let outer_assigned = self.assigned.replace(HashSet::new());

        // Parameters share the body's scope, so the body can't redeclare them
        self.with_scope(|checker| {
            for (param, ty) in params.iter().zip(&function.params) {
                checker.declare_variable(&param.name, ty.clone(), false, true, position);
            }

            let result = checker.check_block_contents(body);
            let reachable_end = checker.assigned.is_some();

            match (&function.return_type, &body.result) {
                _ if generator => {}
                (Some(return_type), Some(value)) if result != Type::Void => {
                    checker.coerce(&result, return_type, value.position);
                }
                (Some(return_type), _) if reachable_end => {
                    checker.error(
                        CheckErrorKind::MissingReturn {
                            function: name.to_owned(),
                            return_type: checker.type_name(return_type),
                        },
                        position,
                    );
                }
                _ => {}
            }
        });

        self.context = outer_context;
        self.assigned = outer_assigned;
    }

    fn check_return(&mut self, value: Option<&Expression>, position: Location) {
        let Some(function) = self.context.function.clone() else {
            self.misplaced(Construct::Return, position);
            if let Some(value) = value {
                self.check_expression(value);
            }
            return;
        };

        if function.yield_type.is_some() {
            if let Some(value) = value {
                self.error(CheckErrorKind::GeneratorReturnValue, position);
                self.check_expression(value);
            }
            return;
        }

        match (&function.return_type, value) {
            (Some(return_type), Some(value)) => self.check_coerced(value, return_type),
            (Some(return_type), None) => self.error(
                CheckErrorKind::MissingReturnValue {
                    function: function.name.clone(),
                    return_type: self.type_name(return_type),
                },
                position,
            ),
            (None, Some(value)) => {
                self.error(
                    CheckErrorKind::UnexpectedReturnValue(function.name),
                    position,
                );
                self.check_expression(value);
            }
            (None, None) => {}
        }
    }

    fn check_switch(
        &mut self,
        subject: &Expression,
        cases: &[crate::parser::Case],
        default: Option<&Block>,
    ) {
        let subject_type = self.check_expression(subject).concrete();

        // Which case runs isn't known, so each starts from the state before
        // the switch. That ignores what a case falling through has assigned,
        // which can only make the result more cautious.
        let before = self.assigned.clone();
        let mut after: Assigned = None;
        self.push_frame(false);

        for (index, case) in cases.iter().enumerate() {
            self.assigned = before.clone();

            self.with_scope(|checker| {
                let mut bindings = Vec::new();
                checker.check_pattern(&case.pattern, &subject_type, &mut bindings);

                let falls_into = index > 0 && !ends_with_jump(&cases[index - 1].body);
                if falls_into && !bindings.is_empty() {
                    checker.error(
                        CheckErrorKind::FallthroughIntoBindings,
                        case.pattern.position,
                    );
                }

                for (name, ty, position) in bindings {
                    checker.declare_variable(&name, ty, false, true, position);
                }
                checker.check_block_contents(&case.body);
            });

            after = meet(after, self.assigned.take());
        }

        match default {
            Some(default) => {
                self.assigned = before;
                self.check_block(default);
                after = meet(after, self.assigned.take());
            }
            // No case may match at all
            None => after = meet(after, before),
        }

        let frame = self.pop_frame();
        self.assigned = meet(after, frame.breaks);
    }

    // -------------------------------------------------------------------------
    // Patterns
    // -------------------------------------------------------------------------

    /// Checks `pattern` against a value of type `expected`, collecting the
    /// names it binds and their types into `bindings`.
    fn check_pattern(
        &mut self,
        pattern: &Pattern,
        expected: &Type,
        bindings: &mut Vec<(String, Type, Location)>,
    ) {
        let position = pattern.position;

        match &pattern.kind {
            PatternKind::WildcardPattern => {}
            PatternKind::BindingPattern { name } => {
                bindings.push((name.clone(), expected.clone(), position));
            }
            PatternKind::ValuePattern { value } => self.check_coerced(value, expected),
            PatternKind::TupleVariantPattern { path, elements } => {
                let types = match self.resolve_variant_pattern(path, expected, position) {
                    Some(VariantShape::Tuple(types)) => {
                        if types.len() != elements.len() {
                            self.error(
                                CheckErrorKind::FieldCount {
                                    name: path.clone(),
                                    expected: types.len(),
                                    found: elements.len(),
                                },
                                position,
                            );
                        }
                        types
                    }
                    Some(shape) => {
                        let style = shape.style();
                        let path = path.clone();
                        self.error(CheckErrorKind::VariantStyle { path, style }, position);
                        Vec::new()
                    }
                    None => Vec::new(),
                };

                for (index, element) in elements.iter().enumerate() {
                    let ty = types.get(index).cloned().unwrap_or(Type::Unknown);
                    self.check_pattern(element, &ty, bindings);
                }
            }
            PatternKind::StructVariantPattern { path, fields } => {
                let expected_fields = match self.resolve_variant_pattern(path, expected, position) {
                    Some(VariantShape::Struct(fields)) => Some(fields),
                    Some(shape) => {
                        let style = shape.style();
                        let path = path.clone();
                        self.error(CheckErrorKind::VariantStyle { path, style }, position);
                        None
                    }
                    None => None,
                };

                // A pattern may leave fields out; those parts aren't matched
                let given: Vec<_> = fields
                    .iter()
                    .map(|field| (field.name.as_str(), field.pattern.position))
                    .collect();
                let names: Option<Vec<String>> = expected_fields
                    .as_ref()
                    .map(|fields| fields.iter().map(|(name, _)| name.clone()).collect());
                self.check_field_names(path, &given, names.as_deref(), false, position);

                for field in fields {
                    let ty = expected_fields
                        .as_ref()
                        .and_then(|expected| {
                            expected
                                .iter()
                                .find(|(name, _)| *name == field.name)
                                .map(|(_, ty)| ty.clone())
                        })
                        .unwrap_or(Type::Unknown);
                    self.check_pattern(&field.pattern, &ty, bindings);
                }
            }
        }
    }

    /// Resolves the variant a pattern names and checks it belongs to the enum
    /// being matched, returning its shape.
    fn resolve_variant_pattern(
        &mut self,
        path: &str,
        expected: &Type,
        position: Location,
    ) -> Option<VariantShape> {
        match self.resolve_path(path, position)? {
            Resolved::Variant { enum_id, shape, .. } => {
                let matches = match expected {
                    Type::Enum(id) => *id == enum_id,
                    Type::Unknown => true,
                    _ => false,
                };
                if !matches {
                    self.error(
                        CheckErrorKind::PatternMismatch {
                            path: path.to_owned(),
                            subject: self.type_name(expected),
                        },
                        position,
                    );
                }
                Some(shape)
            }
            Resolved::Symbol(_) => {
                self.error(CheckErrorKind::NotAVariant(path.to_owned()), position);
                None
            }
        }
    }
}

/// `&T` and `*T` are looked through for indexing and member access.
fn strip_references(ty: Type) -> Type {
    match ty {
        Type::Reference(inner) | Type::Pointer(inner) => strip_references(*inner),
        other => other,
    }
}

fn binary_symbol(operator: BinaryOperator) -> &'static str {
    use BinaryOperator as B;
    match operator {
        B::LogicalOr => "||",
        B::LogicalAnd => "&&",
        B::Or => "|",
        B::Xor => "^",
        B::And => "&",
        B::Eq => "==",
        B::Neq => "!=",
        B::Lt => "<",
        B::Gt => ">",
        B::Le => "<=",
        B::Ge => ">=",
        B::Shl => "<<",
        B::Shr => ">>",
        B::Add => "+",
        B::Sub => "-",
        B::Mul => "*",
        B::Div => "/",
        B::Mod => "%",
    }
}

fn is_declaration(kind: &ExpressionKind) -> bool {
    matches!(
        kind,
        ExpressionKind::ImportExpression { .. }
            | ExpressionKind::FunctionDeclarationExpression { .. }
            | ExpressionKind::StructDeclarationExpression { .. }
            | ExpressionKind::EnumDeclarationExpression { .. }
            | ExpressionKind::LetExpression { .. }
    )
}

/// Whether a `case` body ends by jumping away, so it can't fall through into
/// the next case. Only the last statement is looked at, so a body ending in
/// an `if` whose branches all return still counts as falling through.
fn ends_with_jump(body: &Block) -> bool {
    body.result
        .as_deref()
        .or(body.body.last())
        .is_some_and(|last| {
            matches!(
                last.kind,
                ExpressionKind::BreakExpression
                    | ExpressionKind::ReturnExpression { .. }
                    | ExpressionKind::ContinueExpression
            )
        })
}
