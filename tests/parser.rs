use minlang::{
    Ast, BinaryOperator, Block, Expression, ExpressionKind, Parser, ParserError, ParserErrorKind,
    Tokenizer, TypeInfo, UnaryOperator,
};

fn parse(source: &str) -> Result<Ast, Vec<ParserError>> {
    let tokens = Tokenizer::new(source, None)
        .tokenize()
        .unwrap_or_else(|errors| panic!("tokenizer failed: {errors:?}"));
    let mut parser = Parser::new(tokens, None);
    parser.parse().cloned().map_err(<[_]>::to_vec)
}

fn ok(source: &str) -> Ast {
    parse(source).unwrap_or_else(|errors| {
        let messages: Vec<_> = errors.iter().map(ParserError::format).collect();
        panic!("expected {source:?} to parse, got {messages:#?}")
    })
}

fn errors(source: &str) -> Vec<String> {
    match parse(source) {
        Ok(ast) => panic!("expected {source:?} to fail, got {ast:#?}"),
        Err(errors) => errors.iter().map(ParserError::format).collect(),
    }
}

/// Renders an expression compactly so tests can assert on structure.
fn sexpr(expression: &Expression) -> String {
    use ExpressionKind as E;

    let boxed =
        |expression: &Option<Box<Expression>>| expression.as_deref().map_or("_".to_owned(), sexpr);

    match &expression.kind {
        E::IdentifierExpression { name } => name.clone(),
        E::IntegerLiteralExpression { value, .. } => value.to_string(),
        E::FloatLiteralExpression { value, .. } => format!("{value:?}"),
        E::BooleanLiteralExpression { value } => value.to_string(),
        E::StringLiteralExpression { value } => format!("{value:?}"),
        E::AtExpression => "@".to_owned(),
        E::BinaryExpression {
            operator,
            left,
            right,
        } => format!("({} {} {})", binary(*operator), sexpr(left), sexpr(right)),
        E::UnaryExpression { operator, operand } => {
            let operator = match operator {
                UnaryOperator::Neg => "-",
                UnaryOperator::Not => "!",
                UnaryOperator::Deref => "*",
                UnaryOperator::AddressOf => "&",
            };
            format!("({operator} {})", sexpr(operand))
        }
        E::AssignmentExpression { target, value } => {
            format!("(= {} {})", sexpr(target), sexpr(value))
        }
        E::CallExpression { callee, args } => {
            let args: Vec<_> = args.iter().map(sexpr).collect();
            format!("(call {} [{}])", sexpr(callee), args.join(" "))
        }
        E::IndexExpression { target, index } => {
            format!("(index {} {})", sexpr(target), sexpr(index))
        }
        E::MemberExpression { target, member } => format!("(. {} {member})", sexpr(target)),
        E::BlockExpression { block } => block_sexpr(block),
        E::LetExpression {
            constant,
            name,
            typeinfo,
            value,
        } => format!(
            "({} {name} {} {})",
            if *constant { "const" } else { "let" },
            typeinfo.as_ref().map_or("_".to_owned(), type_sexpr),
            boxed(value)
        ),
        E::IfExpression {
            condition,
            then,
            otherwise,
        } => format!(
            "(if {} {} {})",
            sexpr(condition),
            block_sexpr(then),
            boxed(otherwise)
        ),
        E::ReturnExpression { value } => format!("(return {})", boxed(value)),
        E::LoopExpression { body } => format!("(loop {})", block_sexpr(body)),
        E::DoExpression { body, condition } => {
            format!("(do {} {})", block_sexpr(body), boxed(condition))
        }
        E::BreakExpression => "break".to_owned(),
        E::ContinueExpression => "continue".to_owned(),
        other => format!("{other:?}"),
    }
}

fn block_sexpr(block: &Block) -> String {
    let mut parts: Vec<_> = block
        .body
        .iter()
        .map(|e| format!("{};", sexpr(e)))
        .collect();
    if let Some(result) = &block.result {
        parts.push(sexpr(result));
    }
    format!("{{{}}}", parts.join(" "))
}

fn type_sexpr(typeinfo: &TypeInfo) -> String {
    match typeinfo {
        TypeInfo::NamedType { name } => name.clone(),
        TypeInfo::PointerType { pointee } => format!("*{}", type_sexpr(pointee)),
        TypeInfo::ReferenceType { referent } => format!("&{}", type_sexpr(referent)),
        TypeInfo::ArrayType {
            element,
            size,
            initializer,
        } => format!(
            "{}[{}]{}",
            type_sexpr(element),
            size.map_or("?".to_owned(), |size| size.to_string()),
            initializer.as_deref().map_or(String::new(), sexpr)
        ),
    }
}

fn binary(operator: BinaryOperator) -> &'static str {
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
        B::Add => "+",
        B::Sub => "-",
        B::Mul => "*",
        B::Div => "/",
        B::Mod => "%",
    }
}

/// Parses `source` as the single statement `let _ = <source>;` and renders the
/// value.
fn expr(source: &str) -> String {
    let ast = ok(&format!("let _ = {source};"));
    let ExpressionKind::LetExpression {
        value: Some(value), ..
    } = &ast[0].kind
    else {
        unreachable!()
    };
    sexpr(value)
}

// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

fn import(source: &str) -> (Vec<String>, Vec<String>) {
    let ast = ok(source);
    let ExpressionKind::ImportExpression { module, imports } = &ast[0].kind else {
        panic!("expected import, got {:?}", ast[0].kind);
    };
    (module.clone(), imports.clone())
}

#[test]
fn import_single() {
    assert_eq!(
        import("import std::printf;"),
        (vec!["std".into()], vec!["printf".into()])
    );
}

#[test]
fn import_list() {
    assert_eq!(
        import("import std::{printf, strcat,};"),
        (vec!["std".into()], vec!["printf".into(), "strcat".into()])
    );
}

#[test]
fn import_glob() {
    assert_eq!(
        import("import types::{*};"),
        (vec!["types".into()], vec!["*".into()])
    );
    assert_eq!(
        import("import types::*;"),
        (vec!["types".into()], vec!["*".into()])
    );
}

#[test]
fn import_nested_module() {
    assert_eq!(
        import("import a::b::c;"),
        (vec!["a".into(), "b".into()], vec!["c".into()])
    );
}

#[test]
fn import_requires_semicolon() {
    assert_eq!(
        errors("import std::printf"),
        ["stdin:0:18: Unexpected end of file, expected ';'"]
    );
}

// -----------------------------------------------------------------------------
// Expressions
// -----------------------------------------------------------------------------

#[test]
fn literals() {
    assert_eq!(expr("42"), "42");
    assert_eq!(expr("0x40u8"), "64");
    assert_eq!(expr("1.5f32"), "1.5");
    assert_eq!(expr("\"hi\\n\""), "\"hi\\n\"");
    assert_eq!(expr("@"), "@");
    assert_eq!(expr("true"), "true");
    assert_eq!(expr("false"), "false");
    assert_eq!(expr("std::printf"), "std::printf");
}

#[test]
fn arithmetic_precedence() {
    assert_eq!(expr("1 + 2 * 3"), "(+ 1 (* 2 3))");
    assert_eq!(expr("(1 + 2) * 3"), "(* (+ 1 2) 3)");
    assert_eq!(expr("1 - 2 - 3"), "(- (- 1 2) 3)");
    assert_eq!(expr("a % b / c"), "(/ (% a b) c)");
}

#[test]
fn bitwise_and_logical_precedence() {
    assert_eq!(expr("a | b ^ c & d"), "(| a (^ b (& c d)))");
    assert_eq!(expr("a == b & c"), "(& (== a b) c)");
    assert_eq!(expr("a || b && c"), "(|| a (&& b c))");
    assert_eq!(expr("a < b == c >= d"), "(== (< a b) (>= c d))");
    assert_eq!(expr("a <= b + 1"), "(<= a (+ b 1))");
    assert_eq!(expr("!true || x == false"), "(|| (! true) (== x false))");
}

#[test]
fn unary_operators() {
    assert_eq!(expr("-a * b"), "(* (- a) b)");
    assert_eq!(expr("!a && b"), "(&& (! a) b)");
    assert_eq!(expr("*p + 1"), "(+ (* p) 1)");
    assert_eq!(expr("&x"), "(& x)");
    assert_eq!(expr("&&x"), "(& (& x))");
    assert_eq!(expr("- -a"), "(- (- a))");
}

#[test]
fn assignment_is_right_associative() {
    assert_eq!(expr("a = b = c + 1"), "(= a (= b (+ c 1)))");
}

#[test]
fn postfix_operators() {
    assert_eq!(expr("f()"), "(call f [])");
    assert_eq!(expr("f(a, b + 1,)"), "(call f [a (+ b 1)])");
    assert_eq!(expr("a[i][j]"), "(index (index a i) j)");
    assert_eq!(expr("a.b.c(1)"), "(call (. (. a b) c) [1])");
    assert_eq!(expr("-a[0]"), "(- (index a 0))");
    assert_eq!(expr("*s.p"), "(* (. s p))");
}

#[test]
fn block_expression() {
    assert_eq!(
        expr("{ strcat(@, \"a\"); strcat(@, \"b\"); @ }"),
        "{(call strcat [@ \"a\"]); (call strcat [@ \"b\"]); @}"
    );
    assert_eq!(expr("{ 1; }"), "{1;}");
    assert_eq!(expr("{}"), "{}");
}

#[test]
fn expression_positions() {
    let ast = ok("let x = foo(1,\n  bar);");
    let ExpressionKind::LetExpression {
        value: Some(value), ..
    } = &ast[0].kind
    else {
        unreachable!()
    };

    assert_eq!(
        (value.position.start.line, value.position.start.col),
        (0, 8)
    );
    assert_eq!((value.position.end.line, value.position.end.col), (1, 6));
    assert_eq!(value.tokens, 3..9);
    assert_eq!(ast[0].tokens, 0..9);
}

// -----------------------------------------------------------------------------
// Declarations
// -----------------------------------------------------------------------------

#[test]
fn let_forms() {
    let rendered: Vec<_> = ok("let a; let b: u32; let c = 1; const d: f32 = 1.5f32;")
        .iter()
        .map(sexpr)
        .collect();
    assert_eq!(
        rendered,
        [
            "(let a _ _)",
            "(let b u32 _)",
            "(let c _ 1)",
            "(const d f32 1.5)"
        ]
    );
}

#[test]
fn types() {
    let render = |source: &str| sexpr(&ok(source)[0]);
    assert_eq!(render("let a: u8[?][?];"), "(let a u8[?][?] _)");
    assert_eq!(render("let a: u8[0x40]{0} = 1;"), "(let a u8[64]{0} 1)");
    assert_eq!(render("let a: *u8;"), "(let a *u8 _)");
    assert_eq!(render("let a: &&u8;"), "(let a &&u8 _)");
    assert_eq!(render("let a: *u8[4];"), "(let a *u8[4] _)");
    assert_eq!(render("let a: *(u8[4]);"), "(let a *u8[4] _)");
    assert_eq!(render("let a: types::Foo;"), "(let a types::Foo _)");

    // `*u8[4]` is an array of pointers; `*(u8[4])` a pointer to an array
    let ast = ok("let a: *u8[4]; let b: *(u8[4]);");
    let typeinfo = |index: usize| match &ast[index].kind {
        ExpressionKind::LetExpression {
            typeinfo: Some(typeinfo),
            ..
        } => typeinfo.clone(),
        _ => unreachable!(),
    };
    assert!(matches!(typeinfo(0), TypeInfo::ArrayType { .. }));
    assert!(matches!(typeinfo(1), TypeInfo::PointerType { .. }));
}

#[test]
fn array_suffixes_wrap_left_to_right() {
    // `u8[4][8]` is 8 elements of `u8[4]`: the rightmost suffix is outermost
    let ast = ok("let a: u8[4][8];");
    let ExpressionKind::LetExpression {
        typeinfo: Some(TypeInfo::ArrayType { element, size, .. }),
        ..
    } = &ast[0].kind
    else {
        panic!("expected array type, got {:?}", ast[0].kind);
    };
    assert_eq!(*size, Some(8));

    let TypeInfo::ArrayType { element, size, .. } = element.as_ref() else {
        panic!("expected nested array type, got {element:?}");
    };
    assert_eq!(*size, Some(4));
    assert_eq!(**element, TypeInfo::NamedType { name: "u8".into() });
}

#[test]
fn array_size_too_large() {
    assert_eq!(
        errors("let a: u8[0x10000000000000000];"),
        ["stdin:0:10: Invalid array size '0x10000000000000000'"]
    );
}

#[test]
fn function_declaration() {
    let ast = ok("fn main(argc: s32, argv: u8[?][?]) -> s32 { printf(\"hi\"); 0 }");
    let ExpressionKind::FunctionDeclarationExpression {
        name,
        params,
        return_type,
        body,
    } = &ast[0].kind
    else {
        panic!("expected function, got {:?}", ast[0].kind);
    };

    assert_eq!(name, "main");
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "argc");
    assert_eq!(type_sexpr(&params[1].r#type), "u8[?][?]");
    assert_eq!(return_type.as_ref().map(type_sexpr).as_deref(), Some("s32"));
    assert_eq!(block_sexpr(body), "{(call printf [\"hi\"]); 0}");
}

#[test]
fn function_without_return_type() {
    let ast = ok("fn f() {}");
    assert!(matches!(
        &ast[0].kind,
        ExpressionKind::FunctionDeclarationExpression {
            return_type: None,
            ..
        }
    ));
}

#[test]
fn array_return_type_does_not_swallow_body() {
    let ast = ok("fn f() -> u8[4] { x }");
    let ExpressionKind::FunctionDeclarationExpression {
        return_type, body, ..
    } = &ast[0].kind
    else {
        unreachable!()
    };
    assert_eq!(
        return_type.as_ref().map(type_sexpr).as_deref(),
        Some("u8[4]")
    );
    assert_eq!(block_sexpr(body), "{x}");
}

#[test]
fn struct_and_enum() {
    let ast = ok("struct Point { x: f32, y: f32, } enum Color { Red, Green }");

    let ExpressionKind::StructDeclarationExpression { name, fields } = &ast[0].kind else {
        unreachable!()
    };
    assert_eq!(name, "Point");
    assert_eq!(
        fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["x", "y"]
    );

    let ExpressionKind::EnumDeclarationExpression { name, variants } = &ast[1].kind else {
        unreachable!()
    };
    assert_eq!(name, "Color");
    assert_eq!(variants, &["Red", "Green"]);
}

// -----------------------------------------------------------------------------
// Control flow
// -----------------------------------------------------------------------------

fn body(source: &str) -> String {
    let ast = ok(&format!("fn f() {{ {source} }}"));
    let ExpressionKind::FunctionDeclarationExpression { body, .. } = &ast[0].kind else {
        unreachable!()
    };
    block_sexpr(body)
}

#[test]
fn if_else_chain() {
    assert_eq!(body("if (a) { 1 }"), "{(if a {1} _)}");
    assert_eq!(
        body("if (a) { 1 } else if (b) { 2 } else { 3 }"),
        "{(if a {1} (if b {2} {3}))}"
    );
}

#[test]
fn block_like_statements_need_no_semicolon() {
    assert_eq!(body("if (a) {} *p = 1; x"), "{(if a {} _); (= (* p) 1); x}");
    assert_eq!(body("{ a; } b"), "{{a;}; b}");
}

#[test]
fn switch_statement() {
    let ast = ok("fn f() { switch (x) { case (1) { a; break; } case (2) {} default { b; } } }");
    let ExpressionKind::FunctionDeclarationExpression { body, .. } = &ast[0].kind else {
        unreachable!()
    };
    let ExpressionKind::SwitchExpression {
        subject,
        cases,
        default,
    } = &body.result.as_ref().unwrap().kind
    else {
        panic!("expected switch");
    };

    assert_eq!(sexpr(subject), "x");
    assert_eq!(cases.len(), 2);
    assert_eq!(sexpr(&cases[0].value), "1");
    assert_eq!(block_sexpr(&cases[0].body), "{a; break;}");
    assert_eq!(default.as_ref().map(block_sexpr).as_deref(), Some("{b;}"));
}

#[test]
fn for_loop() {
    for source in ["for x in xs { continue; }", "for (x in xs) { continue; }"] {
        let ast = ok(&format!("fn f() {{ {source} }}"));
        let ExpressionKind::FunctionDeclarationExpression { body, .. } = &ast[0].kind else {
            unreachable!()
        };
        let ExpressionKind::ForExpression {
            binding,
            iterable,
            body,
        } = &body.result.as_ref().unwrap().kind
        else {
            panic!("expected for");
        };
        assert_eq!(binding, "x");
        assert_eq!(sexpr(iterable), "xs");
        assert_eq!(block_sexpr(body), "{continue;}");
    }
}

#[test]
fn loop_statement() {
    assert_eq!(
        body("loop { if (done) { break; } continue; } x"),
        "{(loop {(if done {break;} _); continue;}); x}"
    );
}

#[test]
fn do_block() {
    assert_eq!(
        body("do { if (a) { break; } b(); } c"),
        "{(do {(if a {break;} _); (call b []);} _); c}"
    );
}

#[test]
fn do_while() {
    assert_eq!(
        body("do { i = i + 1; } while (i < 10); x"),
        "{(do {(= i (+ i 1));} (< i 10)); x}"
    );
    assert_eq!(body("do {} while (!done)"), "{(do {} (! done))}");
}

#[test]
fn do_while_requires_semicolon() {
    assert_eq!(
        errors("fn f() { do {} while (a) b; }"),
        ["stdin:0:25: Expected ';' or '}', but got 'b'"]
    );
}

#[test]
fn while_without_do_is_an_error() {
    assert_eq!(
        errors("fn f() { while (a) {} }"),
        ["stdin:0:9: Expected expression, but got 'while'"]
    );
}

#[test]
fn loops_are_expressions() {
    assert_eq!(expr("loop { break; }"), "(loop {break;})");
    assert_eq!(expr("do { 1 }"), "(do {1} _)");
}

#[test]
fn return_forms() {
    assert_eq!(body("return;"), "{(return _);}");
    assert_eq!(body("return a + 1;"), "{(return (+ a 1));}");
    assert_eq!(body("return"), "{(return _)}");
}

// -----------------------------------------------------------------------------
// Errors and recovery
// -----------------------------------------------------------------------------

#[test]
fn missing_semicolon_in_block() {
    assert_eq!(
        errors("fn f() { a b }"),
        ["stdin:0:11: Expected ';' or '}', but got 'b'"]
    );
}

#[test]
fn unexpected_token() {
    assert_eq!(
        errors("let x = ;"),
        ["stdin:0:8: Expected expression, but got ';'"]
    );
    assert_eq!(
        errors("fn (a: s32) {}"),
        ["stdin:0:3: Expected identifier, but got '('"]
    );
    assert_eq!(
        errors("let x: 5;"),
        ["stdin:0:7: Expected type, but got '5'"]
    );
}

#[test]
fn unterminated_block() {
    assert_eq!(
        errors("fn f() { a;"),
        ["stdin:0:11: Unexpected end of file, expected '}'"]
    );
}

#[test]
fn recovers_inside_blocks() {
    let errors = errors(
        "fn f() {
            let a = ;
            let b = 1;
            let c = );
            d
        }
        fn g() { ) }
        let ok = 1;",
    );
    assert_eq!(
        errors,
        [
            "stdin:1:20: Expected expression, but got ';'",
            "stdin:3:20: Expected expression, but got ')'",
            "stdin:6:17: Expected expression, but got ')'",
        ]
    );
}

#[test]
fn recovers_at_top_level() {
    let errors = errors("fn (x) { a; } let 5; let ok = 1; struct { }");
    assert_eq!(
        errors,
        [
            "stdin:0:3: Expected identifier, but got '('",
            "stdin:0:18: Expected identifier, but got '5'",
            "stdin:0:40: Expected identifier, but got '{'",
        ]
    );
}

#[test]
fn eof_error_kind() {
    let Err(errors) = parse("let x =") else {
        panic!("expected failure")
    };
    assert_eq!(
        errors[0].kind,
        ParserErrorKind::Eof {
            expected: "expression".into()
        }
    );
}

// -----------------------------------------------------------------------------
// Examples
// -----------------------------------------------------------------------------

#[test]
fn examples_parse() {
    for file in ["examples/basic.pk", "examples/hello.pk"] {
        let source = std::fs::read_to_string(file).unwrap();
        let ast = ok(&source);
        assert!(matches!(
            ast.last().unwrap().kind,
            ExpressionKind::FunctionDeclarationExpression { .. }
        ));
    }
}

#[test]
fn hello_example_structure() {
    let ast = ok(&std::fs::read_to_string("examples/hello.pk").unwrap());
    let ExpressionKind::FunctionDeclarationExpression { body, .. } = &ast[2].kind else {
        unreachable!()
    };

    assert_eq!(
        sexpr(&body.body[0]),
        "(let message u8[64]{0} {(call strcat [@ \"Hello, \"]); (call strcat [@ \"World!\\n\"]); @})"
    );
    assert_eq!(sexpr(body.result.as_deref().unwrap()), "0");
}
