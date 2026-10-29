//! What the language server knows about one document: its tokens, syntax
//! tree, checked program and problems, plus what each identifier token is and
//! refers to. Positions here are 0-based lines and *character* columns, like
//! the compiler's; the server converts them to LSP positions.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::checker::{
    CheckedProgram, Checker, Resolution, Type, VariantShape, is_primitive, module_members,
};
use crate::parser::visit::{Node, walk};
use crate::parser::{Ast, ExpressionKind, Parser, PatternKind};
use crate::tokenizer::{Keyword, Punctuation, Token, TokenKind, TokenStream, Tokenizer};

/// The semantic token types the server reports, in legend order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenClass {
    Keyword,
    Number,
    String,
    Operator,
    Namespace,
    Type,
    Struct,
    Enum,
    EnumMember,
    Function,
    Parameter,
    Variable,
    Property,
}

impl TokenClass {
    pub const ALL: [TokenClass; 13] = [
        Self::Keyword,
        Self::Number,
        Self::String,
        Self::Operator,
        Self::Namespace,
        Self::Type,
        Self::Struct,
        Self::Enum,
        Self::EnumMember,
        Self::Function,
        Self::Parameter,
        Self::Variable,
        Self::Property,
    ];

    /// The standard LSP name of the token type.
    pub fn name(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::Number => "number",
            Self::String => "string",
            Self::Operator => "operator",
            Self::Namespace => "namespace",
            Self::Type => "type",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::EnumMember => "enumMember",
            Self::Function => "function",
            Self::Parameter => "parameter",
            Self::Variable => "variable",
            Self::Property => "property",
        }
    }
}

/// Semantic token modifiers, as bit flags in legend order.
pub mod modifiers {
    pub const DECLARATION: u32 = 1;
    pub const READONLY: u32 = 2;
    pub const DEFAULT_LIBRARY: u32 = 4;
    pub const NAMES: [&str; 3] = ["declaration", "readonly", "defaultLibrary"];
}

use modifiers::{DECLARATION, DEFAULT_LIBRARY, READONLY};

/// What an identifier refers to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Target {
    Variable(usize),
    Function(usize),
    Struct(usize),
    Enum(usize),
    Variant(usize, usize),
    Field(usize, String),
    Primitive(String),
    Module(String),
    /// A namespace, by its full path.
    Namespace(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    pub column: usize,
    /// In characters.
    pub length: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticToken {
    pub line: usize,
    pub column: usize,
    pub length: usize,
    pub class: TokenClass,
    pub modifiers: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Namespace,
    Function,
    Struct,
    Enum,
    Variable,
    Constant,
    Field,
    EnumMember,
}

/// An item in the document's outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// The whole declaration: `((line, column), (line, column))`.
    pub range: ((usize, usize), (usize, usize)),
    /// Its name.
    pub selection: ((usize, usize), (usize, usize)),
    pub children: Vec<Symbol>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Type,
    Function,
    Variable,
    Field,
    EnumMember,
    Module,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
}

pub struct Analysis {
    pub lines: Vec<String>,
    pub tokens: TokenStream,
    pub ast: Ast,
    pub program: CheckedProgram,
    pub problems: Vec<Problem>,
    classes: Vec<Option<(TokenClass, u32)>>,
    targets: HashMap<usize, Target>,
    declarations: HashMap<Target, usize>,
}

fn token_length(token: &Token) -> usize {
    token.raw.chars().count()
}

pub fn analyze(source: &str, file: Option<&str>) -> Analysis {
    let lines: Vec<String> = source
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_owned())
        .collect();

    let mut problems = Vec::new();
    let mut report = |line: usize, column: usize, message: String| {
        let length = lines
            .get(line)
            .map_or(1, |text| problem_length(text, column));
        problems.push(Problem {
            line,
            column,
            length,
            message,
        });
    };

    let (tokens, errors) = Tokenizer::new(source, file).tokenize_partial();
    for error in &errors {
        report(error.line, error.column, error.message());
    }

    // The parser recovers from errors, so even a broken document has a tree
    let mut parser = Parser::new(tokens, file);
    let parsed = parser.parse().is_ok();
    for error in &parser.errors {
        report(error.line, error.column, error.message());
    }
    let ast = parser.node.clone();
    // Without comments, so the AST's token ranges index into it
    let tokens = std::mem::take(&mut parser.tokens);

    // Imported files are read from disk, as last saved
    let imports = file
        .map(|file| crate::modules::load_imports(Path::new(file), &ast).imports)
        .unwrap_or_default();
    let (program, check_errors) = Checker::new(file).with_imports(imports).check_partial(&ast);
    // Statements the parser skipped make the checker's errors misleading
    if errors.is_empty() && parsed {
        for error in &check_errors {
            report(error.line, error.column, error.message());
        }
    }

    let mut analysis = Analysis {
        lines,
        classes: vec![None; tokens.len()],
        tokens,
        ast,
        program,
        problems,
        targets: HashMap::new(),
        declarations: HashMap::new(),
    };
    analysis.classify();
    analysis
}

/// How many characters of `line` to underline for a problem at `column`: a
/// whole name or number, otherwise one character.
fn problem_length(line: &str, column: usize) -> usize {
    let word = |c: &char| c.is_ascii_alphanumeric() || *c == '_';
    let chars: Vec<char> = line.chars().collect();
    match chars.get(column) {
        Some(c) if word(c) => chars[column..].iter().take_while(|c| word(c)).count(),
        _ => 1,
    }
}

fn is_symbol(token: Option<&Token>, symbol: Punctuation) -> bool {
    matches!(token, Some(Token { kind: TokenKind::Punctuation { symbol: s }, .. }) if *s == symbol)
}

fn is_identifier(token: Option<&Token>) -> bool {
    matches!(
        token,
        Some(Token {
            kind: TokenKind::Identifier { .. },
            ..
        })
    )
}

impl Analysis {
    // -------------------------------------------------------------------------
    // Classifying tokens
    // -------------------------------------------------------------------------

    fn set(&mut self, index: usize, class: TokenClass, modifiers: u32) {
        if let Some(slot @ None) = self.classes.get_mut(index) {
            *slot = Some((class, modifiers));
        }
    }

    /// Records that the identifier at `index` refers to `target`, and is its
    /// declaration if `modifiers` says so.
    fn bind(&mut self, index: usize, class: TokenClass, modifiers: u32, target: Option<Target>) {
        if !is_identifier(self.tokens.get(index)) {
            return;
        }
        self.set(index, class, modifiers);
        if let Some(target) = target {
            if modifiers & DECLARATION != 0 {
                self.declarations.entry(target.clone()).or_insert(index);
            }
            self.targets.entry(index).or_insert(target);
        }
    }

    /// The identifier tokens of the path starting at `start`: `a::b::c`.
    fn path_tokens(&self, start: usize) -> Vec<usize> {
        let mut indices = Vec::new();
        let mut index = start;
        while is_identifier(self.tokens.get(index)) {
            indices.push(index);
            if !is_symbol(self.tokens.get(index + 1), Punctuation::Namespace) {
                break;
            }
            index += 2;
        }
        indices
    }

    /// Classifies the path at `start` from what it resolved to.
    fn classify_path(
        &mut self,
        start: usize,
        resolution: Option<&Resolution>,
        parameters: &HashSet<usize>,
    ) {
        let path = self.path_tokens(start);
        let Some((&last, prefix)) = path.split_last() else {
            return;
        };

        match resolution {
            Some(Resolution::Variable(variable)) => {
                let info = &self.program.variables[*variable];
                let (class, modifiers) = if parameters.contains(variable) {
                    (TokenClass::Parameter, 0)
                } else if info.constant {
                    (TokenClass::Variable, READONLY)
                } else {
                    (TokenClass::Variable, 0)
                };
                self.bind(last, class, modifiers, Some(Target::Variable(*variable)));
            }
            Some(Resolution::Function(function)) => {
                let modifiers = self.library_modifier(*function);
                self.bind(
                    last,
                    TokenClass::Function,
                    modifiers,
                    Some(Target::Function(*function)),
                );
            }
            Some(&Resolution::Variant { enum_id, index }) => {
                // Any segments before the enum's are namespaces or a module
                if let Some(&token) = prefix.last() {
                    self.bind(token, TokenClass::Enum, 0, Some(Target::Enum(enum_id)));
                }
                self.bind(
                    last,
                    TokenClass::EnumMember,
                    0,
                    Some(Target::Variant(enum_id, index)),
                );
            }
            Some(Resolution::Struct(id)) => {
                self.bind(last, TokenClass::Struct, 0, Some(Target::Struct(*id)));
            }
            None => {}
        }
    }

    /// The names of the `name: ...` pairs directly inside the brackets that
    /// open at `open` (a `(` or `{`), like parameters or struct fields.
    fn named_members(&self, open: usize) -> Vec<usize> {
        let mut names = Vec::new();
        let mut depth = 0usize;
        for index in open..self.tokens.len() {
            match &self.tokens[index].kind {
                TokenKind::Punctuation {
                    symbol: Punctuation::LParen | Punctuation::LBrace | Punctuation::LBracket,
                } => depth += 1,
                TokenKind::Punctuation {
                    symbol: Punctuation::RParen | Punctuation::RBrace | Punctuation::RBracket,
                } => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Identifier { .. }
                    if depth == 1
                        && is_symbol(self.tokens.get(index + 1), Punctuation::Colon)
                        && !is_symbol(
                            self.tokens.get(index.wrapping_sub(1)),
                            Punctuation::Colon,
                        ) =>
                {
                    names.push(index);
                }
                _ => {}
            }
        }
        names
    }

    fn classify(&mut self) {
        // What the tokens are by themselves
        for index in 0..self.tokens.len() {
            let class = match &self.tokens[index].kind {
                TokenKind::Keyword { .. } | TokenKind::BooleanLiteral { .. } => TokenClass::Keyword,
                TokenKind::IntegerLiteral { .. } | TokenKind::FloatLiteral { .. } => {
                    TokenClass::Number
                }
                TokenKind::StringLiteral { .. } => TokenClass::String,
                TokenKind::Punctuation { symbol } => match symbol {
                    Punctuation::LParen
                    | Punctuation::RParen
                    | Punctuation::LBrace
                    | Punctuation::RBrace
                    | Punctuation::LBracket
                    | Punctuation::RBracket
                    | Punctuation::Semi
                    | Punctuation::Comma
                    | Punctuation::Colon
                    | Punctuation::Namespace
                    | Punctuation::Dot => continue,
                    _ => TokenClass::Operator,
                },
                // The parser sets comments aside, so they never get here
                TokenKind::Identifier { .. } | TokenKind::Comment { .. } => continue,
            };
            self.set(index, class, 0);
        }

        let parameters: HashSet<usize> = self
            .program
            .functions
            .iter()
            .flat_map(|function| function.param_variables.iter().copied())
            .collect();

        // What the syntax tree says about the identifiers
        let ast = std::mem::take(&mut self.ast);
        let mut nodes = Vec::new();
        walk(&ast, &mut |node| nodes.push(node));
        for node in nodes {
            self.classify_node(node, &parameters);
        }
        self.ast = ast;

        // Names the tree doesn't locate, mostly types
        for index in 0..self.tokens.len() {
            if self.classes[index].is_some() {
                continue;
            }
            let TokenKind::Identifier { name } = &self.tokens[index].kind else {
                continue;
            };
            let name = name.clone();
            let after_namespace = is_symbol(self.tokens.get(index + 1), Punctuation::Namespace);

            if is_primitive(&name) {
                self.bind(
                    index,
                    TokenClass::Type,
                    DEFAULT_LIBRARY,
                    Some(Target::Primitive(name)),
                );
            } else if let Some(id) = self.program.structs.iter().position(|s| s.name == name) {
                self.bind(index, TokenClass::Struct, 0, Some(Target::Struct(id)));
            } else if let Some(id) = self.program.enums.iter().position(|e| e.name == name) {
                self.bind(index, TokenClass::Enum, 0, Some(Target::Enum(id)));
            } else if after_namespace && module_members(&name).is_some() {
                self.bind(index, TokenClass::Namespace, 0, Some(Target::Module(name)));
            } else if after_namespace {
                // A namespace, or an imported file's module
                self.set(index, TokenClass::Namespace, 0);
            } else if is_symbol(self.tokens.get(index.wrapping_sub(1)), Punctuation::Dot) {
                self.set(index, TokenClass::Property, 0);
            }
        }
    }

    fn function_id(&self, declaration: usize) -> Option<usize> {
        self.program
            .functions
            .iter()
            .position(|function| function.declaration == declaration)
    }

    fn classify_node(&mut self, node: Node, parameters: &HashSet<usize>) {
        match node {
            Node::Expression(e) => {
                let start = e.tokens.start;
                match &e.kind {
                    ExpressionKind::FunctionDeclarationExpression { .. } => {
                        let function = self.function_id(e.id);
                        let target = function.map(Target::Function);
                        self.bind(start + 1, TokenClass::Function, DECLARATION, target);

                        let variables = function
                            .map(|id| self.program.functions[id].param_variables.clone())
                            .unwrap_or_default();
                        for (position, index) in
                            self.named_members(start + 2).into_iter().enumerate()
                        {
                            let target = variables.get(position).copied().map(Target::Variable);
                            self.bind(index, TokenClass::Parameter, DECLARATION, target);
                        }
                    }
                    ExpressionKind::NamespaceExpression { .. } => {
                        for index in self.path_tokens(start + 1) {
                            self.set(index, TokenClass::Namespace, DECLARATION);
                        }
                    }
                    ExpressionKind::LetExpression { constant, .. } => {
                        let target = self
                            .program
                            .declarations
                            .get(&e.id)
                            .copied()
                            .map(Target::Variable);
                        let modifiers = if *constant {
                            DECLARATION | READONLY
                        } else {
                            DECLARATION
                        };
                        self.bind(start + 1, TokenClass::Variable, modifiers, target);
                    }
                    ExpressionKind::StructDeclarationExpression { name, .. } => {
                        let id = self
                            .program
                            .structs
                            .iter()
                            .position(|s| s.name == *name && s.position == e.position);
                        self.bind(
                            start + 1,
                            TokenClass::Struct,
                            DECLARATION,
                            id.map(Target::Struct),
                        );
                        if let Some(open) = (start..e.tokens.end)
                            .find(|&i| is_symbol(self.tokens.get(i), Punctuation::LBrace))
                        {
                            for index in self.named_members(open) {
                                let field = self.tokens[index].raw.clone();
                                let target = id.map(|id| Target::Field(id, field));
                                self.bind(index, TokenClass::Property, DECLARATION, target);
                            }
                        }
                    }
                    ExpressionKind::EnumDeclarationExpression { name, .. } => {
                        let id = self.program.enums.iter().position(|definition| {
                            definition.name == *name && definition.position == e.position
                        });
                        self.bind(
                            start + 1,
                            TokenClass::Enum,
                            DECLARATION,
                            id.map(Target::Enum),
                        );

                        let Some(open) = (start..e.tokens.end)
                            .find(|&i| is_symbol(self.tokens.get(i), Punctuation::LBrace))
                        else {
                            return;
                        };
                        let mut depth = 0;
                        let mut variant = 0;
                        for index in open..e.tokens.end {
                            match &self.tokens[index].kind {
                                TokenKind::Punctuation {
                                    symbol: Punctuation::LBrace | Punctuation::LParen,
                                } => depth += 1,
                                TokenKind::Punctuation {
                                    symbol: Punctuation::RBrace | Punctuation::RParen,
                                } => depth -= 1,
                                TokenKind::Identifier { .. } if depth == 1 => {
                                    let previous = self.tokens.get(index - 1);
                                    if is_symbol(previous, Punctuation::LBrace)
                                        || is_symbol(previous, Punctuation::Comma)
                                    {
                                        let target = id.map(|id| Target::Variant(id, variant));
                                        self.bind(
                                            index,
                                            TokenClass::EnumMember,
                                            DECLARATION,
                                            target,
                                        );
                                        variant += 1;
                                    }
                                }
                                TokenKind::Identifier { .. }
                                    if depth == 2
                                        && is_symbol(
                                            self.tokens.get(index + 1),
                                            Punctuation::Colon,
                                        ) =>
                                {
                                    self.bind(index, TokenClass::Property, DECLARATION, None);
                                }
                                _ => {}
                            }
                        }
                    }
                    ExpressionKind::IdentifierExpression { .. } => {
                        let resolution = self.program.resolutions.get(&e.id).cloned();
                        self.classify_path(start, resolution.as_ref(), parameters);
                    }
                    ExpressionKind::StructLiteralExpression { .. } => {
                        let resolution = self.program.resolutions.get(&e.id).cloned();
                        self.classify_path(start, resolution.as_ref(), parameters);

                        let struct_id = match resolution {
                            Some(Resolution::Struct(id)) => Some(id),
                            _ => None,
                        };
                        let path = self.path_tokens(start);
                        if let Some(&last) = path.last() {
                            for index in self.named_members(last + 1) {
                                let field = self.tokens[index].raw.clone();
                                let target = struct_id.map(|id| Target::Field(id, field));
                                self.bind(index, TokenClass::Property, 0, target);
                            }
                        }
                    }
                    ExpressionKind::MemberExpression { target, member } => {
                        let mut ty = self.program.types.get(&target.id).cloned();
                        while let Some(Type::Reference(inner) | Type::Pointer(inner)) = ty {
                            ty = Some(*inner);
                        }
                        let field = match ty {
                            Some(Type::Struct(id)) => Some(Target::Field(id, member.clone())),
                            _ => None,
                        };
                        self.bind(e.tokens.end - 1, TokenClass::Property, 0, field);
                    }
                    ExpressionKind::ForExpression { .. } => {
                        let binding = if is_symbol(self.tokens.get(start + 1), Punctuation::LParen)
                        {
                            start + 2
                        } else {
                            start + 1
                        };
                        let target = self
                            .program
                            .declarations
                            .get(&e.id)
                            .copied()
                            .map(Target::Variable);
                        self.bind(binding, TokenClass::Variable, DECLARATION, target);
                    }
                    ExpressionKind::ImportExpression { module, .. } => {
                        let mut path = Vec::new();
                        for index in start..e.tokens.end {
                            let TokenKind::Identifier { name } = &self.tokens[index].kind else {
                                continue;
                            };
                            let name = name.clone();
                            if is_symbol(self.tokens.get(index + 1), Punctuation::Namespace) {
                                path.push(name.clone());
                                let target =
                                    if self.program.namespaces.contains_key(&path.join("::")) {
                                        Target::Namespace(path.join("::"))
                                    } else {
                                        Target::Module(name)
                                    };
                                self.bind(index, TokenClass::Namespace, 0, Some(target));
                            } else if is_primitive(&name) {
                                self.bind(
                                    index,
                                    TokenClass::Type,
                                    DEFAULT_LIBRARY,
                                    Some(Target::Primitive(name)),
                                );
                            } else if let Some(id) =
                                self.program.functions.iter().position(|function| {
                                    function.name == name && function.path == *module
                                })
                            {
                                let modifiers = self.library_modifier(id);
                                let target = Some(Target::Function(id));
                                self.bind(index, TokenClass::Function, modifiers, target);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Node::Pattern(pattern) => match &pattern.kind {
                PatternKind::BindingPattern { .. } => {
                    let target = self
                        .program
                        .declarations
                        .get(&pattern.id)
                        .copied()
                        .map(Target::Variable);
                    self.bind(
                        pattern.tokens.start,
                        TokenClass::Variable,
                        DECLARATION,
                        target,
                    );
                }
                PatternKind::TupleVariantPattern { .. }
                | PatternKind::StructVariantPattern { .. } => {
                    let resolution = self.program.resolutions.get(&pattern.id).cloned();
                    self.classify_path(pattern.tokens.start, resolution.as_ref(), parameters);
                    if let Some(&last) = self.path_tokens(pattern.tokens.start).last() {
                        for index in self.named_members(last + 1) {
                            self.set(index, TokenClass::Property, 0);
                        }
                    }
                }
                _ => {}
            },
        }
    }

    // -------------------------------------------------------------------------
    // Queries
    // -------------------------------------------------------------------------

    pub fn semantic_tokens(&self) -> Vec<SemanticToken> {
        self.tokens
            .iter()
            .zip(&self.classes)
            .filter_map(|(token, class)| {
                let (class, modifiers) = (*class)?;
                Some(SemanticToken {
                    line: token.line,
                    column: token.col,
                    length: token_length(token),
                    class,
                    modifiers,
                })
            })
            .collect()
    }

    /// The token at `line`/`column`, including a cursor just after its end.
    pub fn token_at(&self, line: usize, column: usize) -> Option<usize> {
        let mut best = None;
        for (index, token) in self.tokens.iter().enumerate() {
            if token.line != line || token.col > column {
                continue;
            }
            let end = token.col + token_length(token);
            if column < end {
                return Some(index);
            }
            // A cursor just past an identifier still counts as on it
            if column == end && is_identifier(Some(token)) {
                best = Some(index);
            }
        }
        best
    }

    pub fn target_at(&self, line: usize, column: usize) -> Option<&Target> {
        self.targets.get(&self.token_at(line, column)?)
    }

    fn token_range(&self, index: usize) -> ((usize, usize), (usize, usize)) {
        let token = &self.tokens[index];
        (
            (token.line, token.col),
            (token.line, token.col + token_length(token)),
        )
    }

    /// Where the thing at `line`/`column` is declared.
    pub fn definition(
        &self,
        line: usize,
        column: usize,
    ) -> Option<((usize, usize), (usize, usize))> {
        let target = self.target_at(line, column)?;
        let index = *self.declarations.get(target)?;
        Some(self.token_range(index))
    }

    /// Every mention of the thing at `line`/`column`, including its
    /// declaration.
    pub fn references(&self, line: usize, column: usize) -> Vec<((usize, usize), (usize, usize))> {
        let Some(target) = self.target_at(line, column) else {
            return Vec::new();
        };
        let mut indices: Vec<_> = self
            .targets
            .iter()
            .filter(|(_, other)| *other == target)
            .map(|(&index, _)| index)
            .collect();
        indices.sort_unstable();
        indices
            .into_iter()
            .map(|index| self.token_range(index))
            .collect()
    }

    /// Whether the thing at `line`/`column` is declared in this document, so
    /// it can be renamed.
    pub fn can_rename(&self, line: usize, column: usize) -> bool {
        self.target_at(line, column)
            .is_some_and(|target| self.declarations.contains_key(target))
    }

    /// `DEFAULT_LIBRARY` for a function from the standard library.
    fn library_modifier(&self, function: usize) -> u32 {
        let module = self.program.functions[function].module.as_deref();
        if module.is_some_and(|module| module.starts_with("std_")) {
            DEFAULT_LIBRARY
        } else {
            0
        }
    }

    fn signature(&self, function: usize) -> String {
        let definition = &self.program.functions[function];
        let mut params: Vec<_> = definition
            .param_names
            .iter()
            .zip(&definition.params)
            .map(|(name, ty)| format!("{name}: {}", self.program.type_name(ty)))
            .collect();
        if definition.variadic {
            params.push("...".to_owned());
        }
        let prefix = if definition.c_name.is_some() {
            "extern \"C\" fn"
        } else {
            "fn"
        };
        let mut name = definition.path.clone();
        name.push(definition.name.clone());
        // Within functions, the path is theirs; only namespaces qualify names
        if definition.declaration != usize::MAX && definition.module.is_none() {
            name = vec![definition.name.clone()];
        }
        let mut text = format!("{prefix} {}({})", name.join("::"), params.join(", "));
        if let Some(return_type) = &definition.return_type {
            text.push_str(&format!(" -> {}", self.program.type_name(return_type)));
        }
        text
    }

    fn variant_text(&self, shape: &VariantShape) -> String {
        match shape {
            VariantShape::Unit => String::new(),
            VariantShape::Tuple(types) => {
                let types: Vec<_> = types.iter().map(|ty| self.program.type_name(ty)).collect();
                format!("({})", types.join(", "))
            }
            VariantShape::Struct(fields) => {
                let fields: Vec<_> = fields
                    .iter()
                    .map(|(name, ty)| format!("{name}: {}", self.program.type_name(ty)))
                    .collect();
                format!(" {{ {} }}", fields.join(", "))
            }
        }
    }

    fn describe(&self, target: &Target) -> String {
        let program = &self.program;
        match target {
            Target::Variable(variable) => {
                let info = &program.variables[*variable];
                let keyword = if info.constant { "const" } else { "let" };
                format!("{keyword} {}: {}", info.name, program.type_name(&info.ty))
            }
            Target::Function(function) => self.signature(*function),
            Target::Struct(id) => {
                let definition = &program.structs[*id];
                let fields: Vec<_> = definition
                    .fields
                    .iter()
                    .map(|(name, ty)| format!("    {name}: {},", program.type_name(ty)))
                    .collect();
                format!("struct {} {{\n{}\n}}", definition.name, fields.join("\n"))
            }
            Target::Enum(id) => {
                let definition = &program.enums[*id];
                let backing = definition
                    .backing_type
                    .as_ref()
                    .map_or(String::new(), |ty| format!(": {}", program.type_name(ty)));
                let variants: Vec<_> = definition
                    .variants
                    .iter()
                    .map(|(name, shape)| format!("    {name}{},", self.variant_text(shape)))
                    .collect();
                format!(
                    "enum {}{backing} {{\n{}\n}}",
                    definition.name,
                    variants.join("\n")
                )
            }
            Target::Variant(enum_id, index) => {
                let definition = &program.enums[*enum_id];
                let (name, shape) = &definition.variants[*index];
                format!(
                    "{}::{name}{} = {}",
                    definition.name,
                    self.variant_text(shape),
                    definition.discriminants[*index]
                )
            }
            Target::Field(struct_id, field) => {
                let definition = &program.structs[*struct_id];
                let ty = definition
                    .fields
                    .iter()
                    .find(|(name, _)| name == field)
                    .map_or("unknown".to_owned(), |(_, ty)| program.type_name(ty));
                format!("{}.{field}: {ty}", definition.name)
            }
            Target::Primitive(name) => format!("{name} (built-in type)"),
            Target::Module(name) => format!("module {name}"),
            Target::Namespace(path) => format!("namespace {path}"),
        }
    }

    /// Markdown describing what's at `line`/`column`: the declaration of a
    /// name, or the type of an expression.
    pub fn hover(&self, line: usize, column: usize) -> Option<String> {
        if let Some(target) = self.target_at(line, column) {
            let mut text = format!("```minlang\n{}\n```", self.describe(target));
            if let Target::Function(function) = target
                && let Some(doc) = &self.program.functions[*function].doc
            {
                text.push_str("\n\n");
                text.push_str(&doc_markdown(doc));
            }
            return Some(text);
        }

        // Otherwise the type of the smallest expression around the token
        let index = self.token_at(line, column)?;
        let mut best: Option<(usize, usize)> = None;
        walk(&self.ast, &mut |node| {
            if let Node::Expression(e) = node
                && e.tokens.contains(&index)
                && self.program.types.contains_key(&e.id)
                && best.is_none_or(|(_, size)| e.tokens.len() < size)
            {
                best = Some((e.id, e.tokens.len()));
            }
        });
        let ty = self.program.types.get(&best?.0)?;
        if matches!(ty, Type::Void | Type::Never | Type::Unknown) {
            return None;
        }
        Some(format!("```minlang\n{}\n```", self.program.type_name(ty)))
    }

    /// The document's outline: its namespaces, functions, structs, enums and
    /// globals.
    pub fn symbols(&self) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        for item in &self.ast {
            if let Some(symbol) = self.symbol(item) {
                symbols.push(symbol);
            }
        }
        symbols
    }

    fn symbol(&self, item: &crate::parser::Expression) -> Option<Symbol> {
        let range = (
            (item.position.start.line, item.position.start.col),
            (item.position.end.line, item.position.end.col),
        );
        let name_token = item.tokens.start + 1;
        if !is_identifier(self.tokens.get(name_token)) {
            return None;
        }
        let mut name = self.tokens[name_token].raw.clone();
        let selection = self.token_range(name_token);

        let member = |index: usize, kind| Symbol {
            name: self.tokens[index].raw.clone(),
            kind,
            range: self.token_range(index),
            selection: self.token_range(index),
            children: Vec::new(),
        };
        let declared = |kind: SymbolKind| -> Vec<Symbol> {
            self.declarations
                .iter()
                .filter(|&(target, &index)| {
                    item.tokens.contains(&index)
                        && matches!(
                            (target, kind),
                            (Target::Field(..), SymbolKind::Field)
                                | (Target::Variant(..), SymbolKind::EnumMember)
                        )
                })
                .map(|(_, &index)| index)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(|index| member(index, kind))
                .collect()
        };

        let (kind, children) = match &item.kind {
            ExpressionKind::NamespaceExpression { path, items, .. } => {
                name = path.join("::");
                let items = items.iter().filter_map(|item| self.symbol(item)).collect();
                (SymbolKind::Namespace, items)
            }
            ExpressionKind::FunctionDeclarationExpression { body, .. } => {
                let nested = body
                    .body
                    .iter()
                    .chain(body.result.as_deref())
                    .filter(|statement| {
                        matches!(
                            statement.kind,
                            ExpressionKind::FunctionDeclarationExpression { .. }
                                | ExpressionKind::StructDeclarationExpression { .. }
                                | ExpressionKind::EnumDeclarationExpression { .. }
                        )
                    })
                    .filter_map(|statement| self.symbol(statement))
                    .collect();
                (SymbolKind::Function, nested)
            }
            ExpressionKind::StructDeclarationExpression { .. } => {
                (SymbolKind::Struct, declared(SymbolKind::Field))
            }
            ExpressionKind::EnumDeclarationExpression { .. } => {
                (SymbolKind::Enum, declared(SymbolKind::EnumMember))
            }
            ExpressionKind::LetExpression { constant, .. } => (
                if *constant {
                    SymbolKind::Constant
                } else {
                    SymbolKind::Variable
                },
                Vec::new(),
            ),
            _ => return None,
        };

        Some(Symbol {
            name,
            kind,
            range,
            selection,
            children,
        })
    }

    /// Suggestions for the cursor at `line`/`column`: members after `Name::`
    /// or `value.`, otherwise keywords, types and the names in scope.
    pub fn completions(&self, line: usize, column: usize) -> Vec<Completion> {
        let before: Vec<usize> = self
            .tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| (token.line, token.col + token_length(token)) <= (line, column))
            .map(|(index, _)| index)
            .collect();
        let last = before.last().copied();
        // Typing a name: look at what precedes it
        let anchor = match last {
            Some(index)
                if is_identifier(self.tokens.get(index))
                    && (
                        self.tokens[index].line,
                        self.tokens[index].col + token_length(&self.tokens[index]),
                    ) == (line, column) =>
            {
                index.checked_sub(1)
            }
            other => other,
        };

        if let Some(anchor) = anchor {
            if is_symbol(self.tokens.get(anchor), Punctuation::Namespace) && anchor > 0 {
                // The whole path before the `::`, like `std::io`
                let mut start = anchor - 1;
                while start >= 2
                    && is_symbol(self.tokens.get(start - 1), Punctuation::Namespace)
                    && is_identifier(self.tokens.get(start - 2))
                {
                    start -= 2;
                }
                let path: Vec<_> = (start..anchor)
                    .step_by(2)
                    .map(|index| self.tokens[index].raw.as_str())
                    .collect();
                return self.namespace_members(&path.join("::"));
            }
            if is_symbol(self.tokens.get(anchor), Punctuation::Dot) && anchor > 0 {
                return self.fields_of(anchor - 1);
            }
        }

        let mut completions = Vec::new();
        for keyword in Keyword::ALL {
            completions.push(Completion {
                label: keyword.as_str().to_owned(),
                kind: CompletionKind::Keyword,
                detail: None,
            });
        }
        for primitive in module_members("types").unwrap_or_default() {
            completions.push(Completion {
                label: primitive.to_owned(),
                kind: CompletionKind::Type,
                detail: None,
            });
        }
        completions.push(Completion {
            label: "types".to_owned(),
            kind: CompletionKind::Module,
            detail: None,
        });
        for path in self.program.namespaces.keys() {
            if !path.contains("::") {
                completions.push(Completion {
                    label: path.clone(),
                    kind: CompletionKind::Module,
                    detail: Some(format!("namespace {path}")),
                });
            }
        }
        for (id, function) in self.program.functions.iter().enumerate() {
            // Those in another file's namespaces are reached through them
            if function.module.is_some() && !function.path.is_empty() {
                continue;
            }
            completions.push(Completion {
                label: function.name.clone(),
                kind: CompletionKind::Function,
                detail: Some(self.signature(id)),
            });
        }
        for definition in &self.program.structs {
            completions.push(Completion {
                label: definition.name.clone(),
                kind: CompletionKind::Type,
                detail: Some(format!("struct {}", definition.name)),
            });
        }
        for definition in &self.program.enums {
            completions.push(Completion {
                label: definition.name.clone(),
                kind: CompletionKind::Type,
                detail: Some(format!("enum {}", definition.name)),
            });
        }

        // Globals, and the variables of the function the cursor is in that
        // are declared before it
        let function = self.ast.iter().find(|item| {
            matches!(
                item.kind,
                ExpressionKind::FunctionDeclarationExpression { .. }
            ) && (item.position.start.line, item.position.start.col) <= (line, column)
                && (line, column) <= (item.position.end.line, item.position.end.col)
        });
        let mut seen = HashSet::new();
        for (id, info) in self.program.variables.iter().enumerate().rev() {
            let start = (info.position.start.line, info.position.start.col);
            let visible = info.global
                || function.is_some_and(|function| {
                    (function.position.start.line, function.position.start.col) <= start
                        && start <= (line, column)
                });
            if visible && seen.insert(info.name.clone()) {
                completions.push(Completion {
                    label: info.name.clone(),
                    kind: CompletionKind::Variable,
                    detail: Some(self.describe(&Target::Variable(id))),
                });
            }
        }
        completions
    }

    fn namespace_members(&self, name: &str) -> Vec<Completion> {
        if let Some(definition) = self
            .program
            .enums
            .iter()
            .find(|definition| definition.name == name)
        {
            return definition
                .variants
                .iter()
                .map(|(variant, shape)| Completion {
                    label: variant.clone(),
                    kind: CompletionKind::EnumMember,
                    detail: Some(format!("{name}::{variant}{}", self.variant_text(shape))),
                })
                .collect();
        }
        if let Some(members) = self.program.namespaces.get(name) {
            return members
                .iter()
                .map(|member| {
                    let path = format!("{name}::{member}");
                    let function = self.program.functions.iter().position(|function| {
                        function.name == *member && function.path.join("::") == name
                    });
                    match function {
                        Some(id) => Completion {
                            label: member.clone(),
                            kind: CompletionKind::Function,
                            detail: Some(self.signature(id)),
                        },
                        None if self.program.namespaces.contains_key(&path) => Completion {
                            label: member.clone(),
                            kind: CompletionKind::Module,
                            detail: Some(format!("namespace {path}")),
                        },
                        None => Completion {
                            label: member.clone(),
                            kind: CompletionKind::Type,
                            detail: None,
                        },
                    }
                })
                .collect();
        }
        module_members(name)
            .unwrap_or_default()
            .into_iter()
            .map(|member| Completion {
                label: member.to_owned(),
                kind: if is_primitive(member) {
                    CompletionKind::Type
                } else {
                    CompletionKind::Function
                },
                detail: None,
            })
            .collect()
    }

    /// The fields of the struct that the identifier at `index` holds.
    fn fields_of(&self, index: usize) -> Vec<Completion> {
        let mut ty = match self.targets.get(&index) {
            Some(Target::Variable(variable)) => Some(self.program.variables[*variable].ty.clone()),
            Some(Target::Field(struct_id, field)) => self.program.structs[*struct_id]
                .fields
                .iter()
                .find(|(name, _)| name == field)
                .map(|(_, ty)| ty.clone()),
            // While typing `value.` the statement doesn't parse, so find the
            // latest variable of that name declared before it
            _ => {
                let token = &self.tokens[index];
                self.program
                    .variables
                    .iter()
                    .filter(|info| {
                        info.name == token.raw
                            && (info.position.start.line, info.position.start.col)
                                < (token.line, token.col)
                    })
                    .max_by_key(|info| (info.position.start.line, info.position.start.col))
                    .map(|info| info.ty.clone())
            }
        };
        while let Some(Type::Reference(inner) | Type::Pointer(inner)) = ty {
            ty = Some(*inner);
        }
        let Some(Type::Struct(id)) = ty else {
            return Vec::new();
        };
        self.program.structs[id]
            .fields
            .iter()
            .map(|(name, ty)| Completion {
                label: name.clone(),
                kind: CompletionKind::Field,
                detail: Some(self.program.type_name(ty)),
            })
            .collect()
    }
}

/// A doc comment as Markdown. Tags like `@arg {size} the size` each get a
/// paragraph, with the name in code.
fn doc_markdown(doc: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut text = String::new();
    for line in doc.lines() {
        let line = line.trim();
        if let Some(tag) = line.strip_prefix('@') {
            if !text.is_empty() {
                paragraphs.push(std::mem::take(&mut text));
            }
            let (name, rest) = tag.split_once(' ').unwrap_or((tag, ""));
            let rest = rest.trim();
            let rest = match rest.strip_prefix('{').and_then(|rest| rest.split_once('}')) {
                Some((argument, rest)) => format!("`{argument}` {}", rest.trim()),
                None => rest.to_owned(),
            };
            paragraphs.push(format!("*@{name}* {rest}").trim_end().to_owned());
        } else if line.is_empty() {
            if !text.is_empty() {
                paragraphs.push(std::mem::take(&mut text));
            }
        } else {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(line);
        }
    }
    if !text.is_empty() {
        paragraphs.push(text);
    }
    paragraphs.join("\n\n")
}
