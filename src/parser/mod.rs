mod errors;
mod expression;
mod typeinfo;

pub use errors::*;
pub use expression::*;
pub use typeinfo::*;

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
        P::Plus => (B::Add, 8),
        P::SubNeg => (B::Sub, 8),
        P::Star => (B::Mul, 9),
        P::Div => (B::Div, 9),
        P::Mod => (B::Mod, 9),
        _ => return None,
    })
}

pub struct Parser {
    pub tokens: TokenStream,
    position: usize,
    pub errors: Vec<ParserError>,
    pub node: Ast,
    filename: Option<String>,
}

impl Parser {
    pub fn new(tokens: TokenStream, filename: Option<&str>) -> Self {
        Self {
            tokens,
            position: 0,
            errors: Vec::new(),
            node: Vec::new(),
            filename: filename.map(str::to_owned),
        }
    }

    pub fn parse(&mut self) -> Result<&Ast, &[ParserError]> {
        self.position = 0;
        self.errors.clear();
        self.node.clear();

        while self.peek_token().is_some() {
            if self.eat(Punctuation::Semi) {
                continue;
            }

            match self.parse_statement() {
                Ok(expression) => {
                    if self.eat(Punctuation::Semi) || expression.kind.is_block_like() {
                        self.node.push(expression);
                    } else {
                        let error = self.unexpected("';'");
                        self.errors.push(error);
                        self.recover(false);
                    }
                }
                Err(error) => {
                    self.errors.push(error);
                    self.recover(false);
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

    fn finish(&self, start: usize, kind: ExpressionKind) -> Expression {
        let first = &self.tokens[start];
        let last = &self.tokens[self.position - 1];
        Expression {
            kind,
            position: Location {
                start: Position {
                    line: first.line,
                    col: first.col,
                },
                end: Position {
                    line: last.line,
                    col: last.col + last.raw.chars().count(),
                },
            },
            tokens: start..self.position,
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

    /// Skips past the statement that failed to parse. Inside a block this stops
    /// before the block's closing `}`; at the top level it also stops after
    /// the `}` that closes a declaration's body.
    fn recover(&mut self, in_block: bool) {
        let mut depth = 0usize;

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
                    if depth == 0 && !in_block {
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
                    Keyword::Fn
                        | Keyword::Struct
                        | Keyword::Enum
                        | Keyword::If
                        | Keyword::Switch
                        | Keyword::For
                        | Keyword::Loop
                        | Keyword::Do
                )
            );

        if block_like {
            self.parse_primary()
        } else {
            self.parse_expression()
        }
    }

    fn parse_block(&mut self) -> ParseResult<Block> {
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

            let expression = match self.parse_statement() {
                Ok(expression) => expression,
                Err(error) => {
                    self.errors.push(error);
                    self.recover(true);
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
                self.recover(true);
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

        if self.eat(Punctuation::Assign) {
            let value = self.parse_expression()?;
            return Ok(self.finish(
                start,
                ExpressionKind::AssignmentExpression {
                    target: Box::new(target),
                    value: Box::new(value),
                },
            ));
        }

        Ok(target)
    }

    fn parse_binary(&mut self, min_precedence: u8) -> ParseResult<Expression> {
        let start = self.position;
        let mut left = self.parse_unary()?;

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
                let args = self.parse_list(Punctuation::RParen, Self::parse_expression)?;
                ExpressionKind::CallExpression {
                    callee: Box::new(expression),
                    args,
                }
            } else if self.eat(Punctuation::LBracket) {
                let index = self.parse_expression()?;
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

        let kind = match token.kind.clone() {
            TokenKind::Identifier { .. } => ExpressionKind::IdentifierExpression {
                name: self.parse_path()?,
            },
            TokenKind::StringLiteral { value } => {
                self.position += 1;
                ExpressionKind::StringLiteralExpression { value }
            }
            TokenKind::IntegerLiteral {
                base,
                size,
                signed,
                value,
            } => {
                self.position += 1;
                ExpressionKind::IntegerLiteralExpression {
                    base,
                    size,
                    signed,
                    value,
                }
            }
            TokenKind::FloatLiteral { value, size } => {
                self.position += 1;
                ExpressionKind::FloatLiteralExpression { value, size }
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
                let inner = self.parse_expression()?;
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
                    Keyword::Fn => self.parse_function()?,
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
                    Keyword::Return => self.parse_return()?,
                    Keyword::Break => ExpressionKind::BreakExpression,
                    Keyword::Continue => ExpressionKind::ContinueExpression,
                    Keyword::In
                    | Keyword::Case
                    | Keyword::Default
                    | Keyword::Else
                    | Keyword::While => {
                        self.position -= 1;
                        return Err(self.unexpected("expression"));
                    }
                }
            }
            TokenKind::Punctuation { .. } => return Err(self.unexpected("expression")),
        };

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
            name,
            params,
            return_type,
            body,
        })
    }

    fn parse_param(&mut self) -> ParseResult<Param> {
        let name = self.expect_identifier()?;
        self.expect(Punctuation::Colon)?;
        let r#type = self.parse_type(false)?;
        Ok(Param { name, r#type })
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
        Ok(ExpressionKind::StructDeclarationExpression { name, fields })
    }

    /// `enum Name { A, B }`
    fn parse_enum(&mut self) -> ParseResult<ExpressionKind> {
        let name = self.expect_identifier()?;
        self.expect(Punctuation::LBrace)?;
        let variants = self.parse_list(Punctuation::RBrace, Self::expect_identifier)?;
        Ok(ExpressionKind::EnumDeclarationExpression { name, variants })
    }

    /// `if (cond) { ... } else if (cond) { ... } else { ... }`
    fn parse_if(&mut self) -> ParseResult<ExpressionKind> {
        let condition = Box::new(self.parse_expression()?);
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
        let subject = Box::new(self.parse_expression()?);
        self.expect(Punctuation::LBrace)?;

        let mut cases = Vec::new();
        let mut default = None;

        while !self.eat(Punctuation::RBrace) {
            if self.eat_keyword(Keyword::Case) {
                let value = self.parse_expression()?;
                let body = self.parse_block()?;
                cases.push(Case { value, body });
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
        let parenthesized = self.eat(Punctuation::LParen);

        let binding = self.expect_identifier()?;
        self.expect_keyword(Keyword::In)?;
        let iterable = Box::new(self.parse_expression()?);

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

    /// Prefix `*`/`&` bind tighter than array suffixes, so `*u8[4]` is an array
    /// of four pointers; write `*(u8[4])` for a pointer to an array. An array
    /// initializer (`u8[4]{0}`) is only accepted where `allow_initializer` is
    /// set, since after a return type the `{` begins the function body.
    pub fn parse_type(&mut self, allow_initializer: bool) -> ParseResult<TypeInfo> {
        let mut typeinfo = self.parse_prefixed_type()?;

        while self.eat(Punctuation::LBracket) {
            let size = self.parse_array_size()?;
            self.expect(Punctuation::RBracket)?;

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
