mod errors;
mod tokens;
mod utils;

pub use errors::*;
pub use tokens::*;
pub use utils::*;

use num_bigint::BigUint;

fn integer_suffix(suffix: &str) -> Option<IntegerSize> {
    match suffix {
        "u8" | "s8" => Some(8),
        "u16" | "s16" => Some(16),
        "u32" | "s32" => Some(32),
        "u64" | "s64" => Some(64),
        _ => None,
    }
}

fn float_suffix(suffix: &str) -> Option<FloatSize> {
    match suffix {
        "f32" => Some(32),
        "f64" => Some(64),
        _ => None,
    }
}

fn slice(source: &[char], start: usize, end: usize) -> String {
    source[start..end].iter().collect()
}

pub struct Tokenizer {
    contents: String,
    file: String,
    tokens: TokenStream,
    errors: Vec<TokenizerError>,
}

impl Tokenizer {
    pub fn new(contents: impl Into<String>, file: Option<&str>) -> Self {
        Self {
            contents: contents.into(),
            file: file.unwrap_or("stdin").to_owned(),
            tokens: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn tokenize(&mut self) -> Result<TokenStream, Vec<TokenizerError>> {
        self.tokens = Vec::new();
        self.errors = Vec::new();

        let contents = std::mem::take(&mut self.contents);

        for (line, text) in contents.split('\n').enumerate() {
            let text = text.strip_suffix('\r').unwrap_or(text);
            let source: Vec<char> = text.chars().collect();
            self.tokenize_line(&source, line);
        }

        self.contents = contents;

        if !self.errors.is_empty() {
            return Err(std::mem::take(&mut self.errors));
        }

        Ok(std::mem::take(&mut self.tokens))
    }

    fn tokenize_line(&mut self, source: &[char], line: usize) {
        let mut col = 0;

        while col < source.len() {
            let char = source[col];

            if char.is_whitespace() {
                col += 1;
                continue;
            }

            if char == '/' && source.get(col + 1) == Some(&'/') {
                break;
            }

            if char.is_ascii_alphabetic() || char == '_' {
                let start = col;

                col += 1;

                while col < source.len()
                    && (source[col].is_ascii_alphanumeric() || source[col] == '_')
                {
                    col += 1;
                }

                let raw = slice(source, start, col);

                let kind = match raw.as_str() {
                    "true" => TokenKind::BooleanLiteral { value: true },
                    "false" => TokenKind::BooleanLiteral { value: false },
                    _ => match Keyword::lookup(&raw) {
                        Some(keyword) => TokenKind::Keyword { keyword },
                        None => TokenKind::Identifier { name: raw.clone() },
                    },
                };

                self.push(kind, line, start, raw);
                continue;
            }

            if char.is_ascii_digit() {
                col = self.tokenize_number(source, line, col);
                continue;
            }

            if char == '"' || char == '\'' {
                col = self.tokenize_string(source, line, col);
                continue;
            }

            // Longest match first, so `<<=` wins over `<<` and `<`
            let punctuation = (1..=3).rev().find_map(|length| {
                let raw = slice(source, col, (col + length).min(source.len()));
                Punctuation::ALL
                    .iter()
                    .copied()
                    .find(|value| value.as_str() == raw)
                    .map(|symbol| (symbol, raw))
            });

            if let Some((symbol, raw)) = punctuation {
                let length = raw.len();
                self.push(TokenKind::Punctuation { symbol }, line, col, raw);
                col += length;
                continue;
            }

            self.error(TokenizerErrorKind::UnexpectedCharacter(char), line, col);

            col += 1;
        }
    }

    fn push(&mut self, kind: TokenKind, line: usize, col: usize, raw: String) {
        self.tokens.push(Token {
            kind,
            line,
            col,
            raw,
        });
    }

    fn error(&mut self, kind: TokenizerErrorKind, line: usize, column: usize) {
        self.errors
            .push(TokenizerError::new(kind, line, column, Some(&self.file)));
    }

    fn invalid_numeric(&mut self, message: String, line: usize, column: usize) {
        self.error(
            TokenizerErrorKind::InvalidNumericLiteral(message),
            line,
            column,
        );
    }

    fn tokenize_number(&mut self, source: &[char], line: usize, start: usize) -> usize {
        let mut col = start;

        if source[col] == '0' {
            match source.get(col + 1).map(char::to_ascii_lowercase) {
                Some('x') => {
                    return self.tokenize_prefixed_integer(
                        source,
                        line,
                        start,
                        16,
                        |c| c.is_ascii_hexdigit(),
                        "0x",
                    );
                }
                Some('b') => {
                    return self.tokenize_prefixed_integer(
                        source,
                        line,
                        start,
                        2,
                        |c| matches!(c, '0' | '1'),
                        "0b",
                    );
                }
                Some('o') => {
                    return self.tokenize_prefixed_integer(
                        source,
                        line,
                        start,
                        8,
                        |c| matches!(c, '0'..='7'),
                        "0o",
                    );
                }
                _ => {}
            }
        }

        let is_digit = |col: usize| source.get(col).is_some_and(char::is_ascii_digit);

        while is_digit(col) {
            col += 1;
        }

        let mut is_float = false;

        if source.get(col) == Some(&'.') {
            is_float = true;
            col += 1;

            while is_digit(col) {
                col += 1;
            }
        }

        if source.get(col).map(char::to_ascii_lowercase) == Some('e') {
            is_float = true;
            col += 1;

            if matches!(source.get(col), Some('+' | '-')) {
                col += 1;
            }

            let exponent_start = col;

            while is_digit(col) {
                col += 1;
            }

            if col == exponent_start {
                self.invalid_numeric(
                    "Invalid exponent in numeric literal".to_owned(),
                    line,
                    exponent_start,
                );

                return col;
            }
        }

        let suffix_start = col;

        while source.get(col).is_some_and(char::is_ascii_alphanumeric) {
            col += 1;
        }

        let suffix = slice(source, suffix_start, col);
        let raw = slice(source, start, col);
        let numeric_part = slice(source, start, suffix_start);

        if !is_float && (suffix == "f32" || suffix == "f64") {
            is_float = true;
        }

        if is_float {
            let size = if suffix.is_empty() {
                Some(64)
            } else {
                float_suffix(&suffix)
            };

            let Some(size) = size else {
                self.invalid_numeric(
                    format!("Invalid float suffix '{suffix}'"),
                    line,
                    suffix_start,
                );

                return col;
            };

            let value = match numeric_part.parse::<f64>() {
                Ok(value) if value.is_finite() => value,
                _ => {
                    self.invalid_numeric(format!("Invalid numeric literal '{raw}'"), line, start);

                    return col;
                }
            };

            self.push(
                TokenKind::FloatLiteral {
                    value,
                    size,
                    suffixed: !suffix.is_empty(),
                },
                line,
                start,
                raw,
            );

            return col;
        }

        let size = if suffix.is_empty() {
            Some(32)
        } else {
            integer_suffix(&suffix)
        };

        let Some(size) = size else {
            self.invalid_numeric(
                format!("Invalid integer suffix '{suffix}'"),
                line,
                suffix_start,
            );

            return col;
        };

        let mut base: IntegerBase = 10;
        let mut digits = numeric_part.as_str();

        if numeric_part.len() > 1 && numeric_part.starts_with('0') {
            base = 8;
            digits = &numeric_part[1..];

            if !digits.chars().all(|c| matches!(c, '0'..='7')) {
                self.invalid_numeric(
                    format!("Invalid octal numeric literal '{raw}'"),
                    line,
                    start,
                );

                return col;
            }
        }

        let Some(value) = BigUint::parse_bytes(digits.as_bytes(), base.into()) else {
            self.invalid_numeric(format!("Invalid numeric literal '{raw}'"), line, start);

            return col;
        };

        let signed = suffix.is_empty() || suffix.starts_with('s');

        self.push(
            TokenKind::IntegerLiteral {
                base,
                size,
                signed,
                value,
                suffixed: !suffix.is_empty(),
            },
            line,
            start,
            raw,
        );

        col
    }

    fn tokenize_prefixed_integer(
        &mut self,
        source: &[char],
        line: usize,
        start: usize,
        base: IntegerBase,
        digit_pattern: fn(char) -> bool,
        prefix: &str,
    ) -> usize {
        let mut col = start + prefix.len();

        let digits_start = col;

        while source.get(col).is_some_and(|&c| digit_pattern(c)) {
            col += 1;
        }

        if col == digits_start {
            self.invalid_numeric(format!("Invalid {prefix} numeric literal"), line, start);

            return col;
        }

        let suffix_start = col;

        while source.get(col).is_some_and(char::is_ascii_alphanumeric) {
            col += 1;
        }

        let suffix = slice(source, suffix_start, col);
        let size = if suffix.is_empty() {
            Some(32)
        } else {
            integer_suffix(&suffix)
        };

        let Some(size) = size else {
            self.invalid_numeric(
                format!("Invalid integer suffix '{suffix}'"),
                line,
                suffix_start,
            );

            return col;
        };

        let raw = slice(source, start, col);

        let digits = slice(source, digits_start, suffix_start);

        let Some(value) = BigUint::parse_bytes(digits.as_bytes(), base.into()) else {
            self.invalid_numeric(format!("Invalid numeric literal '{raw}'"), line, start);

            return col;
        };

        let signed = suffix.is_empty() || suffix.starts_with('s');

        self.push(
            TokenKind::IntegerLiteral {
                base,
                size,
                signed,
                value,
                suffixed: !suffix.is_empty(),
            },
            line,
            start,
            raw,
        );

        col
    }

    fn tokenize_string(&mut self, source: &[char], line: usize, start: usize) -> usize {
        let mut col = start + 1;
        let mut value = String::new();
        let quote = source[start];

        while col < source.len() {
            let current = source[col];

            if current == quote {
                let raw = slice(source, start, col + 1);

                self.push(TokenKind::StringLiteral { value }, line, start, raw);

                return col + 1;
            }

            if current == '\\' {
                let Some(&next) = source.get(col + 1) else {
                    self.error(TokenizerErrorKind::UnterminatedString, line, start);

                    return source.len();
                };

                match next {
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    '\\' => value.push('\\'),
                    '"' => value.push('"'),
                    '0' => value.push('\0'),
                    _ => {
                        self.error(
                            TokenizerErrorKind::InvalidEscapeSequence(format!("\\{next}")),
                            line,
                            col,
                        );
                        value.push(next);
                    }
                }

                col += 2;
                continue;
            }

            value.push(current);
            col += 1;
        }

        self.error(TokenizerErrorKind::UnterminatedString, line, start);

        source.len()
    }
}
