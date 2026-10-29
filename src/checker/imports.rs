//! Importing items from other files.
//!
//! An imported file is checked on its own first. Its `pub` items are then
//! copied into the importing file's program with new ids, and marked with the
//! module they come from, so code generation can refer to them by their
//! symbols instead of compiling them again.

use std::collections::HashMap;
use std::rc::Rc;

use super::{
    CheckErrorKind, CheckedProgram, Checker, EnumDef, Export, FunctionDef, ItemPath, Resolved,
    StructDef, Symbol, Type, Variable, VariableInfo, VariantShape, builtins,
};
use crate::parser::Location;

/// What another file declares, for the files that import it.
#[derive(Debug)]
pub struct Interface {
    /// Its module name: the first segment of its symbols.
    pub module: String,
    pub program: CheckedProgram,
    /// Whether checking it found errors, so `program` may be incomplete.
    pub has_errors: bool,
}

impl Interface {
    fn export(&self, name: &str) -> Option<Export> {
        self.program
            .exports
            .iter()
            .find(|(export, _)| export == name)
            .map(|&(_, export)| export)
    }

    /// Whether the file declares `name` at the top level without `pub`.
    fn has_private(&self, name: &str) -> bool {
        let program = &self.program;
        let top_level =
            |module: &Option<String>, path: &ItemPath| module.is_none() && path.is_empty();
        program
            .functions
            .iter()
            .any(|f| f.name == name && top_level(&f.module, &f.path))
            || program
                .structs
                .iter()
                .any(|s| s.name == name && top_level(&s.module, &s.path))
            || program
                .enums
                .iter()
                .any(|e| e.name == name && top_level(&e.module, &e.path))
            || program
                .variables
                .iter()
                .any(|v| v.name == name && v.global && v.module.is_none())
    }
}

/// The files a file can import, keyed by the module path written in the
/// `import`, like `geo` or `shapes::geo`. An `Err` says why the file couldn't
/// be loaded.
pub type Imports = HashMap<String, Result<Rc<Interface>, String>>;

/// Identifies an item across files: its kind, defining module, path and name.
pub(super) type ItemKey = (&'static str, String, ItemPath, String);

impl Checker {
    /// `import geo;`, `import geo::{Point, area}` or `import geo::*`, for a
    /// module that isn't built in.
    pub(super) fn declare_file_import(
        &mut self,
        module: &[String],
        imports: &[String],
        position: Location,
    ) {
        // `import geo;` and `import shapes::geo;` name the module itself
        let module_path = module.join("::");
        if let [name] = imports
            && name != "*"
        {
            let path = if module.is_empty() {
                name.clone()
            } else {
                format!("{module_path}::{name}")
            };
            let names_item = !module.is_empty() && self.imports.contains_key(&module_path);
            if !names_item && self.imports.contains_key(&path) {
                if let Some(interface) = self.load(&path, position) {
                    self.merge_namespaces(&interface, position);
                    self.modules.insert(name.clone(), interface);
                }
                return;
            }
        }

        if module.is_empty() || !self.imports.contains_key(&module_path) {
            let name = if module.is_empty() {
                imports.join("::")
            } else {
                module_path
            };
            self.error(CheckErrorKind::UnknownModule(name), position);
            return;
        }

        let Some(interface) = self.load(&module_path, position) else {
            return;
        };
        self.merge_namespaces(&interface, position);
        for name in imports {
            if name == "*" {
                for (name, export) in interface.program.exports.clone() {
                    // Namespaced items are reached through their namespace
                    if name.contains("::") {
                        continue;
                    }
                    let symbol = self.import(&interface, export);
                    self.declare(&name, symbol, position);
                }
                continue;
            }
            match interface.export(name) {
                Some(export) => {
                    let symbol = self.import(&interface, export);
                    self.declare(name, symbol, position);
                }
                None => self.missing_export(&interface, &module_path, name, position),
            }
        }
    }

    /// Adds the `pub` items in the namespaces of an imported file to this
    /// file's namespaces of the same names, once per file.
    pub(super) fn merge_namespaces(&mut self, interface: &Interface, position: Location) {
        if !self.merged.insert(interface.module.clone()) {
            return;
        }
        for (name, export) in interface.program.exports.clone() {
            let mut path: Vec<String> = name.split("::").map(str::to_owned).collect();
            let Some(name) = path.pop().filter(|_| !path.is_empty()) else {
                continue;
            };
            let symbol = self.import(interface, export);
            self.declare_in_namespace(&path, &name, symbol, position);
        }
    }

    /// The imported file at `path`, reporting why if it couldn't be loaded.
    fn load(&mut self, path: &str, position: Location) -> Option<Rc<Interface>> {
        match self.imports.get(path).cloned()? {
            Ok(interface) => {
                if interface.has_errors {
                    self.error(CheckErrorKind::ModuleHasErrors(path.to_owned()), position);
                }
                Some(interface)
            }
            Err(message) => {
                let module = path.to_owned();
                self.error(CheckErrorKind::ImportFailed { module, message }, position);
                None
            }
        }
    }

    fn missing_export(
        &mut self,
        interface: &Interface,
        module: &str,
        name: &str,
        position: Location,
    ) {
        if interface.has_private(name) {
            let (module, name) = (module.to_owned(), name.to_owned());
            self.error(CheckErrorKind::NotPublic { module, name }, position);
        } else {
            self.unknown_member(module, name, position);
        }
    }

    /// Resolves `member` of the file module imported as `module`.
    pub(super) fn resolve_module_member(
        &mut self,
        module: &str,
        member: &str,
        position: Location,
    ) -> Option<Resolved> {
        if let Some(builtin) = builtins::module(module) {
            return match builtin.member(member) {
                Some(ty) => Some(Resolved::Symbol(Symbol::Primitive(ty))),
                None => {
                    self.unknown_member(module, member, position);
                    None
                }
            };
        }
        let Some(interface) = self.modules.get(module).cloned() else {
            self.error(CheckErrorKind::UndefinedName(module.to_owned()), position);
            return None;
        };
        match interface.export(member) {
            Some(export) => Some(Resolved::Symbol(self.import(&interface, export))),
            None => {
                self.missing_export(&interface, module, member, position);
                None
            }
        }
    }

    /// Copies an exported item into this program, returning its symbol here.
    fn import(&mut self, from: &Interface, export: Export) -> Symbol {
        match export {
            Export::Function(id) => Symbol::Function(self.import_function(from, id)),
            Export::Struct(id) => Symbol::Struct(self.import_struct(from, id)),
            Export::Enum(id) => Symbol::Enum(self.import_enum(from, id)),
            Export::Variable(id) => {
                let id = self.import_variable(from, id);
                let info = &self.program.variables[id];
                Symbol::Variable(Variable {
                    id,
                    ty: info.ty.clone(),
                    constant: info.constant,
                    depth: 0,
                })
            }
        }
    }

    /// The local id of an item already imported under `key`, or `None` after
    /// reserving `id` for it.
    fn imported_id(&mut self, key: ItemKey, id: usize) -> Option<usize> {
        match self.imported.get(&key) {
            Some(&existing) => Some(existing),
            None => {
                self.imported.insert(key, id);
                None
            }
        }
    }

    fn key(
        from: &Interface,
        kind: &'static str,
        module: &Option<String>,
        path: &ItemPath,
        name: &str,
    ) -> ItemKey {
        let module = module.clone().unwrap_or_else(|| from.module.clone());
        (kind, module, path.clone(), name.to_owned())
    }

    fn import_struct(&mut self, from: &Interface, id: usize) -> usize {
        let definition = &from.program.structs[id];
        let key = Self::key(
            from,
            "struct",
            &definition.module,
            &definition.path,
            &definition.name,
        );
        let local = self.program.structs.len();
        if let Some(existing) = self.imported_id(key.clone(), local) {
            return existing;
        }
        // Pushed before its fields are imported, since they may point back
        // at it
        self.program.structs.push(StructDef {
            name: definition.name.clone(),
            path: definition.path.clone(),
            module: Some(key.1),
            position: definition.position,
            fields: Vec::new(),
        });
        let fields = definition
            .fields
            .iter()
            .map(|(name, ty)| (name.clone(), self.import_type(from, ty)))
            .collect();
        self.program.structs[local].fields = fields;
        local
    }

    fn import_enum(&mut self, from: &Interface, id: usize) -> usize {
        let definition = &from.program.enums[id];
        let key = Self::key(
            from,
            "enum",
            &definition.module,
            &definition.path,
            &definition.name,
        );
        let local = self.program.enums.len();
        if let Some(existing) = self.imported_id(key.clone(), local) {
            return existing;
        }
        self.program.enums.push(EnumDef {
            name: definition.name.clone(),
            path: definition.path.clone(),
            module: Some(key.1),
            position: definition.position,
            backing_type: None,
            variants: Vec::new(),
            discriminants: definition.discriminants.clone(),
        });
        let backing_type = definition
            .backing_type
            .as_ref()
            .map(|ty| self.import_type(from, ty));
        let variants = definition
            .variants
            .iter()
            .map(|(name, shape)| {
                let shape = match shape {
                    VariantShape::Unit => VariantShape::Unit,
                    VariantShape::Tuple(types) => VariantShape::Tuple(
                        types.iter().map(|ty| self.import_type(from, ty)).collect(),
                    ),
                    VariantShape::Struct(fields) => VariantShape::Struct(
                        fields
                            .iter()
                            .map(|(name, ty)| (name.clone(), self.import_type(from, ty)))
                            .collect(),
                    ),
                };
                (name.clone(), shape)
            })
            .collect();
        let local_definition = &mut self.program.enums[local];
        local_definition.backing_type = backing_type;
        local_definition.variants = variants;
        local
    }

    fn import_function(&mut self, from: &Interface, id: usize) -> usize {
        let definition = &from.program.functions[id];
        let key = Self::key(
            from,
            "fn",
            &definition.module,
            &definition.path,
            &definition.name,
        );
        let local = self.program.functions.len();
        if let Some(existing) = self.imported_id(key.clone(), local) {
            return existing;
        }
        let params = definition
            .params
            .iter()
            .map(|ty| self.import_type(from, ty))
            .collect();
        let return_type = definition
            .return_type
            .as_ref()
            .map(|ty| self.import_type(from, ty));
        self.program.functions.push(FunctionDef {
            name: definition.name.clone(),
            path: definition.path.clone(),
            module: Some(key.1),
            public: true,
            position: definition.position,
            doc: definition.doc.clone(),
            params,
            param_names: definition.param_names.clone(),
            c_name: definition.c_name.clone(),
            variadic: definition.variadic,
            param_variables: Vec::new(),
            return_type,
            declaration: usize::MAX,
        });
        local
    }

    fn import_variable(&mut self, from: &Interface, id: usize) -> usize {
        let info = &from.program.variables[id];
        let key = Self::key(from, "let", &info.module, &info.path, &info.name);
        let local = self.program.variables.len();
        if let Some(existing) = self.imported_id(key.clone(), local) {
            return existing;
        }
        let ty = self.import_type(from, &info.ty);
        self.program.variables.push(VariableInfo {
            name: info.name.clone(),
            path: info.path.clone(),
            position: info.position,
            ty,
            constant: info.constant,
            global: true,
            module: Some(key.1),
            public: true,
        });
        local
    }

    /// `ty` from the imported file, with its structs and enums imported.
    fn import_type(&mut self, from: &Interface, ty: &Type) -> Type {
        match ty {
            Type::Struct(id) => Type::Struct(self.import_struct(from, *id)),
            Type::Enum(id) => Type::Enum(self.import_enum(from, *id)),
            Type::Pointer(inner) => Type::Pointer(Box::new(self.import_type(from, inner))),
            Type::Reference(inner) => Type::Reference(Box::new(self.import_type(from, inner))),
            Type::Iterator(item) => Type::Iterator(Box::new(self.import_type(from, item))),
            Type::Array { element, size } => Type::Array {
                element: Box::new(self.import_type(from, element)),
                size: *size,
            },
            other => other.clone(),
        }
    }
}
