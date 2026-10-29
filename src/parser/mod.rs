mod errors;
mod expression;
mod typeinfo;
pub mod visit;

pub use errors::*;
pub use expression::*;
pub use typeinfo::*;

use std::cell::Cell;
use std::ops::Range;

use crate::tokenizer::{Keyword, Punctuation, Token, TokenKind, TokenStream};

type ParseResult<T> = Result<T, ParserError>;

/// Binding power of each binary operator; higher binds tighter. Assignment is
/// handled separately since it is right-associative.
fn binary_operator(symbol: Punctuation) -> Option<(BinaryOperator, u8)> {
    use BinaryOperator as B;
    use Punctuation as P;

    Some(match symbol {
        P::LogicalOr => (B::LogicalOr, 1),
        P::LogicalAnd => (B::LogicalAnd, 2),
        P::Or => (B::Or, 3),
        P::Xor => (B::Xor, 4),
        P::And => (B::And, 5),
        P::Eq => (B::Eq, 6),
        P::Neq => (B::Neq, 6),
        P::Lt => (B::Lt, 7),
        P::Gt => (B::Gt, 7),
        P::Le => (B::Le, 7),
        P::Ge => (B::Ge, 7),
        P::Shl => (B::Shl, 8),
        P::Shr => (B::Shr, 8),
        P::Plus => (B::Add, 9),
        P::SubNeg => (B::Sub, 9),
        P::Star => (B::Mul, 10),
        P::Div => (B::Div, 10),
        P::Mod => (B::Mod, 10),
        _ => return None,
    })
}

/// `=` gives `Some(None)`; a compound assignment like `+=` gives the operator
/// it applies.
fn assignment_operator(symbol: Punctuation) -> Option<Option<BinaryOperator>> {
    use BinaryOperator as B;
    use Punctuation as P;

    Some(match symbol {
        P::Assign => None,
        P::PlusAssign => Some(B::Add),
        P::SubAssign => Some(B::Sub),
        P::MulAssign => Some(B::Mul),
        P::DivAssign => Some(B::Div),
        P::ModAssign => Some(B::Mod),
        P::AndAssign => Some(B::And),
        P::OrAssign => Some(B::Or),
        P::XorAssign => Some(B::Xor),
        P::ShlAssign => Some(B::Shl),
        P::ShrAssign => Some(B::Shr),
        _ => return None,
    })
}

/// A comment set aside from the token stream, with the index of the token
/// that follows it.
struct Comment {
    next: usize,
    token: Token,
}

pub struct Parser {
    /// The tokens to parse, without comments.
    pub tokens: TokenStream,
    /// In source order.
    comments: Vec<Comment>,
    position: usize,
    pub errors: Vec<ParserError>,
    pub node: Ast,
    filename: Option<String>,
    /// The id the next expression or pattern gets. A `Cell` so `finish` can
    /// stay `&self`.
    next_id: Cell<usize>,
    /// Set while parsing an expression that is directly followed by a block,
    /// such as an `if` condition, where `x {` must not start a struct literal.
    no_struct_literal: bool,
}

impl Parser {
    pub fn new(tokens: TokenStream, filename: Option<&str>) -> Self {
        let mut code = Vec::with_capacity(tokens.len());
        let mut comments = Vec::new();
        for token in tokens {
            match token.kind {
                TokenKind::Comment { .. } => comments.push(Comment {
                    next: code.len(),
                    token,
                }),
                _ => code.push(token),
            }
        }

        Self {
            tokens: code,
            comments,
            position: 0,
            errors: Vec::new(),
            node: Vec::new(),
            filename: filename.map(str::to_owned),
            no_struct_literal: false,
            next_id: Cell::new(0),
        }
    }

    pub fn parse(&mut self) -> Result<&Ast, &[ParserError]> {
        self.position = 0;
        self.errors.clear();
        self.node.clear();
        self.next_id.set(0);

        while self.peek_token().is_some() {
            if self.eat(Punctuation::Semi) {
                continue;
            }

            let start = self.position;

            match self.parse_statement() {
                Ok(expression) => {
                    if self.eat(Punctuation::Semi) || expression.kind.is_block_like() {
                        self.node.push(expression);
                    } else {
                        let error = self.unexpected("';'");
                        self.errors.push(error);
                        self.recover(false, start);
                    }
                }
                Err(error) => {
                    self.errors.push(error);
                    self.recover(false, start);
                }
            }
        }

        if !self.errors.is_empty() {
            return Err(&self.errors);
        }
        Ok(&self.node)
    }

    pub fn read_token(&mut self) -> Option<&Token> {
        let token = self.tokens.get(self.position)?;
        self.position += 1;
        Some(token)
    }

    pub fn peek_token(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    // -------------------------------------------------------------------------
    // Token helpers
    // -------------------------------------------------------------------------

    fn peek_symbol(&self) -> Option<Punctuation> {
        match self.peek_token()?.kind {
            TokenKind::Punctuation { symbol } => Some(symbol),
            _ => None,
        }
    }

    fn peek_keyword(&self) -> Option<Keyword> {
        match self.peek_token()?.kind {
            TokenKind::Keyword { keyword } => Some(keyword),
            _ => None,
        }
    }

    fn check(&self, symbol: Punctuation) -> bool {
        self.peek_symbol() == Some(symbol)
    }

    fn eat(&mut self, symbol: Punctuation) -> bool {
        let matched = self.check(symbol);
        if matched {
            self.position += 1;
        }
        matched
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> bool {
        let matched = self.peek_keyword() == Some(keyword);
        if matched {
            self.position += 1;
        }
        matched
    }

    fn expect(&mut self, symbol: Punctuation) -> ParseResult<()> {
        if self.eat(symbol) {
            return Ok(());
        }
        Err(self.unexpected(&format!("'{symbol}'")))
    }

    fn expect_keyword(&mut self, keyword: Keyword) -> ParseResult<()> {
        if self.eat_keyword(keyword) {
            return Ok(());
        }
        Err(self.unexpected(&format!("'{keyword}'")))
    }

    fn expect_identifier(&mut self) -> ParseResult<String> {
        if let Some(Token {
            kind: TokenKind::Identifier { name },
            ..
        }) = self.peek_token()
        {
            let name = name.clone();
            self.position += 1;
            return Ok(name);
        }
        Err(self.unexpected("identifier"))
    }

    /// Parses `ident (:: ident)*` into a single `a::b::c` name.
    fn parse_path(&mut self) -> ParseResult<String> {
        let mut name = self.expect_identifier()?;
        while self.eat(Punctuation::Namespace) {
            name.push_str("::");
            name.push_str(&self.expect_identifier()?);
        }
        Ok(name)
    }

    /// Parses `item, item, ... close`, allowing a trailing comma. The opening
    /// delimiter must already be consumed.
    fn parse_list<T>(
        &mut self,
        close: Punctuation,
        mut item: impl FnMut(&mut Self) -> ParseResult<T>,
    ) -> ParseResult<Vec<T>> {
        let mut items = Vec::new();
        loop {
            if self.eat(close) {
                break;
            }
            items.push(item(self)?);
            if !self.eat(Punctuation::Comma) {
                self.expect(close)?;
                break;
            }
        }
        Ok(items)
    }

    /// The source location and token range from `start` up to the last
    /// consumed token.
    fn span(&self, start: usize) -> (Location, Range<usize>) {
        let first = &self.tokens[start];
        let last = &self.tokens[self.position - 1];
        let position = Location {
            start: Position {
                line: first.line,
                col: first.col,
            },
            end: Position {
                line: last.line,
                col: last.col + last.raw.chars().count(),
            },
        };
        (position, start..self.position)
    }

    /// The `///` lines directly above the token at `index`, joined with
    /// newlines, with the space after each `///` dropped.
    fn doc_comment(&self, index: usize) -> Option<String> {
        let mut line = self.tokens.get(index)?.line;
        let end = self
            .comments
            .partition_point(|comment| comment.next <= index);

        let mut lines = Vec::new();
        for comment in self.comments[..end].iter().rev() {
            match &comment.token.kind {
                TokenKind::Comment { text, doc: true }
                    if comment.next == index && comment.token.line + 1 == line =>
                {
                    lines.push(text.strip_prefix(' ').unwrap_or(text));
                    line = comment.token.line;
                }
                _ => break,
            }
        }

        if lines.is_empty() {
            return None;
        }
        lines.reverse();
        Some(lines.join("\n"))
    }

    fn new_id(&self) -> usize {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    fn finish(&self, start: usize, kind: ExpressionKind) -> Expression {
        let (position, tokens) = self.span(start);
        Expression {
            kind,
            id: self.new_id(),
            position,
            tokens,
        }
    }

    fn finish_pattern(&self, start: usize, kind: PatternKind) -> Pattern {
        let (position, tokens) = self.span(start);
        Pattern {
            kind,
            id: self.new_id(),
            position,
            tokens,
        }
    }

    fn unexpected(&self, expected: &str) -> ParserError {
        let expected = expected.to_owned();
        let file = self.filename.as_deref();

        match self.peek_token() {
            Some(token) => ParserError::new(
                ParserErrorKind::UnexpectedToken {
                    expected,
                    actual: token.raw.clone(),
                },
                token.line,
                token.col,
                file,
            ),
            None => {
                let (line, column) = self.tokens.last().map_or((0, 0), |token| {
                    (token.line, token.col + token.raw.chars().count())
                });
                ParserError::new(ParserErrorKind::Eof { expected }, line, column, file)
            }
        }
    }

    /// Skips past the statement starting at token `start` that failed to
    /// parse. If the failure happened inside braces the statement opened (say,
    /// in a `case` pattern), this skips to their matching `}`. Otherwise it
    /// skips to the next `;`; inside a block it stops before the block's
    /// closing `}`, and at the top level after the `}` that closes a
    /// declaration's body.
    fn recover(&mut self, in_block: bool, start: usize) {
        let mut depth = self.tokens[start..self.position]
            .iter()
            .fold(0usize, |depth, token| match token.kind {
                TokenKind::Punctuation {
                    symbol: Punctuation::LBrace,
                } => depth + 1,
                TokenKind::Punctuation {
                    symbol: Punctuation::RBrace,
                } => depth.saturating_sub(1),
                _ => depth,
            });
        let inside_statement_braces = depth > 0;

        while let Some(symbol) = self.peek_token().map(|_| self.peek_symbol()) {
            match symbol {
                Some(Punctuation::Semi) if depth == 0 => {
                    self.position += 1;
                    return;
                }
                Some(Punctuation::LBrace) => depth += 1,
                Some(Punctuation::RBrace) => {
                    if depth == 0 {
                        if !in_block {
                            self.position += 1;
                        }
                        return;
                    }
                    depth -= 1;
                    if depth == 0 && (inside_statement_braces || !in_block) {
                        self.position += 1;
                        return;
                    }
                }
                _ => {}
            }
            self.position += 1;
        }
    }

    // -------------------------------------------------------------------------
    // Statements and blocks
    // -------------------------------------------------------------------------

    /// Like `parse_expression`, but a leading block-like construct is not
    /// continued as an operand, so `if (a) {} *p = 1;` is two statements.
    fn parse_statement(&mut self) -> ParseResult<Expression> {
        let block_like = self.check(Punctuation::LBrace)
            || matches!(
                self.peek_keyword(),
                Some(
                    Keyword::Pub
                        | Keyword::Namespace
                        | Keyword::Fn
                        | Keyword::Struct
                        | Keyword::Enum
                        | Keyword::If
                        | Keyword::Switch
                        | Keyword::For
                        | Keyword::Loop
                        | Keyword::While
                        | Keyword::Do
                )
            );

        if block_like {
            self.parse_primary()
        } else {
            self.parse_simple_statement()
        }
    }

    /// An expression, or `x++`/`x--`, which can only appear as statements.
    fn parse_simple_statement(&mut self) -> ParseResult<Expression> {
        let start = self.position;
        let expression = self.parse_expression()?;

        let is_place = matches!(
            expression.kind,
            ExpressionKind::IdentifierExpression { .. }
                | ExpressionKind::IndexExpression { .. }
                | ExpressionKind::MemberExpression { .. }
                | ExpressionKind::UnaryExpression {
                    operator: UnaryOperator::Deref,
                    ..
                }
        );
        if !is_place {
            return Ok(expression);
        }

        let decrement = match self.peek_symbol() {
            Some(Punctuation::Increment) => false,
            Some(Punctuation::Decrement) => true,
            _ => return Ok(expression),
        };
        self.position += 1;

        Ok(self.finish(
            start,
            ExpressionKind::IncrementExpression {
                target: Box::new(expression),
                decrement,
            },
        ))
    }

    /// Runs `parse` with struct literals allowed or forbidden, restoring the
    /// previous setting afterwards.
    fn with_struct_literals<T>(
        &mut self,
        allowed: bool,
        parse: impl FnOnce(&mut Self) -> ParseResult<T>,
    ) -> ParseResult<T> {
        let previous = std::mem::replace(&mut self.no_struct_literal, !allowed);
        let result = parse(self);
        self.no_struct_literal = previous;
        result
    }

    /// Parses an expression that is directly followed by a block.
    fn parse_condition(&mut self) -> ParseResult<Expression> {
        self.with_struct_literals(false, Self::parse_expression)
    }

    fn parse_block(&mut self) -> ParseResult<Block> {
        self.with_struct_literals(true, Self::parse_block_contents)
    }

    fn parse_block_contents(&mut self) -> ParseResult<Block> {
        self.expect(Punctuation::LBrace)?;

        let mut body = Vec::new();
        let mut result = None;

        loop {
            if self.eat(Punctuation::RBrace) {
                break;
            }
            if self.peek_token().is_none() {
                return Err(self.unexpected("'}'"));
            }
            if self.eat(Punctuation::Semi) {
                continue;
            }

            let start = self.position;

            let expression = match self.parse_statement() {
                Ok(expression) => expression,
                Err(error) => {
                    self.errors.push(error);
                    self.recover(true, start);
                    continue;
                }
            };

            if self.eat(Punctuation::Semi) {
                body.push(expression);
            } else if self.eat(Punctuation::RBrace) {
                result = Some(Box::new(expression));
                break;
            } else if expression.kind.is_block_like() {
                body.push(expression);
            } else {
                let error = self.unexpected("';' or '}'");
                self.errors.push(error);
                self.recover(true, start);
            }
        }

        Ok(Block { body, result })
    }

    // -------------------------------------------------------------------------
    // Expressions
    // -------------------------------------------------------------------------

    pub fn parse_expression(&mut self) -> ParseResult<Expression> {
        let start = self.position;
        let target = self.parse_binary(1)?;

        if let Some(operator) = self.peek_symbol().and_then(assignment_operator) {
            self.position += 1;
            let value = self.parse_expression()?;
            return Ok(self.finish(
                start,
                ExpressionKind::AssignmentExpression {
                    operator,
                    target: Box::new(target),
                    value: Box::new(value),
                },
            ));
        }

        Ok(target)
    }

    fn parse_binary(&mut self, min_precedence: u8) -> ParseResult<Expression> {
        let start = self.position;
        let mut left = self.parse_cast()?;

        while let Some((operator, precedence)) = self.peek_symbol().and_then(binary_operator) {
            if precedence < min_precedence {
                break;
            }
            self.position += 1;

            let right = self.parse_binary(precedence + 1)?;
            left = self.finish(
                start,
                ExpressionKind::BinaryExpression {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
            );
        }

        Ok(left)
    }

    /// `value as T`. Binds tighter than binary operators and looser than unary
    /// ones, so `-x as u8` casts `-x` and `a + b as u8` casts only `b`.
    fn parse_cast(&mut self) -> ParseResult<Expression> {
        let start = self.position;
        let mut expression = self.parse_unary()?;

        while self.eat_keyword(Keyword::As) {
            let typeinfo = self.parse_type(false)?;
            expression = self.finish(
                start,
                ExpressionKind::CastExpression {
                    value: Box::new(expression),
                    typeinfo,
                },
            );
        }

        Ok(expression)
    }

    fn parse_unary(&mut self) -> ParseResult<Expression> {
        let start = self.position;

        let operators: &[UnaryOperator] = match self.peek_symbol() {
            Some(Punctuation::SubNeg) => &[UnaryOperator::Neg],
            Some(Punctuation::Not) => &[UnaryOperator::Not],
            Some(Punctuation::Star) => &[UnaryOperator::Deref],
            Some(Punctuation::And) => &[UnaryOperator::AddressOf],
            // `&&x` is lexed as one token
            Some(Punctuation::LogicalAnd) => &[UnaryOperator::AddressOf, UnaryOperator::AddressOf],
            _ => return self.parse_postfix(),
        };
        self.position += 1;

        let mut expression = self.parse_unary()?;
        for &operator in operators {
            expression = self.finish(
                start,
                ExpressionKind::UnaryExpression {
                    operator,
                    operand: Box::new(expression),
                },
            );
        }

        Ok(expression)
    }

    fn parse_postfix(&mut self) -> ParseResult<Expression> {
        let start = self.position;
        let mut expression = self.parse_primary()?;

        loop {
            let kind = if self.eat(Punctuation::LParen) {
                let args = self.with_struct_literals(true, |parser| {
                    parser.parse_list(Punctuation::RParen, Self::parse_expression)
                })?;
                ExpressionKind::CallExpression {
                    callee: Box::new(expression),
                    args,
                }
            } else if self.eat(Punctuation::LBracket) {
                let index = self.with_struct_literals(true, Self::parse_expression)?;
                self.expect(Punctuation::RBracket)?;
                ExpressionKind::IndexExpression {
                    target: Box::new(expression),
                    index: Box::new(index),
                }
            } else if self.eat(Punctuation::Dot) {
                let member = self.expect_identifier()?;
                ExpressionKind::MemberExpression {
                    target: Box::new(expression),
                    member,
                }
            } else {
                break;
            };

            expression = self.finish(start, kind);
        }

        Ok(expression)
    }

    fn parse_primary(&mut self) -> ParseResult<Expression> {
        let start = self.position;

        let Some(token) = self.peek_token() else {
            return Err(self.unexpected("expression"));
        };

        let mut kind = match token.kind.clone() {
            TokenKind::Identifier { .. } => {
                let name = self.parse_path()?;

                if !self.no_struct_literal && self.eat(Punctuation::LBrace) {
                    let fields = self.with_struct_literals(true, |parser| {
                        parser.parse_list(Punctuation::RBrace, Self::parse_field_init)
                    })?;
                    ExpressionKind::StructLiteralExpression { name, fields }
                } else {
                    ExpressionKind::IdentifierExpression { name }
                }
            }
            TokenKind::StringLiteral { value } => {
                self.position += 1;
                ExpressionKind::StringLiteralExpression { value }
            }
            TokenKind::IntegerLiteral {
                base,
                size,
                signed,
                value,
                suffixed,
            } => {
                self.position += 1;
                ExpressionKind::IntegerLiteralExpression {
                    base,
                    size,
                    signed,
                    value,
                    suffixed,
                }
            }
            TokenKind::FloatLiteral {
                value,
                size,
                suffixed,
            } => {
                self.position += 1;
                ExpressionKind::FloatLiteralExpression {
                    value,
                    size,
                    suffixed,
                }
            }
            TokenKind::BooleanLiteral { value } => {
                self.position += 1;
                ExpressionKind::BooleanLiteralExpression { value }
            }
            TokenKind::Punctuation {
                symbol: Punctuation::At,
            } => {
                self.position += 1;
                ExpressionKind::AtExpression
            }
            TokenKind::Punctuation {
                symbol: Punctuation::LParen,
            } => {
                self.position += 1;
                let inner = self.with_struct_literals(true, Self::parse_expression)?;
                self.expect(Punctuation::RParen)?;
                return Ok(inner);
            }
            TokenKind::Punctuation {
                symbol: Punctuation::LBrace,
            } => ExpressionKind::BlockExpression {
                block: self.parse_block()?,
            },
            TokenKind::Keyword { keyword } => {
                self.position += 1;
                match keyword {
                    Keyword::Import => self.parse_import()?,
                    Keyword::Pub => self.parse_public()?,
                    Keyword::Namespace => self.parse_namespace()?,
                    Keyword::Fn => self.parse_function()?,
                    Keyword::Extern => self.parse_extern()?,
                    Keyword::Let => self.parse_let(false)?,
                    Keyword::Const => self.parse_let(true)?,
                    Keyword::Struct => self.parse_struct()?,
                    Keyword::Enum => self.parse_enum()?,
                    Keyword::If => self.parse_if()?,
                    Keyword::Switch => self.parse_switch()?,
                    Keyword::For => self.parse_for()?,
                    Keyword::Loop => ExpressionKind::LoopExpression {
                        body: self.parse_block()?,
                    },
                    Keyword::Do => self.parse_do()?,
                    Keyword::While => ExpressionKind::WhileExpression {
                        condition: Box::new(self.parse_condition()?),
                        body: self.parse_block()?,
                    },
                    Keyword::Return => self.parse_return()?,
                    Keyword::Yield => ExpressionKind::YieldExpression {
                        value: Box::new(self.parse_expression()?),
                    },
                    Keyword::Break => ExpressionKind::BreakExpression,
                    Keyword::Continue => ExpressionKind::ContinueExpression,
                    Keyword::In
                    | Keyword::Case
                    | Keyword::Default
                    | Keyword::Else
                    | Keyword::Iter
                    | Keyword::As => {
                        self.position -= 1;
                        return Err(self.unexpected("expression"));
                    }
                }
            }
            TokenKind::Punctuation { .. } => return Err(self.unexpected("expression")),
            TokenKind::Comment { .. } => unreachable!("comments are set aside by Parser::new"),
        };

        match &mut kind {
            ExpressionKind::FunctionDeclarationExpression { doc, .. }
            | ExpressionKind::ExternFunctionDeclarationExpression { doc, .. }
            | ExpressionKind::LetExpression { doc, .. }
            | ExpressionKind::StructDeclarationExpression { doc, .. }
            | ExpressionKind::EnumDeclarationExpression { doc, .. }
            | ExpressionKind::NamespaceExpression { doc, .. } => {
                *doc = self.doc_comment(start);
            }
            _ => {}
        }

        Ok(self.finish(start, kind))
    }

    // -------------------------------------------------------------------------
    // Keyword constructs. Each is called with its keyword already consumed.
    // -------------------------------------------------------------------------

    /// `import a::b::c`, `import a::{b, c}`, `import a::*`
    fn parse_import(&mut self) -> ParseResult<ExpressionKind> {
        let mut module = Vec::new();

        let imports = loop {
            if self.eat(Punctuation::LBrace) {
                break self.parse_list(Punctuation::RBrace, |parser| {
                    if parser.eat(Punctuation::Star) {
                        return Ok("*".to_owned());
                    }
                    parser.expect_identifier()
                })?;
            }
            if self.eat(Punctuation::Star) {
                break vec!["*".to_owned()];
            }

            module.push(self.expect_identifier()?);

            if !self.eat(Punctuation::Namespace) {
                break module.pop().into_iter().collect();
            }
        };

        Ok(ExpressionKind::ImportExpression { module, imports })
    }

    /// `namespace a::b { ... }`, whose items are parsed like the top level's.
    fn parse_namespace(&mut self) -> ParseResult<ExpressionKind> {
        let mut path = vec![self.expect_identifier()?];
        while self.eat(Punctuation::Namespace) {
            path.push(self.expect_identifier()?);
        }

        let Block { mut body, result } = self.parse_block()?;
        body.extend(result.map(|result| *result));

        Ok(ExpressionKind::NamespaceExpression {
            doc: None,
            path,
            items: body,
        })
    }

    /// `pub` followed by a `fn`, `let`, `const`, `struct` or `enum`.
    fn parse_public(&mut self) -> ParseResult<ExpressionKind> {
        let mut kind = match self.peek_keyword() {
            Some(Keyword::Fn) => {
                self.position += 1;
                self.parse_function()?
            }
            Some(Keyword::Extern) => {
                self.position += 1;
                self.parse_extern()?
            }
            Some(Keyword::Let) => {
                self.position += 1;
                self.parse_let(false)?
            }
            Some(Keyword::Const) => {
                self.position += 1;
                self.parse_let(true)?
            }
            Some(Keyword::Struct) => {
                self.position += 1;
                self.parse_struct()?
            }
            Some(Keyword::Enum) => {
                self.position += 1;
                self.parse_enum()?
            }
            _ => {
                return Err(self.unexpected("'fn', 'extern', 'let', 'const', 'struct' or 'enum'"));
            }
        };
        match &mut kind {
            ExpressionKind::FunctionDeclarationExpression { public, .. }
            | ExpressionKind::ExternFunctionDeclarationExpression { public, .. }
            | ExpressionKind::LetExpression { public, .. }
            | ExpressionKind::StructDeclarationExpression { public, .. }
            | ExpressionKind::EnumDeclarationExpression { public, .. } => *public = true,
            _ => {}
        }
        Ok(kind)
    }

    /// `fn name(a: T, b: U) -> R { ... }`
    fn parse_function(&mut self) -> ParseResult<ExpressionKind> {
        let name = self.expect_identifier()?;

        self.expect(Punctuation::LParen)?;
        let params = self.parse_list(Punctuation::RParen, Self::parse_param)?;

        let return_type = if self.eat(Punctuation::Point) {
            Some(self.parse_type(false)?)
        } else {
            None
        };

        let body = self.parse_block()?;

        Ok(ExpressionKind::FunctionDeclarationExpression {
            public: false,
            doc: None,
            name,
            params,
            return_type,
            body,
        })
    }

    /// `extern "C" fn name(a: T, ...) -> R`, without a body.
    fn parse_extern(&mut self) -> ParseResult<ExpressionKind> {
        let abi = match self.peek_token() {
            Some(Token {
                kind: TokenKind::StringLiteral { value },
                ..
            }) => value.clone(),
            _ => return Err(self.unexpected("an ABI string like \"C\"")),
        };
        self.position += 1;
        self.expect_keyword(Keyword::Fn)?;
        let name = self.expect_identifier()?;

        self.expect(Punctuation::LParen)?;
        let mut params = Vec::new();
        let mut variadic = false;
        while !self.eat(Punctuation::RParen) {
            // `...` can only come last
            if self.eat(Punctuation::Ellipsis) {
                variadic = true;
                self.eat(Punctuation::Comma);
                self.expect(Punctuation::RParen)?;
                break;
            }
            params.push(self.parse_param()?);
            if !self.eat(Punctuation::Comma) {
                self.expect(Punctuation::RParen)?;
                break;
            }
        }

        let return_type = if self.eat(Punctuation::Point) {
            Some(self.parse_type(false)?)
        } else {
            None
        };

        Ok(ExpressionKind::ExternFunctionDeclarationExpression {
            public: false,
            doc: None,
            abi,
            name,
            params,
            variadic,
            return_type,
        })
    }

    fn parse_param(&mut self) -> ParseResult<Param> {
        let doc = self.doc_comment(self.position);
        let name = self.expect_identifier()?;
        self.expect(Punctuation::Colon)?;
        let r#type = self.parse_type(false)?;
        Ok(Param { name, r#type, doc })
    }

    /// `let name: T = value`, with both the type and value optional.
    fn parse_let(&mut self, constant: bool) -> ParseResult<ExpressionKind> {
        let name = self.expect_identifier()?;

        let typeinfo = if self.eat(Punctuation::Colon) {
            Some(self.parse_type(true)?)
        } else {
            None
        };

        let value = if self.eat(Punctuation::Assign) {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        Ok(ExpressionKind::LetExpression {
            public: false,
            doc: None,
            constant,
            name,
            typeinfo,
            value,
        })
    }

    /// `struct Name { a: T, b: U }`
    fn parse_struct(&mut self) -> ParseResult<ExpressionKind> {
        let name = self.expect_identifier()?;
        self.expect(Punctuation::LBrace)?;
        let fields = self.parse_list(Punctuation::RBrace, Self::parse_param)?;
        Ok(ExpressionKind::StructDeclarationExpression {
            public: false,
            doc: None,
            name,
            fields,
        })
    }

    fn parse_field_init(&mut self) -> ParseResult<FieldInit> {
        let name = self.expect_identifier()?;
        self.expect(Punctuation::Colon)?;
        let value = self.parse_expression()?;
        Ok(FieldInit { name, value })
    }

    /// `enum Name { A, B }` or `enum Name: u8 { A = 1, B }`
    fn parse_enum(&mut self) -> ParseResult<ExpressionKind> {
        let name = self.expect_identifier()?;

        let backing_type = if self.eat(Punctuation::Colon) {
            Some(self.parse_type(false)?)
        } else {
            None
        };

        self.expect(Punctuation::LBrace)?;
        let variants = self.parse_list(Punctuation::RBrace, Self::parse_enum_variant)?;

        Ok(ExpressionKind::EnumDeclarationExpression {
            public: false,
            doc: None,
            name,
            backing_type,
            variants,
        })
    }

    fn parse_enum_variant(&mut self) -> ParseResult<EnumVariant> {
        let doc = self.doc_comment(self.position);
        let name = self.expect_identifier()?;

        let fields = if self.eat(Punctuation::LParen) {
            VariantFields::Tuple {
                types: self.parse_list(Punctuation::RParen, |parser| parser.parse_type(false))?,
            }
        } else if self.eat(Punctuation::LBrace) {
            VariantFields::Struct {
                fields: self.parse_list(Punctuation::RBrace, Self::parse_param)?,
            }
        } else {
            VariantFields::Unit
        };

        let value = if self.eat(Punctuation::Assign) {
            Some(self.parse_expression()?)
        } else {
            None
        };

        Ok(EnumVariant {
            name,
            doc,
            fields,
            value,
        })
    }

    /// `if (cond) { ... } else if (cond) { ... } else { ... }`
    fn parse_if(&mut self) -> ParseResult<ExpressionKind> {
        let condition = Box::new(self.parse_condition()?);
        let then = self.parse_block()?;

        let otherwise = if self.eat_keyword(Keyword::Else) {
            if self.peek_keyword() == Some(Keyword::If) {
                Some(Box::new(self.parse_primary()?))
            } else {
                let start = self.position;
                let block = self.parse_block()?;
                Some(Box::new(
                    self.finish(start, ExpressionKind::BlockExpression { block }),
                ))
            }
        } else {
            None
        };

        Ok(ExpressionKind::IfExpression {
            condition,
            then,
            otherwise,
        })
    }

    /// `switch (x) { case (a) { ... } default { ... } }`
    fn parse_switch(&mut self) -> ParseResult<ExpressionKind> {
        let subject = Box::new(self.parse_condition()?);
        self.expect(Punctuation::LBrace)?;

        let mut cases = Vec::new();
        let mut default = None;

        while !self.eat(Punctuation::RBrace) {
            if self.eat_keyword(Keyword::Case) {
                let pattern =
                    self.with_struct_literals(false, |parser| parser.parse_pattern(false))?;
                let body = self.parse_block()?;
                cases.push(Case { pattern, body });
            } else if self.eat_keyword(Keyword::Default) {
                default = Some(self.parse_block()?);
            } else {
                return Err(self.unexpected("'case', 'default' or '}'"));
            }
        }

        Ok(ExpressionKind::SwitchExpression {
            subject,
            cases,
            default,
        })
    }

    // -------------------------------------------------------------------------
    // Patterns
    // -------------------------------------------------------------------------

    /// `binds` is set inside variant patterns, where a bare name binds part of
    /// the matched value rather than naming a constant to compare against.
    fn parse_pattern(&mut self, binds: bool) -> ParseResult<Pattern> {
        let start = self.position;
        let kind = self.parse_pattern_kind(binds)?;
        Ok(self.finish_pattern(start, kind))
    }

    fn parse_pattern_kind(&mut self, binds: bool) -> ParseResult<PatternKind> {
        if self.eat(Punctuation::LParen) {
            let inner = self.with_struct_literals(true, |parser| parser.parse_pattern(binds))?;
            self.expect(Punctuation::RParen)?;
            return Ok(inner.kind);
        }

        let Some(TokenKind::Identifier { name }) = self.peek_token().map(|token| &token.kind)
        else {
            return Ok(PatternKind::ValuePattern {
                value: self.parse_expression()?,
            });
        };

        if name == "_" {
            self.position += 1;
            return Ok(PatternKind::WildcardPattern);
        }

        let start = self.position;
        let path = self.parse_path()?;

        if self.eat(Punctuation::LParen) {
            let elements = self.with_struct_literals(true, |parser| {
                parser.parse_list(Punctuation::RParen, |parser| parser.parse_pattern(true))
            })?;
            return Ok(PatternKind::TupleVariantPattern { path, elements });
        }

        // At the top of a `case`, `Path {` may instead be the case body
        if self.check(Punctuation::LBrace)
            && (!self.no_struct_literal || self.brace_followed_by_block())
        {
            self.position += 1;
            let fields = self.with_struct_literals(true, |parser| {
                parser.parse_list(Punctuation::RBrace, Self::parse_field_pattern)
            })?;
            return Ok(PatternKind::StructVariantPattern { path, fields });
        }

        if binds && !path.contains("::") {
            return Ok(PatternKind::BindingPattern { name: path });
        }

        // A constant, possibly part of a larger expression: `LIMIT + 1`
        self.position = start;
        Ok(PatternKind::ValuePattern {
            value: self.parse_expression()?,
        })
    }

    fn parse_field_pattern(&mut self) -> ParseResult<FieldPattern> {
        let start = self.position;
        let name = self.expect_identifier()?;

        let pattern = if self.eat(Punctuation::Colon) {
            self.parse_pattern(true)?
        } else {
            self.finish_pattern(start, PatternKind::BindingPattern { name: name.clone() })
        };

        Ok(FieldPattern { name, pattern })
    }

    /// With the current token a `{`, whether its matching `}` is directly
    /// followed by another `{`.
    fn brace_followed_by_block(&self) -> bool {
        let is = |token: Option<&Token>, symbol| matches!(token, Some(Token { kind: TokenKind::Punctuation { symbol: s }, .. }) if *s == symbol);

        let mut depth = 0usize;
        for index in self.position..self.tokens.len() {
            let token = self.tokens.get(index);
            if is(token, Punctuation::LBrace) {
                depth += 1;
            } else if is(token, Punctuation::RBrace) {
                depth -= 1;
                if depth == 0 {
                    return is(self.tokens.get(index + 1), Punctuation::LBrace);
                }
            }
        }
        false
    }

    /// `do { ... }` or `do { ... } while (cond)`
    fn parse_do(&mut self) -> ParseResult<ExpressionKind> {
        let body = self.parse_block()?;

        let condition = if self.eat_keyword(Keyword::While) {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        Ok(ExpressionKind::DoExpression { body, condition })
    }

    /// `for x in xs { ... }` or `for (x in xs) { ... }`
    fn parse_for(&mut self) -> ParseResult<ExpressionKind> {
        // `for (x in xs)` names a binding then `in`; anything else in
        // parentheses is the clause form
        let is_clause = self.check(Punctuation::LParen)
            && !(matches!(
                self.tokens.get(self.position + 1).map(|token| &token.kind),
                Some(TokenKind::Identifier { .. })
            ) && matches!(
                self.tokens.get(self.position + 2).map(|token| &token.kind),
                Some(TokenKind::Keyword {
                    keyword: Keyword::In
                })
            ));

        if is_clause {
            self.position += 1;
            return self.with_struct_literals(true, Self::parse_for_clauses);
        }

        let parenthesized = self.eat(Punctuation::LParen);

        let binding = self.expect_identifier()?;
        self.expect_keyword(Keyword::In)?;
        let iterable = Box::new(self.parse_condition()?);

        if parenthesized {
            self.expect(Punctuation::RParen)?;
        }

        let body = self.parse_block()?;

        Ok(ExpressionKind::ForExpression {
            binding,
            iterable,
            body,
        })
    }

    /// `for (init; condition; step; let item = ...) { ... }`, after the `(`.
    fn parse_for_clauses(&mut self) -> ParseResult<ExpressionKind> {
        let init = if self.check(Punctuation::Semi) {
            None
        } else {
            Some(Box::new(self.parse_simple_statement()?))
        };
        self.expect(Punctuation::Semi)?;

        let condition = if self.check(Punctuation::Semi) {
            None
        } else {
            Some(Box::new(self.parse_expression()?))
        };
        self.expect(Punctuation::Semi)?;

        let step = if matches!(
            self.peek_symbol(),
            Some(Punctuation::RParen | Punctuation::Semi)
        ) {
            None
        } else {
            Some(Box::new(self.parse_simple_statement()?))
        };

        let each = if self.eat(Punctuation::Semi) {
            if self.peek_keyword() != Some(Keyword::Let) {
                return Err(self.unexpected("'let'"));
            }
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        self.expect(Punctuation::RParen)?;
        let body = self.parse_block()?;

        Ok(ExpressionKind::ForClauseExpression {
            init,
            condition,
            step,
            each,
            body,
        })
    }

    /// `return` or `return value`
    fn parse_return(&mut self) -> ParseResult<ExpressionKind> {
        let ends_statement = matches!(
            self.peek_symbol(),
            Some(Punctuation::Semi | Punctuation::RBrace)
        ) || self.peek_token().is_none();

        let value = if ends_statement {
            None
        } else {
            Some(Box::new(self.parse_expression()?))
        };

        Ok(ExpressionKind::ReturnExpression { value })
    }

    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    /// Array dimensions read as in C: `u8[4][8]` is 4 elements of `u8[8]`, so
    /// the leftmost suffix is the outermost array. Prefix `*`/`&` bind tighter
    /// than array suffixes, so `*u8[4]` is an array of four pointers; write
    /// `*(u8[4])` for a pointer to an array. An array initializer (`u8[4]{0}`)
    /// is only accepted where `allow_initializer` is set, since after a return
    /// type the `{` begins the function body.
    pub fn parse_type(&mut self, allow_initializer: bool) -> ParseResult<TypeInfo> {
        let mut typeinfo = self.parse_prefixed_type()?;

        let mut sizes = Vec::new();
        while self.eat(Punctuation::LBracket) {
            sizes.push(self.parse_array_size()?);
            self.expect(Punctuation::RBracket)?;
        }

        for size in sizes.into_iter().rev() {
            typeinfo = TypeInfo::ArrayType {
                element: Box::new(typeinfo),
                size,
                initializer: None,
            };
        }

        if let TypeInfo::ArrayType { initializer, .. } = &mut typeinfo
            && allow_initializer
            && self.check(Punctuation::LBrace)
        {
            *initializer = Some(Box::new(self.parse_primary()?));
        }

        Ok(typeinfo)
    }

    fn parse_prefixed_type(&mut self) -> ParseResult<TypeInfo> {
        let pointer = |pointee| TypeInfo::PointerType {
            pointee: Box::new(pointee),
        };
        let reference = |referent| TypeInfo::ReferenceType {
            referent: Box::new(referent),
        };

        if self.eat_keyword(Keyword::Iter) {
            return Ok(TypeInfo::IteratorType {
                item: Box::new(self.parse_type(false)?),
            });
        }
        if self.eat(Punctuation::Star) {
            return Ok(pointer(self.parse_prefixed_type()?));
        }
        if self.eat(Punctuation::And) {
            return Ok(reference(self.parse_prefixed_type()?));
        }
        // `&&T` is lexed as one token
        if self.eat(Punctuation::LogicalAnd) {
            return Ok(reference(reference(self.parse_prefixed_type()?)));
        }
        if self.eat(Punctuation::LParen) {
            let inner = self.parse_type(false)?;
            self.expect(Punctuation::RParen)?;
            return Ok(inner);
        }

        if !matches!(
            self.peek_token(),
            Some(Token {
                kind: TokenKind::Identifier { .. },
                ..
            })
        ) {
            return Err(self.unexpected("type"));
        }

        Ok(TypeInfo::NamedType {
            name: self.parse_path()?,
        })
    }

    /// `?` (unknown size) or an integer literal.
    fn parse_array_size(&mut self) -> ParseResult<Option<u64>> {
        if self.eat(Punctuation::Question) {
            return Ok(None);
        }

        let Some(token) = self.peek_token() else {
            return Err(self.unexpected("array size"));
        };

        let TokenKind::IntegerLiteral { value, .. } = &token.kind else {
            return Err(self.unexpected("array size"));
        };

        let Ok(size) = u64::try_from(value) else {
            return Err(ParserError::new(
                ParserErrorKind::InvalidArraySize(token.raw.clone()),
                token.line,
                token.col,
                self.filename.as_deref(),
            ));
        };

        self.position += 1;
        Ok(Some(size))
    }
}
