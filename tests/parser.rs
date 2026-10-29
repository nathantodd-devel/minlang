use minlang::{
    Ast, BinaryOperator, Block, Expression, ExpressionKind, Parser, ParserError, ParserErrorKind,
    Pattern, PatternKind, Tokenizer, TypeInfo, UnaryOperator, VariantFields,
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
        E::AssignmentExpression {
            operator,
            target,
            value,
        } => {
            let operator = operator.map_or("", binary);
            format!("({operator}= {} {})", sexpr(target), sexpr(value))
        }
        E::CastExpression { value, typeinfo } => {
            format!("(as {} {})", sexpr(value), type_sexpr(typeinfo))
        }
        E::WhileExpression { condition, body } => {
            format!("(while {} {})", sexpr(condition), block_sexpr(body))
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
            public,
            constant,
            name,
            typeinfo,
            value,
        } => format!(
            "({}{} {name} {} {})",
            if *public { "pub " } else { "" },
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
        E::StructLiteralExpression { name, fields } => {
            let fields: Vec<_> = fields
                .iter()
                .map(|field| format!(" {}={}", field.name, sexpr(&field.value)))
                .collect();
            format!("({name}{})", fields.concat())
        }
        E::YieldExpression { value } => format!("(yield {})", sexpr(value)),
        E::ReturnExpression { value } => format!("(return {})", boxed(value)),
        E::ForExpression {
            binding,
            iterable,
            body,
        } => format!("(for {binding} {} {})", sexpr(iterable), block_sexpr(body)),
        E::ForClauseExpression {
            init,
            condition,
            step,
            each,
            body,
        } => format!(
            "(for-clause {} {} {} {} {})",
            boxed(init),
            boxed(condition),
            boxed(step),
            boxed(each),
            block_sexpr(body)
        ),
        E::IncrementExpression { target, decrement } => {
            format!(
                "({} {})",
                if *decrement { "--" } else { "++" },
                sexpr(target)
            )
        }
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

/// Bindings render as `$name` to tell them apart from constants.
fn pattern_sexpr(pattern: &Pattern) -> String {
    match &pattern.kind {
        PatternKind::WildcardPattern => "_".to_owned(),
        PatternKind::BindingPattern { name } => format!("${name}"),
        PatternKind::ValuePattern { value } => sexpr(value),
        PatternKind::TupleVariantPattern { path, elements } => {
            let elements: Vec<_> = elements.iter().map(pattern_sexpr).collect();
            format!("({path} {})", elements.join(" "))
        }
        PatternKind::StructVariantPattern { path, fields } => {
            let fields: Vec<_> = fields
                .iter()
                .map(|field| format!("{}={}", field.name, pattern_sexpr(&field.pattern)))
                .collect();
            format!("({path} {{{}}})", fields.join(" "))
        }
    }
}

fn type_sexpr(typeinfo: &TypeInfo) -> String {
    match typeinfo {
        TypeInfo::NamedType { name } => name.clone(),
        TypeInfo::PointerType { pointee } => format!("*{}", type_sexpr(pointee)),
        TypeInfo::ReferenceType { referent } => format!("&{}", type_sexpr(referent)),
        TypeInfo::IteratorType { item } => format!("iter {}", type_sexpr(item)),
        TypeInfo::ArrayType { initializer, .. } => {
            // Render dimensions outermost first, as they are written
            let mut dimensions = String::new();
            let mut element = typeinfo;
            while let TypeInfo::ArrayType {
                element: inner,
                size,
                ..
            } = element
            {
                let size = size.map_or("?".to_owned(), |size| size.to_string());
                dimensions.push_str(&format!("[{size}]"));
                element = inner;
            }
            format!(
                "{}{dimensions}{}",
                type_sexpr(element),
                initializer.as_deref().map_or(String::new(), sexpr)
            )
        }
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
        B::Shl => "<<",
        B::Shr => ">>",
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
        ["stdin:1:19: Unexpected end of file, expected ';'"]
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
fn shift_precedence() {
    // Shifts bind looser than `+` and tighter than comparisons, as in C
    assert_eq!(expr("a << b + 1"), "(<< a (+ b 1))");
    assert_eq!(expr("a >> 2 < b"), "(< (>> a 2) b)");
    assert_eq!(expr("1 << 2 << 3"), "(<< (<< 1 2) 3)");
}

#[test]
fn compound_assignment() {
    assert_eq!(expr("a += 1"), "(+= a 1)");
    assert_eq!(expr("a -= b * 2"), "(-= a (* b 2))");
    assert_eq!(expr("a <<= 1"), "(<<= a 1)");
    assert_eq!(expr("a >>= 1"), "(>>= a 1)");
    assert_eq!(expr("p.x *= 2"), "(*= (. p x) 2)");
    assert_eq!(expr("a[i] |= b ^= c"), "(|= (index a i) (^= b c))");
    assert_eq!(expr("a /= 2"), "(/= a 2)");
    assert_eq!(expr("a %= 2"), "(%= a 2)");
    assert_eq!(expr("a &= 2"), "(&= a 2)");
}

#[test]
fn casts() {
    assert_eq!(expr("x as u8"), "(as x u8)");
    assert_eq!(expr("-x as u8"), "(as (- x) u8)");
    assert_eq!(expr("a + b as u8"), "(+ a (as b u8))");
    assert_eq!(expr("x as u32 as f64"), "(as (as x u32) f64)");
    assert_eq!(expr("&x as *u8"), "(as (& x) *u8)");
    assert_eq!(expr("f(x) as s64 * 2"), "(* (as (call f [x]) s64) 2)");
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
fn pub_declarations() {
    let ast = ok(
        "pub fn f() {} pub struct P {} pub enum E { A } pub let a = 1; pub const b = 2; fn g() {}",
    );
    let public: Vec<bool> = ast
        .iter()
        .map(|item| match &item.kind {
            ExpressionKind::FunctionDeclarationExpression { public, .. }
            | ExpressionKind::StructDeclarationExpression { public, .. }
            | ExpressionKind::EnumDeclarationExpression { public, .. }
            | ExpressionKind::LetExpression { public, .. } => *public,
            _ => panic!("expected a declaration"),
        })
        .collect();
    assert_eq!(public, [true, true, true, true, true, false]);
    assert_eq!(sexpr(&ast[4]), "(pub const b _ 2)");

    assert_eq!(
        errors("pub if (a) {}"),
        ["stdin:1:5: Expected 'fn', 'let', 'const', 'struct' or 'enum', but got 'if'"]
    );
}

#[test]
fn types() {
    let render = |source: &str| sexpr(&ok(source)[0]);
    assert_eq!(render("let a: u8[?][?];"), "(let a u8[?][?] _)");
    assert_eq!(render("let a: u8[0x40]{0} = 1;"), "(let a u8[64]{0} 1)");
    assert_eq!(render("let a: u8[2][3][4]{0};"), "(let a u8[2][3][4]{0} _)");
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
fn array_dimensions_read_like_c() {
    // `u8[4][8]` is 4 elements of `u8[8]`: the leftmost suffix is outermost
    let ast = ok("let a: u8[4][8];");
    let ExpressionKind::LetExpression {
        typeinfo: Some(TypeInfo::ArrayType { element, size, .. }),
        ..
    } = &ast[0].kind
    else {
        panic!("expected array type, got {:?}", ast[0].kind);
    };
    assert_eq!(*size, Some(4));

    let TypeInfo::ArrayType { element, size, .. } = element.as_ref() else {
        panic!("expected nested array type, got {element:?}");
    };
    assert_eq!(*size, Some(8));
    assert_eq!(**element, TypeInfo::NamedType { name: "u8".into() });
}

#[test]
fn array_size_too_large() {
    assert_eq!(
        errors("let a: u8[0x10000000000000000];"),
        ["stdin:1:11: Invalid array size '0x10000000000000000'"]
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
        ..
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

    let ExpressionKind::StructDeclarationExpression { name, fields, .. } = &ast[0].kind else {
        unreachable!()
    };
    assert_eq!(name, "Point");
    assert_eq!(
        fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["x", "y"]
    );

    let ExpressionKind::EnumDeclarationExpression {
        name,
        backing_type,
        variants,
        ..
    } = &ast[1].kind
    else {
        unreachable!()
    };
    assert_eq!(name, "Color");
    assert_eq!(*backing_type, None);
    assert_eq!(
        variants.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
        ["Red", "Green"]
    );
    assert!(variants.iter().all(|v| v.value.is_none()));
}

#[test]
fn enum_with_backing_type_and_values() {
    let ast = ok("enum Flag: u8 { Read = 1, Write = 2, Exec = 4 | 0, None, }");
    let ExpressionKind::EnumDeclarationExpression {
        name,
        backing_type,
        variants,
        ..
    } = &ast[0].kind
    else {
        unreachable!()
    };

    assert_eq!(name, "Flag");
    assert_eq!(backing_type.as_ref().map(type_sexpr).as_deref(), Some("u8"));

    let rendered: Vec<_> = variants
        .iter()
        .map(|variant| {
            let value = variant.value.as_ref().map_or("_".to_owned(), sexpr);
            format!("{}={value}", variant.name)
        })
        .collect();
    assert_eq!(rendered, ["Read=1", "Write=2", "Exec=(| 4 0)", "None=_"]);
}

#[test]
fn enum_variants_with_data() {
    let ast =
        ok("enum Shape { Circle(f32), Rect { w: f32, h: f32, }, Empty, Tagged(u8, *u8) = 4 }");
    let ExpressionKind::EnumDeclarationExpression { variants, .. } = &ast[0].kind else {
        unreachable!()
    };

    let rendered: Vec<_> = variants
        .iter()
        .map(|variant| {
            let fields = match &variant.fields {
                VariantFields::Unit => String::new(),
                VariantFields::Tuple { types } => {
                    let types: Vec<_> = types.iter().map(type_sexpr).collect();
                    format!("({})", types.join(", "))
                }
                VariantFields::Struct { fields } => {
                    let fields: Vec<_> = fields
                        .iter()
                        .map(|field| format!("{}: {}", field.name, type_sexpr(&field.r#type)))
                        .collect();
                    format!(" {{ {} }}", fields.join(", "))
                }
            };
            let value = variant
                .value
                .as_ref()
                .map_or(String::new(), |value| format!(" = {}", sexpr(value)));
            format!("{}{fields}{value}", variant.name)
        })
        .collect();

    assert_eq!(
        rendered,
        [
            "Circle(f32)",
            "Rect { w: f32, h: f32 }",
            "Empty",
            "Tagged(u8, *u8) = 4"
        ]
    );
}

#[test]
fn enum_variant_construction() {
    assert_eq!(expr("Shape::Circle(1.5)"), "(call Shape::Circle [1.5])");
    assert_eq!(expr("Shape::Rect { w: 1, h: 2 }"), "(Shape::Rect w=1 h=2)");
    assert_eq!(expr("Shape::Empty"), "Shape::Empty");
}

/// Parses `cases` inside a switch and renders each case's pattern and body.
fn cases(cases: &str) -> Vec<String> {
    let ast = ok(&format!("fn f() {{ switch s {{ {cases} }} }}"));
    let ExpressionKind::FunctionDeclarationExpression { body, .. } = &ast[0].kind else {
        unreachable!()
    };
    let ExpressionKind::SwitchExpression { cases, .. } = &body.result.as_ref().unwrap().kind else {
        panic!("expected switch");
    };
    cases
        .iter()
        .map(|case| {
            format!(
                "{} {}",
                pattern_sexpr(&case.pattern),
                block_sexpr(&case.body)
            )
        })
        .collect()
}

#[test]
fn variant_patterns() {
    assert_eq!(
        cases(
            "case Shape::Circle(r) { r; }
             case Shape::Rect { w, h: height } { w * height }
             case Shape::Rect {} {}
             case Shape::Empty { a; }
             case Shape::Tagged(4, _) {}"
        ),
        [
            "(Shape::Circle $r) {r;}",
            "(Shape::Rect {w=$w h=$height}) {(* w height)}",
            "(Shape::Rect {}) {}",
            "Shape::Empty {a;}",
            "(Shape::Tagged 4 _) {}",
        ]
    );
}

#[test]
fn nested_and_parenthesized_patterns() {
    assert_eq!(
        cases(
            "case Outer::A(Inner::B(x), Inner::C { v: _ }) {}
             case (Shape::Rect { w, h }) {}
             case (Shape::Circle(r)) {}"
        ),
        [
            "(Outer::A (Inner::B $x) (Inner::C {v=_})) {}",
            "(Shape::Rect {w=$w h=$h}) {}",
            "(Shape::Circle $r) {}",
        ]
    );
}

#[test]
fn value_patterns() {
    assert_eq!(
        cases(
            "case (1.5) {} case -1 {} case LIMIT {} case LIMIT + 1 {} case \"s\" {} case true {} case _ {}"
        ),
        [
            "1.5 {}",
            "(- 1) {}",
            "LIMIT {}",
            "(+ LIMIT 1) {}",
            "\"s\" {}",
            "true {}",
            "_ {}"
        ]
    );
}

#[test]
fn pattern_positions() {
    let ast = ok("fn f() { switch s { case A::B(x) {} } }");
    let ExpressionKind::FunctionDeclarationExpression { body, .. } = &ast[0].kind else {
        unreachable!()
    };
    let ExpressionKind::SwitchExpression { cases, .. } = &body.result.as_ref().unwrap().kind else {
        unreachable!()
    };
    let pattern = &cases[0].pattern;
    let PatternKind::TupleVariantPattern { elements, .. } = &pattern.kind else {
        unreachable!()
    };

    assert_eq!(
        (pattern.position.start.col, pattern.position.end.col),
        (25, 32)
    );
    assert_eq!(
        (elements[0].position.start.col, elements[0].position.end.col),
        (30, 31)
    );
}

#[test]
fn pattern_errors() {
    assert_eq!(
        errors("fn f() { switch s { case A::B(x y) {} } }"),
        ["stdin:1:33: Expected ')', but got 'y'"]
    );
    assert_eq!(
        errors("fn f() { switch s { case A::B { x: } {} } }"),
        ["stdin:1:36: Expected expression, but got '}'"]
    );
}

// -----------------------------------------------------------------------------
// Generators
// -----------------------------------------------------------------------------

#[test]
fn generator_function() {
    let ast = ok("fn evens(limit: s32) -> iter s32 {
        for i in range(0, limit) {
            if (i % 2 == 0) { yield i; }
        }
    }");
    let ExpressionKind::FunctionDeclarationExpression {
        return_type, body, ..
    } = &ast[0].kind
    else {
        unreachable!()
    };

    assert_eq!(
        return_type.as_ref().map(type_sexpr).as_deref(),
        Some("iter s32")
    );
    assert_eq!(
        block_sexpr(body),
        "{(for i (call range [0 limit]) {(if (== (% i 2) 0) {(yield i);} _)})}"
    );
}

#[test]
fn yield_takes_any_expression() {
    assert_eq!(body("yield a + 1;"), "{(yield (+ a 1));}");
    assert_eq!(body("yield Point { x: 1 };"), "{(yield (Point x=1));}");
}

#[test]
fn yield_requires_a_value() {
    assert_eq!(
        errors("fn f() -> iter s32 { yield; }"),
        ["stdin:1:27: Expected expression, but got ';'"]
    );
}

#[test]
fn iterator_types() {
    let render = |source: &str| sexpr(&ok(source)[0]);
    assert_eq!(render("let a: iter s32;"), "(let a iter s32 _)");
    assert_eq!(render("let a: *iter s32;"), "(let a *iter s32 _)");
    assert_eq!(render("let a: iter iter u8;"), "(let a iter iter u8 _)");

    let typeinfo = |source: &str| match &ok(source)[0].kind {
        ExpressionKind::LetExpression {
            typeinfo: Some(typeinfo),
            ..
        } => typeinfo.clone(),
        _ => unreachable!(),
    };

    // `iter` takes the whole type to its right
    let TypeInfo::IteratorType { item } = typeinfo("let a: iter u8[4];") else {
        panic!("expected iterator type");
    };
    assert!(matches!(*item, TypeInfo::ArrayType { size: Some(4), .. }));

    // Parentheses give an array of iterators
    let TypeInfo::ArrayType { element, .. } = typeinfo("let a: (iter u8)[4];") else {
        panic!("expected array type");
    };
    assert!(matches!(*element, TypeInfo::IteratorType { .. }));
}

#[test]
fn iterators_are_values() {
    assert_eq!(
        body("let it: iter s32 = evens(10); for x in it { x }"),
        "{(let it iter s32 (call evens [10])); (for x it {x})}"
    );
}

#[test]
fn iter_is_reserved() {
    assert_eq!(
        errors("let iter = 1;"),
        ["stdin:1:5: Expected identifier, but got 'iter'"]
    );
}

#[test]
fn enum_variant_access() {
    assert_eq!(expr("Color::Red"), "Color::Red");
    assert_eq!(expr("c == Color::Red"), "(== c Color::Red)");
}

#[test]
fn struct_literal() {
    assert_eq!(expr("Point { x: 1, y: 2 + 3 }"), "(Point x=1 y=(+ 2 3))");
    assert_eq!(expr("Point { x: 1, }"), "(Point x=1)");
    assert_eq!(expr("Empty {}"), "(Empty)");
    assert_eq!(expr("geo::Point { x: 1 }"), "(geo::Point x=1)");
}

#[test]
fn nested_struct_literals_and_access() {
    assert_eq!(
        expr("Line { a: Point { x: 1 }, b: origin() }.a.x"),
        "(. (. (Line a=(Point x=1) b=(call origin [])) a) x)"
    );
    assert_eq!(expr("f(Point { x: 1 })"), "(call f [(Point x=1)])");
    assert_eq!(expr("ps[Index { i: 0 }.i]"), "(index ps (. (Index i=0) i))");
}

#[test]
fn struct_literal_not_allowed_before_block() {
    // `x {` is the condition then the body, not a struct literal
    assert_eq!(body("if x { 1 }"), "{(if x {1} _)}");
    assert_eq!(body("for p in points { p }"), "{(for p points {p})}");

    let ast = ok("fn f() { switch c { case Color::Red { a; } default {} } }");
    let ExpressionKind::FunctionDeclarationExpression { body, .. } = &ast[0].kind else {
        unreachable!()
    };
    let ExpressionKind::SwitchExpression { subject, cases, .. } =
        &body.result.as_ref().unwrap().kind
    else {
        panic!("expected switch");
    };
    assert_eq!(sexpr(subject), "c");
    assert_eq!(pattern_sexpr(&cases[0].pattern), "Color::Red");
}

#[test]
fn struct_literal_allowed_in_parenthesized_condition() {
    assert_eq!(
        body("if (p == Point { x: 1 }) { 1 }"),
        "{(if (== p (Point x=1)) {1} _)}"
    );
    assert_eq!(body("if x { Point { x: 1 } }"), "{(if x {(Point x=1)} _)}");
}

#[test]
fn struct_literal_errors() {
    assert_eq!(
        errors("let p = Point { x 1 };"),
        ["stdin:1:19: Expected ':', but got '1'"]
    );
    assert_eq!(
        errors("let p = Point { x: 1 y: 2 };"),
        ["stdin:1:22: Expected '}', but got 'y'"]
    );
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
    assert_eq!(pattern_sexpr(&cases[0].pattern), "1");
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
        ["stdin:1:26: Expected ';' or '}', but got 'b'"]
    );
}

#[test]
fn while_loop() {
    assert_eq!(
        body("while (i < 10) { i += 1; } x"),
        "{(while (< i 10) {(+= i 1);}); x}"
    );
    assert_eq!(body("while x { y }"), "{(while x {y})}");
    assert_eq!(expr("while (a) { break; }"), "(while a {break;})");
}

#[test]
fn bare_do_before_while_needs_a_semicolon() {
    // Without the `;`, `while (a)` is read as the `do` block's condition
    assert_eq!(
        body("do { x; }; while (a) { y; }"),
        "{(do {x;} _); (while a {y;})}"
    );
    assert_eq!(body("do { x; } while (a); z"), "{(do {x;} a); z}");
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
        ["stdin:1:12: Expected ';' or '}', but got 'b'"]
    );
}

#[test]
fn unexpected_token() {
    assert_eq!(
        errors("let x = ;"),
        ["stdin:1:9: Expected expression, but got ';'"]
    );
    assert_eq!(
        errors("fn (a: s32) {}"),
        ["stdin:1:4: Expected identifier, but got '('"]
    );
    assert_eq!(
        errors("let x: 5;"),
        ["stdin:1:8: Expected type, but got '5'"]
    );
}

#[test]
fn unterminated_block() {
    assert_eq!(
        errors("fn f() { a;"),
        ["stdin:1:12: Unexpected end of file, expected '}'"]
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
            "stdin:2:21: Expected expression, but got ';'",
            "stdin:4:21: Expected expression, but got ')'",
            "stdin:7:18: Expected expression, but got ')'",
        ]
    );
}

#[test]
fn recovers_inside_braces_the_statement_opened() {
    // The bad pattern is inside the switch's braces; recovery must skip to the
    // switch's `}` rather than mistaking it for the end of the function
    let errors = errors(
        "fn f() {
            switch s { case A::B(x y) {} case C {} }
            let ok = 1;
            let bad = ;
        }
        let fine = 2;",
    );
    assert_eq!(
        errors,
        [
            "stdin:2:36: Expected ')', but got 'y'",
            "stdin:4:23: Expected expression, but got ';'",
        ]
    );
}

#[test]
fn recovers_at_top_level() {
    let errors = errors("fn (x) { a; } let 5; let ok = 1; struct { }");
    assert_eq!(
        errors,
        [
            "stdin:1:4: Expected identifier, but got '('",
            "stdin:1:19: Expected identifier, but got '5'",
            "stdin:1:41: Expected identifier, but got '{'",
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
    for file in [
        "examples/basic.pk",
        "examples/hello.pk",
        "examples/structs.pk",
        "examples/iterators.pk",
    ] {
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

// -----------------------------------------------------------------------------
// C-style for and increments
// -----------------------------------------------------------------------------

#[test]
fn increment_statements() {
    assert_eq!(
        body("i++; j--; a[i]++; p.x--; *q++;"),
        "{(++ i); (-- j); (++ (index a i)); (-- (. p x)); (++ (* q));}"
    );
    assert_eq!(body("i++"), "{(++ i)}");
}

#[test]
fn increments_are_not_expressions() {
    assert_eq!(
        errors("fn f() { let x = i++; }"),
        ["stdin:1:19: Expected ';' or '}', but got '++'"]
    );
    assert_eq!(
        errors("fn f() { g(i++); }"),
        ["stdin:1:13: Expected ')', but got '++'"]
    );
    assert_eq!(
        errors("fn f() { 1++; }"),
        ["stdin:1:11: Expected ';' or '}', but got '++'"]
    );
}

#[test]
fn for_clauses() {
    assert_eq!(
        body("for (let i: u32 = 0; i < n; i++) { i }"),
        "{(for-clause (let i u32 0) (< i n) (++ i) _ {i})}"
    );
    assert_eq!(
        body("for (i = 0; i < n; i += 1) {}"),
        "{(for-clause (= i 0) (< i n) (+= i 1) _ {})}"
    );
    assert_eq!(
        body("for (;;) { break; }"),
        "{(for-clause _ _ _ _ {break;})}"
    );
    assert_eq!(body("for (; i < n;) {}"), "{(for-clause _ (< i n) _ _ {})}");
}

#[test]
fn for_clause_with_each() {
    assert_eq!(
        body("for (let i: u32 = 0; i < len(arr); i++; let item = get(arr, i)) { item }"),
        "{(for-clause (let i u32 0) (< i (call len [arr])) (++ i) (let item _ (call get [arr i])) {item})}"
    );
    // The step may be left out even with an `each` clause
    assert_eq!(
        body("for (let i = 0; i < n; ; let x = i) {}"),
        "{(for-clause (let i _ 0) (< i n) _ (let x _ i) {})}"
    );
}

#[test]
fn for_clause_each_must_be_let() {
    assert_eq!(
        errors("fn f() { for (;;i++; x = 1) {} }"),
        ["stdin:1:22: Expected 'let', but got 'x'"]
    );
}

#[test]
fn for_in_still_parses() {
    assert_eq!(body("for (x in xs) { x }"), "{(for x xs {x})}");
    assert_eq!(body("for x in xs { x }"), "{(for x xs {x})}");
}

#[test]
fn for_clause_allows_struct_literals() {
    assert_eq!(
        body("for (let p = Point { x: 0 }; p.x < 3; p.x++) {}"),
        "{(for-clause (let p _ (Point x=0)) (< (. p x) 3) (++ (. p x)) _ {})}"
    );
}
