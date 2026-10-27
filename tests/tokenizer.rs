use minlang::{Token, TokenKind, TokenType, Tokenizer};
use num_bigint::BigUint;

fn tokenize(source: &str) -> Result<Vec<Token>, Vec<minlang::TokenizerError>> {
    Tokenizer::new(source, None).tokenize()
}

fn ok(source: &str) -> Vec<Token> {
    tokenize(source).unwrap_or_else(|errors| panic!("expected ok, got {errors:?}"))
}

fn assert_err(source: &str) {
    assert!(tokenize(source).is_err(), "expected {source:?} to fail");
}

fn assert_int(token: &Token, value: u64, base: u8, size: u8, signed: bool) {
    assert_eq!(
        token.kind,
        TokenKind::IntegerLiteral {
            base,
            size,
            signed,
            value: BigUint::from(value),
        }
    );
}

fn assert_float(token: &Token, value: f64, size: u8) {
    assert_eq!(token.kind, TokenKind::FloatLiteral { value, size });
}

fn assert_int_value_size_signed(
    token: &Token,
    expected: u64,
    expected_size: u8,
    expected_signed: bool,
) {
    let TokenKind::IntegerLiteral {
        value,
        size,
        signed,
        ..
    } = &token.kind
    else {
        panic!("expected IntegerLiteral, got {:?}", token.kind);
    };
    assert_eq!(*value, BigUint::from(expected));
    assert_eq!(*size, expected_size);
    assert_eq!(*signed, expected_signed);
}

#[test]
fn invalid_float_bit_width() {
    assert_err("100.f22");
}

#[test]
fn invalid_int_bit_width() {
    assert_err("100u22");
}

#[test]
fn unexpected_character() {
    assert_err("let unexpected_dollar = $;");
}

#[test]
fn unterminated_string() {
    assert_err("let unterminated_string_1 = \"hello;");
}

#[test]
fn weird_float_syntax() {
    let tokens = ok("let float6 = 2f32;");
    assert_eq!(tokens[3].kind.token_type(), TokenType::FloatLiteral);
}

// -----------------------------------------------------------------------------
// Number parsing
// -----------------------------------------------------------------------------

#[test]
fn decimal_integer() {
    let tokens = ok("123");
    assert_eq!(tokens.len(), 1);
    assert_int(&tokens[0], 123, 10, 32, true);
    assert_eq!(tokens[0].raw, "123");
}

#[test]
fn zero_is_decimal() {
    let tokens = ok("0");
    assert_int(&tokens[0], 0, 10, 32, true);
}

#[test]
fn unsigned_integer_suffixes() {
    let tokens = ok("1u8 2u16 3u32 4u64");
    assert_eq!(tokens.len(), 4);
    assert_int_value_size_signed(&tokens[0], 1, 8, false);
    assert_int_value_size_signed(&tokens[1], 2, 16, false);
    assert_int_value_size_signed(&tokens[2], 3, 32, false);
    assert_int_value_size_signed(&tokens[3], 4, 64, false);
}

#[test]
fn signed_integer_suffixes() {
    let tokens = ok("1s8 2s16 3s32 4s64");
    assert_eq!(tokens.len(), 4);
    assert_int_value_size_signed(&tokens[0], 1, 8, true);
    assert_int_value_size_signed(&tokens[1], 2, 16, true);
    assert_int_value_size_signed(&tokens[2], 3, 32, true);
    assert_int_value_size_signed(&tokens[3], 4, 64, true);
}

#[test]
fn hexadecimal_integer() {
    let tokens = ok("0xff");
    assert_int(&tokens[0], 255, 16, 32, true);
    assert_eq!(tokens[0].raw, "0xff");
}

#[test]
fn hexadecimal_unsigned_integer() {
    let tokens = ok("0xFFu64");
    assert_int(&tokens[0], 255, 16, 64, false);
    assert_eq!(tokens[0].raw, "0xFFu64");
}

#[test]
fn binary_integer() {
    let tokens = ok("0b101010");
    assert_int(&tokens[0], 42, 2, 32, true);
    assert_eq!(tokens[0].raw, "0b101010");
}

#[test]
fn octal_integer() {
    let tokens = ok("0o755");
    assert_int(&tokens[0], 493, 8, 32, true);
    assert_eq!(tokens[0].raw, "0o755");
}

#[test]
fn legacy_leading_zero_octal() {
    let tokens = ok("0673");
    assert_int(&tokens[0], 443, 8, 32, true);
    assert_eq!(tokens[0].raw, "0673");
}

#[test]
fn invalid_legacy_octal() {
    assert_err("089");
}

#[test]
fn missing_hexadecimal_digits() {
    assert_err("0x");
}

#[test]
fn missing_binary_digits() {
    assert_err("0b");
}

#[test]
fn missing_octal_digits() {
    assert_err("0o");
}

#[test]
fn float() {
    let tokens = ok("12.5");
    assert_float(&tokens[0], 12.5, 64);
    assert_eq!(tokens[0].raw, "12.5");
}

#[test]
fn float_f32_suffix() {
    let tokens = ok("12.5f32");
    assert_float(&tokens[0], 12.5, 32);
    assert_eq!(tokens[0].raw, "12.5f32");
}

#[test]
fn float_f64_suffix() {
    let tokens = ok("12.5f64");
    assert_float(&tokens[0], 12.5, 64);
    assert_eq!(tokens[0].raw, "12.5f64");
}

#[test]
fn integer_looking_f32_is_a_float() {
    let tokens = ok("2f32");
    assert_float(&tokens[0], 2.0, 32);
    assert_eq!(tokens[0].raw, "2f32");
}

#[test]
fn integer_looking_f64_is_a_float() {
    let tokens = ok("2f64");
    assert_float(&tokens[0], 2.0, 64);
    assert_eq!(tokens[0].raw, "2f64");
}

#[test]
fn scientific_notation() {
    let tokens = ok("1e10");
    assert_float(&tokens[0], 1e10, 64);
    assert_eq!(tokens[0].raw, "1e10");
}

#[test]
fn scientific_notation_with_positive_exponent() {
    let tokens = ok("1e+10");
    assert_float(&tokens[0], 1e10, 64);
}

#[test]
fn scientific_notation_with_negative_exponent() {
    let tokens = ok("1e-3");
    assert_float(&tokens[0], 0.001, 64);
}

#[test]
fn scientific_notation_with_f32() {
    let tokens = ok("1.5e2f32");
    assert_float(&tokens[0], 150.0, 32);
    assert_eq!(tokens[0].raw, "1.5e2f32");
}

#[test]
fn invalid_exponent() {
    assert_err("1e");
}

#[test]
fn invalid_exponent_with_sign() {
    assert_err("1e+");
}

#[test]
fn invalid_float_suffix() {
    assert_err("12.5foo");
}

#[test]
fn invalid_integer_suffix() {
    assert_err("123foo");
}

#[test]
fn prefixed_integer_with_invalid_suffix() {
    assert_err("0xfffoo");
}

#[test]
fn multiple_numbers() {
    let tokens = ok("1 2u8 3.5f32 0xff");
    assert_eq!(tokens.len(), 4);
    assert_eq!(tokens[0].kind.token_type(), TokenType::IntegerLiteral);
    assert_eq!(tokens[1].kind.token_type(), TokenType::IntegerLiteral);
    assert_eq!(tokens[2].kind.token_type(), TokenType::FloatLiteral);
    assert_eq!(tokens[3].kind.token_type(), TokenType::IntegerLiteral);
}

#[test]
fn numbers_adjacent_to_punctuation() {
    let tokens = ok("foo(123, 4.5);");
    let TokenKind::IntegerLiteral { value, .. } = &tokens[2].kind else {
        panic!("expected IntegerLiteral, got {:?}", tokens[2].kind);
    };
    assert_eq!(*value, BigUint::from(123u32));
    let TokenKind::FloatLiteral { value, .. } = tokens[4].kind else {
        panic!("expected FloatLiteral, got {:?}", tokens[4].kind);
    };
    assert_eq!(value, 4.5);
}

// -----------------------------------------------------------------------------
// Punctuation and keywords
// -----------------------------------------------------------------------------

#[test]
fn two_character_punctuation() {
    let tokens = ok(":: -> == != && || <= >= < > + - & |");
    let raws: Vec<_> = tokens.iter().map(|token| token.raw.as_str()).collect();
    assert_eq!(
        raws,
        [
            "::", "->", "==", "!=", "&&", "||", "<=", ">=", "<", ">", "+", "-", "&", "|"
        ]
    );
    assert!(
        tokens
            .iter()
            .all(|token| token.kind.token_type() == TokenType::Punctuation)
    );
}

#[test]
fn else_is_a_keyword() {
    let tokens = ok("else elsewhere");
    assert_eq!(tokens[0].kind.token_type(), TokenType::Keyword);
    assert_eq!(tokens[1].kind.token_type(), TokenType::Identifier);
}

#[test]
fn boolean_literals() {
    let tokens = ok("true false truthy false_");
    assert_eq!(tokens[0].kind, TokenKind::BooleanLiteral { value: true });
    assert_eq!(tokens[1].kind, TokenKind::BooleanLiteral { value: false });
    assert_eq!(tokens[0].raw, "true");
    assert_eq!(tokens[2].kind.token_type(), TokenType::Identifier);
    assert_eq!(tokens[3].kind.token_type(), TokenType::Identifier);
}

#[test]
fn loop_keywords() {
    let tokens = ok("do while loop break continue looping");
    let kinds: Vec<_> = tokens.iter().map(|token| token.kind.token_type()).collect();
    assert_eq!(
        kinds,
        [
            TokenType::Keyword,
            TokenType::Keyword,
            TokenType::Keyword,
            TokenType::Keyword,
            TokenType::Keyword,
            TokenType::Identifier,
        ]
    );
}
