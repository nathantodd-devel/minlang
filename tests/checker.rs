use minlang::{CheckError, Checker, Parser, Tokenizer};

/// Tokenizes, parses and checks `source`, returning the formatted check errors.
fn check(source: &str) -> Vec<String> {
    let tokens = Tokenizer::new(source, None)
        .tokenize()
        .unwrap_or_else(|errors| panic!("tokenizer failed: {errors:?}"));
    let mut parser = Parser::new(tokens, None);
    let ast = parser
        .parse()
        .unwrap_or_else(|errors| panic!("parser failed: {errors:?}"))
        .clone();

    match Checker::new(None).check(&ast) {
        Ok(()) => Vec::new(),
        Err(errors) => errors.iter().map(CheckError::format).collect(),
    }
}

fn ok(source: &str) {
    let errors = check(source);
    assert!(
        errors.is_empty(),
        "expected {source:?} to check, got {errors:#?}"
    );
}

/// Checks `body` inside a function, returning just the messages.
/// Checks `body` inside a function with some typed parameters to work with,
/// returning just the messages.
fn check_body(body: &str) -> Vec<String> {
    check(&format!(
        "struct Point {{ x: s32, y: s32 }}\nfn f(p: s32, flag: bool, point: *Point, numbers: iter s32) {{\n{body}\n}}"
    ))
        .into_iter()
        .map(|error| error.split(": ").skip(1).collect::<Vec<_>>().join(": "))
        .collect()
}

fn ok_body(body: &str) {
    let errors = check_body(body);
    assert!(
        errors.is_empty(),
        "expected {body:?} to check, got {errors:#?}"
    );
}

#[test]
fn examples_check() {
    for file in [
        "examples/basic.pk",
        "examples/hello.pk",
        "examples/structs.pk",
        "examples/iterators.pk",
    ] {
        let source = std::fs::read_to_string(file).unwrap();
        let errors = check(&source);
        assert!(errors.is_empty(), "{file}: {errors:#?}");
    }
}

// -----------------------------------------------------------------------------
// Names and scopes
// -----------------------------------------------------------------------------

#[test]
fn undefined_names() {
    assert_eq!(check_body("x;"), ["Undefined name 'x'"]);
    assert_eq!(check_body("let a = b + 1;"), ["Undefined name 'b'"]);
}

#[test]
fn error_positions_count_from_one() {
    assert_eq!(
        check("fn f() {\n    let a = b;\n}"),
        ["stdin:2:13: Undefined name 'b'"]
    );
}

#[test]
fn variables_are_visible_after_declaration() {
    ok_body("let a = 1; let b = a + p;");
    assert_eq!(check_body("a; let a = 1;"), ["Undefined name 'a'"]);
    // The value is checked before the name exists
    assert_eq!(check_body("let a = a;"), ["Undefined name 'a'"]);
}

#[test]
fn block_scopes() {
    assert_eq!(check_body("{ let a = 1; } a;"), ["Undefined name 'a'"]);
    assert_eq!(
        check_body("if (flag) { let a = 1; } a;"),
        ["Undefined name 'a'"]
    );
    // Shadowing in an inner scope is allowed
    ok_body("let a = 1; { let a = 2; a; }");
}

#[test]
fn redeclaration_in_one_scope() {
    assert_eq!(
        check_body("let a = 1; let a = 2;"),
        ["'a' is already declared in this scope"]
    );
    // Parameters share the body's scope
    assert_eq!(
        check_body("let p = 1;"),
        ["'p' is already declared in this scope"]
    );
    assert_eq!(
        check("fn f(a: s32, a: s32) {}"),
        ["stdin:1:1: 'a' is already declared in this scope"]
    );
    assert_eq!(
        check("struct A {} fn A() {}"),
        ["stdin:1:13: 'A' is already declared in this scope"]
    );
}

#[test]
fn declarations_can_be_used_before_they_appear() {
    ok(
        "fn a() { b(); let p: Point = Point { x: 1 }; c::X; } fn b() {} struct Point { x: s32 } enum c { X }",
    );
    // Also inside blocks
    ok_body("inner(); fn inner() {}");
}

#[test]
fn nested_functions_cannot_see_outer_loops() {
    assert_eq!(
        check_body("loop { fn g() { break; } break; }"),
        ["'break' outside of a loop, 'do' or 'switch'"]
    );
}

#[test]
fn only_declarations_at_top_level() {
    assert_eq!(
        check("fn f() {} f();"),
        [
            "stdin:1:11: Only declarations (import, fn, struct, enum, let, const) are allowed at the top level"
        ]
    );
    ok("let a = 1; const B = 2;");
}

// -----------------------------------------------------------------------------
// Imports
// -----------------------------------------------------------------------------

#[test]
fn imports() {
    ok("import std::printf; fn f() { printf(\"hi\"); }");
    ok("import std::{printf, strcat}; fn f() { strcat(1, 2); printf(); }");
    ok("import std::{*}; fn f() { strcat(1, 2); }");
    ok("import types::*; fn f(a: u8) {}");
}

#[test]
fn qualified_names_need_no_import() {
    ok("fn f() { std::printf(\"hi\"); }");
    ok("fn f(a: types::u8) {}");
}

#[test]
fn imported_names_need_an_import() {
    assert_eq!(
        check("fn f() { printf(\"hi\"); }"),
        ["stdin:1:10: Undefined name 'printf'"]
    );
}

#[test]
fn import_errors() {
    assert_eq!(
        check("import nope::x;"),
        ["stdin:1:1: Unknown module 'nope'"]
    );
    assert_eq!(
        check("import std::{printf, nope};"),
        ["stdin:1:1: 'std' has no member 'nope'"]
    );
    assert_eq!(check_body("std::nope();"), ["'std' has no member 'nope'"]);
    assert_eq!(
        check("import std::printf; fn printf() {}"),
        ["stdin:1:21: 'printf' is already declared in this scope"]
    );
}

// -----------------------------------------------------------------------------
// Types
// -----------------------------------------------------------------------------

#[test]
fn primitive_types_need_no_import() {
    ok("fn f(a: u8, b: s64, c: f32, d: bool, e: usize) -> u16 { 0 }");
}

#[test]
fn type_errors() {
    assert_eq!(check_body("let a: Nope;"), ["Undefined name 'Nope'"]);
    assert_eq!(check_body("let a: p;"), ["'p' is not a type"]);
    assert_eq!(
        check_body("let a: *u8[4][?] = 1 as Nope;"),
        ["Undefined name 'Nope'"]
    );
    assert_eq!(
        check("fn f(a: iter Nope) {}"),
        ["stdin:1:1: Undefined name 'Nope'"]
    );
    assert_eq!(check_body("let a: u8[4]{nope};"), ["Undefined name 'nope'"]);
}

#[test]
fn types_are_not_values() {
    assert_eq!(check_body("let a = u8;"), ["'u8' is a type, not a value"]);
    assert_eq!(
        check("struct P {} fn f() { let a = P; P(); }"),
        [
            "stdin:1:30: 'P' is a type, not a value",
            "stdin:1:33: 'P' is a type, not a value"
        ]
    );
}

#[test]
fn members_of_non_namespaces() {
    assert_eq!(check_body("p::x;"), ["'p' is not an enum or module"]);
    assert_eq!(check_body("a::b::c;"), ["Undefined name 'a::b::c'"]);
}

// -----------------------------------------------------------------------------
// Functions and calls
// -----------------------------------------------------------------------------

#[test]
fn argument_counts() {
    ok("fn add(a: s32, b: s32) -> s32 { add(a, b) }");
    assert_eq!(
        check("fn add(a: s32, b: s32) -> s32 { add(a) }"),
        ["stdin:1:33: 'add' takes 2 arguments, but 1 was given"]
    );
    assert_eq!(
        check("fn one(a: s32) { one(1, 2, 3); }"),
        ["stdin:1:18: 'one' takes 1 argument, but 3 were given"]
    );
}

#[test]
fn calling_non_functions() {
    assert_eq!(
        check_body("p(1, 2);"),
        ["Cannot call a value of type 's32'"]
    );
    assert_eq!(
        check_body("f;"),
        ["Function 'f' cannot be used as a value; call it instead"]
    );
}

#[test]
fn return_values() {
    ok("fn f() -> s32 { return 1; }");
    ok("fn f() { return; }");
    assert_eq!(
        check("fn f() -> u8[4] { return; }"),
        ["stdin:1:19: Missing return value: 'f' returns 'u8[4]'"]
    );
    assert_eq!(
        check("fn f() { return 1; }"),
        ["stdin:1:10: 'f' has no return type, so 'return' cannot take a value"]
    );
    assert_eq!(
        check("let a = { return; };"),
        ["stdin:1:11: 'return' outside of a function"]
    );
}

// -----------------------------------------------------------------------------
// Generators
// -----------------------------------------------------------------------------

#[test]
fn generators() {
    ok("fn g() -> iter s32 { yield 1; return; }");
    assert_eq!(
        check("fn f() -> s32 { yield 1; 0 }"),
        ["stdin:1:17: 'yield' outside of a generator (a function returning 'iter T')"]
    );
    assert_eq!(
        check("fn g() -> iter s32 { return 1; }"),
        ["stdin:1:22: Generators finish with a bare 'return'; use 'yield' to produce values"]
    );
}

#[test]
fn nested_functions_have_their_own_generator_context() {
    assert_eq!(
        check("fn g() -> iter s32 { fn inner() { yield 1; } yield 2; }"),
        ["stdin:1:35: 'yield' outside of a generator (a function returning 'iter T')"]
    );
}

// -----------------------------------------------------------------------------
// Loops and jumps
// -----------------------------------------------------------------------------

#[test]
fn break_and_continue() {
    ok_body("loop { break; continue; }");
    ok_body("while (flag) { break; continue; }");
    ok_body("for x in numbers { break; continue; x; }");
    ok_body("do { break; continue; } while (flag);");
    // `break` leaves a bare `do` or a `switch`, but neither can be continued
    ok_body("do { break; }");
    ok_body("switch p { case 1 { break; } }");
    assert_eq!(
        check_body("break; continue;"),
        [
            "'break' outside of a loop, 'do' or 'switch'",
            "'continue' outside of a loop"
        ]
    );
    assert_eq!(
        check_body("do { continue; }"),
        ["'continue' outside of a loop"]
    );
    assert_eq!(
        check_body("switch p { case 1 { continue; } }"),
        ["'continue' outside of a loop"]
    );
    ok_body("loop { switch p { case 1 { continue; } } }");
}

#[test]
fn for_binding_is_scoped_to_the_body() {
    assert_eq!(
        check_body("for x in numbers { x; } x;"),
        ["Undefined name 'x'"]
    );
}

#[test]
fn do_while_condition_cannot_see_body_variables() {
    assert_eq!(
        check_body("do { let again = true; } while (again);"),
        ["Undefined name 'again'"]
    );
}

// -----------------------------------------------------------------------------
// Variables and assignment
// -----------------------------------------------------------------------------

#[test]
fn assignment_targets() {
    ok_body(
        "let a = 1; a = 2; a += 3; let b: s32[4]{0}; b[0] = 1; *point = Point { x: 1, y: 2 }; point.x = 1;",
    );
    assert_eq!(
        check_body("const A = 1; A = 2; A += 1;"),
        [
            "Cannot assign to constant 'A'",
            "Cannot assign to constant 'A'"
        ]
    );
    assert_eq!(
        check_body("1 = 2; f = 3; p + 1 = 2;"),
        [
            "Invalid assignment target",
            "Invalid assignment target",
            "Invalid assignment target"
        ]
    );
}

#[test]
fn constants_need_a_value() {
    assert_eq!(check_body("const A: s32;"), ["Constant 'A' needs a value"]);
    ok_body("let a: s32;");
}

#[test]
fn at_only_in_initializers() {
    ok_body("let a: u8[4]{0} = { @ };");
    assert_eq!(check_body("@;"), ["'@' outside of a 'let' initializer"]);
    assert_eq!(
        check_body("let a = 1; a = @;"),
        ["'@' outside of a 'let' initializer"]
    );
}

// -----------------------------------------------------------------------------
// Structs
// -----------------------------------------------------------------------------

const POINT: &str = "struct Point { x: f32, y: f32 }";

#[test]
fn struct_literals() {
    ok(&format!(
        "{POINT} fn f() {{ let a = Point {{ x: 1.0, y: 2.0 }}; }}"
    ));
    assert_eq!(
        check(&format!(
            "{POINT} fn f() {{ Point {{ x: 1.0, z: 2.0, x: 3.0 }}; }}"
        )),
        [
            "stdin:1:69: Duplicate field 'x'",
            "stdin:1:61: 'Point' has no field 'z'",
            "stdin:1:42: Missing fields in 'Point': y"
        ]
    );
    assert_eq!(check_body("p { x: 1 };"), ["'p' is not a struct"]);
    assert_eq!(check_body("Nope { x: 1 };"), ["Undefined name 'Nope'"]);
}

#[test]
fn struct_declarations() {
    assert_eq!(
        check("struct A { x: s32, x: f32, y: Nope }"),
        [
            "stdin:1:1: Duplicate field 'x'",
            "stdin:1:1: Undefined name 'Nope'"
        ]
    );
    ok("struct Node { value: s32, next: *Node }");
}

// -----------------------------------------------------------------------------
// Enums
// -----------------------------------------------------------------------------

const SHAPE: &str = "enum Shape { Circle(f32), Rect { w: f32, h: f32 }, Empty }";

#[test]
fn enum_declarations() {
    ok("enum Flag: u8 { Read = 1, Write = 2 }");
    assert_eq!(
        check("enum A { X, Y, X }"),
        ["stdin:1:1: Duplicate variant 'X'"]
    );
    assert_eq!(
        check("enum A: f32 { X }"),
        ["stdin:1:1: Enum backing type must be an integer type, not 'f32'"]
    );
    assert_eq!(
        check("enum A { X(Nope), Y { a: s32, a: s32 } }"),
        [
            "stdin:1:1: Undefined name 'Nope'",
            "stdin:1:1: Duplicate field 'a'"
        ]
    );
}

#[test]
fn variant_construction() {
    ok(&format!(
        "{SHAPE} fn f() {{ Shape::Circle(1.0); Shape::Rect {{ w: 1.0, h: 2.0 }}; Shape::Empty; }}"
    ));
}

#[test]
fn variant_construction_errors() {
    let errors = |body: &str| check(&format!("{SHAPE}\nfn f() {{ {body} }}"));

    assert_eq!(
        errors("Shape::Square;"),
        ["stdin:2:10: 'Shape' has no member 'Square'"]
    );
    assert_eq!(
        errors("Shape::Circle(1.0, 2.0);"),
        ["stdin:2:10: 'Shape::Circle' takes 1 argument, but 2 were given"]
    );
    assert_eq!(
        errors("Shape::Circle;"),
        ["stdin:2:10: 'Shape::Circle' has positional fields: write 'Shape::Circle(...)'"]
    );
    assert_eq!(
        errors("Shape::Empty();"),
        ["stdin:2:10: 'Shape::Empty' has no fields: write 'Shape::Empty'"]
    );
    assert_eq!(
        errors("Shape::Rect(1.0, 2.0);"),
        ["stdin:2:10: 'Shape::Rect' has named fields: write 'Shape::Rect { ... }'"]
    );
    assert_eq!(
        errors("Shape::Circle { r: 1.0 };"),
        ["stdin:2:10: 'Shape::Circle' has positional fields: write 'Shape::Circle(...)'"]
    );
    assert_eq!(
        errors("Shape::Rect { w: 1.0 };"),
        ["stdin:2:10: Missing fields in 'Shape::Rect': h"]
    );
}

// -----------------------------------------------------------------------------
// Switch and patterns
// -----------------------------------------------------------------------------

/// Checks `cases` in a switch over a `Shape`, returning just the messages.
fn check_cases(cases: &str) -> Vec<String> {
    check(&format!(
        "{SHAPE}\nfn f(s: Shape) {{ switch s {{ {cases} }} }}"
    ))
    .into_iter()
    .map(|error| error.split(": ").skip(1).collect::<Vec<_>>().join(": "))
    .collect()
}

#[test]
fn pattern_bindings_are_scoped_to_their_case() {
    let errors = check_cases(
        "case Shape::Circle(r) { r; break; }
         case Shape::Rect { w, h: height } { w * height; r; break; }
         default { w; }",
    );
    assert_eq!(errors, ["Undefined name 'r'", "Undefined name 'w'"]);
}

#[test]
fn value_patterns_are_checked() {
    assert_eq!(
        check_cases("case Shape::Empty {} case LIMIT {}"),
        ["Undefined name 'LIMIT'"]
    );
    assert_eq!(
        check_cases("case Shape::Circle {}"),
        ["'Shape::Circle' has positional fields: write 'Shape::Circle(...)'"]
    );
}

#[test]
fn variant_pattern_errors() {
    assert_eq!(
        check_cases("case Shape::Circle(a, b) { break; }"),
        ["'Shape::Circle' has 1 field, but the pattern has 2"]
    );
    assert_eq!(
        check_cases("case Shape::Rect(a, b) { break; }"),
        ["'Shape::Rect' has named fields: write 'Shape::Rect { ... }'"]
    );
    assert_eq!(
        check_cases("case Shape::Circle { r } { break; }"),
        ["'Shape::Circle' has positional fields: write 'Shape::Circle(...)'"]
    );
    assert_eq!(
        check_cases("case Shape::Rect { w, d } { break; }"),
        ["'Shape::Rect' has no field 'd'"]
    );
    assert_eq!(
        check_cases("case Shape::Square(a) { break; }"),
        ["'Shape' has no member 'Square'"]
    );
    assert_eq!(
        check(&format!(
            "{SHAPE} struct P {{ x: s32 }} fn f(s: Shape) {{ switch s {{ case P {{ x }} {{}} }} }}"
        ))
        .len(),
        1
    );
}

#[test]
fn struct_patterns_may_leave_fields_out() {
    assert_eq!(
        check_cases("case Shape::Rect { w } { w; break; }"),
        Vec::<String>::new()
    );
}

#[test]
fn duplicate_bindings() {
    let errors = check(&format!(
        "{SHAPE}\nenum Pair {{ Of(s32, s32) }}\nfn f(p: Pair) {{ switch p {{ case Pair::Of(a, a) {{ break; }} }} }}"
    ));
    assert_eq!(
        errors,
        ["stdin:3:45: 'a' is already declared in this scope"]
    );
}

#[test]
fn fallthrough_into_bindings() {
    assert_eq!(
        check_cases("case Shape::Empty { } case Shape::Circle(r) { r; }"),
        [
            "The previous case falls through into this one, but this case binds names; end the previous case with 'break' or 'return'"
        ]
    );
    // Ending the previous case with a jump avoids it
    assert_eq!(
        check_cases("case Shape::Empty { break; } case Shape::Circle(r) { r; }"),
        Vec::<String>::new()
    );
    // Falling through into cases that bind nothing is fine
    assert_eq!(
        check_cases("case Shape::Empty {} case Shape::Circle(_) {} default {}"),
        Vec::<String>::new()
    );
}

#[test]
fn errors_are_all_reported() {
    assert_eq!(
        check_body("a; b; break;"),
        [
            "Undefined name 'a'",
            "Undefined name 'b'",
            "'break' outside of a loop, 'do' or 'switch'"
        ]
    );
}

// -----------------------------------------------------------------------------
// Types: literals and conversions
// -----------------------------------------------------------------------------

/// Checks `source`, returning just the messages.
fn messages(source: &str) -> Vec<String> {
    check(source)
        .into_iter()
        .map(|error| error.split(": ").skip(1).collect::<Vec<_>>().join(": "))
        .collect()
}

#[test]
fn literals_take_the_expected_type() {
    ok_body(
        "let a: u8 = 255; let b: s8 = -128; let c: f32 = 1.5; let d: f32 = 2;
         let e: u64 = 0xFFFFFFFFFFFFFFFF;",
    );
}

#[test]
fn literals_must_fit() {
    assert_eq!(check_body("let a: u8 = 256;"), ["256 does not fit in 'u8'"]);
    assert_eq!(check_body("let a: u8 = -1;"), ["-1 does not fit in 'u8'"]);
    assert_eq!(check_body("let a = 300u8;"), ["300 does not fit in 'u8'"]);
    assert_eq!(
        check_body("let a: s32 = 0xFFFFFFFF;"),
        ["4294967295 does not fit in 's32'"]
    );
    assert_eq!(
        check_body("let a: u32 = 1.5;"),
        ["Expected 'u32', found 'float'; use 'as u32' to convert"]
    );
}

#[test]
fn literals_default_to_s32_and_f64() {
    ok_body("let a = 2; let b: s64 = a; let c = 1.5; let d: f64 = c;");
    assert_eq!(
        check_body("let a = 2; let b: u32 = a; let c = 1.5; let d: f32 = c;"),
        [
            "Expected 'u32', found 's32'; use 'as u32' to convert",
            "Expected 'f32', found 'f64'; use 'as f32' to convert"
        ]
    );
}

#[test]
fn lossless_widening_is_implicit() {
    ok_body(
        "let a: u8 = 1; let b: u16 = a; let c: s16 = a; let d: u64 = a; let e: usize = a;
         let f: s8 = 1; let g: s64 = f; let h: f32 = 1.0; let i: f64 = h;
         let j: usize = 1; let k: u64 = j;",
    );
}

#[test]
fn narrowing_needs_a_cast() {
    assert_eq!(
        check_body(
            "let a: s8 = 1; let b: u16 = a; let c: u32 = 1; let d: u8 = c; let e: s32 = c;
             let f: u64 = 1; let g: usize = f; let h: f64 = 1.0; let i: f32 = h;"
        ),
        [
            "Expected 'u16', found 's8'; use 'as u16' to convert",
            "Expected 'u8', found 'u32'; use 'as u8' to convert",
            "Expected 's32', found 'u32'; use 'as s32' to convert",
            "Expected 'usize', found 'u64'; use 'as usize' to convert",
            "Expected 'f32', found 'f64'; use 'as f32' to convert"
        ]
    );
}

#[test]
fn casts() {
    ok_body(
        "let x: u32 = 300; let y: u8 = x as u8; let f = 1.5 as s32; let t = true as u8;
         let n = -1 as u64; let fl = p as f64;
         let r = &p as *u8; let address = point as usize; let back = address as *Point;",
    );
    assert_eq!(
        check_body("1 as bool; Point { x: 1, y: 2 } as s32; flag as f32;"),
        [
            "Cannot cast 'integer' to 'bool'",
            "Cannot cast 'Point' to 's32'",
            "Cannot cast 'bool' to 'f32'"
        ]
    );
}

#[test]
fn pointers_and_references() {
    ok_body("let r: *s32 = &p; let s: &s32 = &p; let v: s32 = *s; let w: s32 = *r;");
    assert_eq!(
        check_body("let r: *s32 = &p; let s: &s32 = r;"),
        ["Expected '&s32', found '*s32'"]
    );
}

// -----------------------------------------------------------------------------
// Types: operators
// -----------------------------------------------------------------------------

#[test]
fn binary_operators() {
    ok_body(
        "let a: u8 = 1; let b: u32 = 2; let c: u32 = a + b; let d = 1 + 2.5; let e: f64 = d;
         let s = b << 2; let t: u32 = s; let lt: bool = a < b && flag || p == 3;",
    );
}

#[test]
fn binary_operator_errors() {
    assert_eq!(
        check_body("let s: s8 = 1; let b: u32 = 2; s + b;"),
        ["Cannot apply '+' to 's8' and 'u32'"]
    );
    assert_eq!(
        check_body("let a: u8 = 1; let e: u8 = a + 300;"),
        ["300 does not fit in 'u8'"]
    );
    assert_eq!(
        check_body("p && true; 1.5 & 2; flag + 1;"),
        [
            "Cannot apply '&&' to 's32' and 'bool'",
            "Cannot apply '&' to 'float' and 'integer'",
            "Cannot apply '+' to 'bool' and 'integer'"
        ]
    );
    assert_eq!(
        check_body("Point { x: 1, y: 2 } == Point { x: 1, y: 2 };"),
        ["Cannot apply '==' to 'Point' and 'Point'"]
    );
    assert_eq!(
        check_body("let x: s32 = flag == true;"),
        ["Expected 's32', found 'bool'"]
    );
}

#[test]
fn compound_assignment_types() {
    ok_body("let a: u32 = 1; a += 2; a <<= 1; let b: u8 = 1; a += b;");
    assert_eq!(
        check_body("let a: u8 = 1; let b: u32 = 2; a += b;"),
        ["Expected 'u8', found 'u32'; use 'as u8' to convert"]
    );
}

#[test]
fn unary_operators() {
    ok_body("let w: s32 = -p; let n: bool = !flag; let f = -1.5;");
    assert_eq!(
        check_body("let u: u32 = 1; -u; !1; *p;"),
        [
            "Cannot apply '-' to 'u32'",
            "Cannot apply '!' to 'integer'",
            "Cannot apply '*' to 's32'"
        ]
    );
}

#[test]
fn conditions_must_be_bool() {
    assert_eq!(
        check_body("if (p) {} while (1) {} do {} while (point);"),
        [
            "Expected 'bool', found 's32'",
            "Expected 'bool', found 'integer'",
            "Expected 'bool', found '*Point'"
        ]
    );
}

#[test]
fn if_expressions_have_the_branches_type() {
    ok_body(
        "let x: s32 = if (flag) { 1 } else { 2 }; let y: u8 = if (flag) { 1 } else { return; };",
    );
    assert_eq!(
        check_body("let x = if (flag) { 1 } else { true };"),
        ["The value given to 'x' has no type"]
    );
}

// -----------------------------------------------------------------------------
// Types: functions
// -----------------------------------------------------------------------------

#[test]
fn argument_types() {
    assert_eq!(
        check_body(
            "fn g(a: u8) {} fn h() -> u8 { 1 }
             g(300); g(flag); g(p); let x: s32 = g(1); let y: s32 = h();"
        ),
        [
            "300 does not fit in 'u8'",
            "Expected 'u8', found 'bool'",
            "Expected 'u8', found 's32'; use 'as u8' to convert",
            "Expected 's32', found 'void'"
        ]
    );
}

#[test]
fn builtin_functions_are_unchecked() {
    ok("import std::printf; fn f() { let x: s32 = printf(\"hi\", 1, true); }");
}

#[test]
fn return_types() {
    assert_eq!(
        check("fn f() -> u8 { 300 }"),
        ["stdin:1:16: 300 does not fit in 'u8'"]
    );
    assert_eq!(
        check("fn f() -> u8 { return 1.5; }"),
        ["stdin:1:23: Expected 'u8', found 'float'; use 'as u8' to convert"]
    );
    assert_eq!(
        check("fn f() -> s32 { true }"),
        ["stdin:1:17: Expected 's32', found 'bool'"]
    );
    ok("fn f() -> s32 { let x = 1; x }");
}

#[test]
fn every_path_must_return() {
    assert_eq!(
        check("fn f() -> s32 { if (true) { return 1; } }"),
        ["stdin:1:1: 'f' must return 's32' on every path"]
    );
    // The loop body may never run
    assert_eq!(
        check("fn f() -> s32 { while (true) { return 1; } }"),
        ["stdin:1:1: 'f' must return 's32' on every path"]
    );
    ok("fn f() -> s32 { if (true) { return 1; } else { return 2; } }");
    ok("fn f() -> s32 { loop { return 1; } }");
    ok("fn f(x: s32) -> s32 { switch x { case 1 { return 1; } default { return 2; } } }");
}

#[test]
fn generator_types() {
    ok(
        "fn g() -> iter u8 { yield 1; }
         fn f() { for x in g() { let y: u8 = x; let z: u16 = x; } let it: iter u8 = g(); for x in it {} }",
    );
    assert_eq!(
        check("fn g() -> iter u8 { yield 300; }"),
        ["stdin:1:27: 300 does not fit in 'u8'"]
    );
    assert_eq!(
        messages("fn g() -> iter u8 { yield 1; } fn f() { for x in g() { let y: s8 = x; } }"),
        ["Expected 's8', found 'u8'; use 'as s8' to convert"]
    );
    assert_eq!(
        check_body("for x in 5 {}"),
        ["Cannot loop over 'integer'; 'for' needs an iterator ('iter T')"]
    );
}

// -----------------------------------------------------------------------------
// Types: structs, arrays and strings
// -----------------------------------------------------------------------------

#[test]
fn field_types() {
    ok_body(
        "let x: s32 = point.x; let q = Point { x: 1, y: 2 }; let y: s32 = q.y; let r = &q; r.x;",
    );
    assert_eq!(
        check_body("point.z; p.x; Point { x: 1.5, y: flag };"),
        [
            "'Point' has no field 'z'",
            "'s32' has no field 'x'",
            "Expected 's32', found 'float'; use 'as s32' to convert",
            "Expected 's32', found 'bool'"
        ]
    );
}

#[test]
fn arrays_and_indexing() {
    ok_body(
        "let arr: u8[4]{0}; let v: u8 = arr[1]; let w: u8 = arr[p];
         let grid: u8[4][8]{0}; let row: u8[8] = grid[0]; let cell: u8 = grid[3][7];",
    );
    assert_eq!(
        check_body("let arr: u8[4]{0}; arr[1.5]; p[0]; let a: u16[4] = arr;"),
        [
            "Expected 'integer', found 'float'",
            "Cannot index into 's32'",
            "Expected 'u16[4]', found 'u8[4]'"
        ]
    );
}

#[test]
fn array_initializers_are_typed() {
    assert_eq!(
        check_body("let arr: u8[4]{300};"),
        ["300 does not fit in 'u8'"]
    );
}

#[test]
fn strings_are_null_terminated_arrays() {
    ok_body("let s: u8[6] = \"hello\"; let t: u8[?] = \"hello\"; let c: u8 = \"hi\"[0];");
    ok("fn show(s: u8[?]) {} fn f() { show(\"hi\"); }");
    assert_eq!(
        check_body("let s: u8[5] = \"hello\";"),
        ["Expected 'u8[5]', found 'u8[6]'"]
    );
}

#[test]
fn at_has_the_let_type() {
    ok_body("let a: u8[4]{0} = { @ }; let b: s32 = { let c = 1; @ };");
    assert_eq!(
        check_body("let a = { @ };"),
        ["'@' needs its 'let' to declare a type"]
    );
    // `@` is the innermost `let` being initialized: here `b`, not `a`
    ok_body("let a: s16 = { let b: u8 = @; b };");
    assert_eq!(
        check_body("let a: u8 = { let b: s32 = @; b };"),
        ["Expected 'u8', found 's32'; use 'as u8' to convert"]
    );
}

#[test]
fn let_needs_a_type() {
    assert_eq!(
        check_body("let a;"),
        ["Cannot infer a type for 'a'; give it a type or a value"]
    );
    assert_eq!(
        check_body("fn g() {} let a = g();"),
        ["The value given to 'a' has no type"]
    );
}

// -----------------------------------------------------------------------------
// Types: enums and patterns
// -----------------------------------------------------------------------------

#[test]
fn enum_types() {
    ok(&format!(
        "enum Color {{ Red, Green }}
         {SHAPE}
         fn f(s: Shape, c: Color) -> f32 {{
             let same: bool = c == Color::Red;
             let n: u8 = Color::Green as u8;
             switch s {{
                 case Shape::Circle(r) {{ return r; }}
                 case Shape::Rect {{ w, h }} {{ return w * h; }}
                 default {{ return 0.0f32; }}
             }}
         }}"
    ));
}

#[test]
fn pattern_types() {
    assert_eq!(
        messages(&format!(
            "{SHAPE} fn f(s: Shape) {{ switch s {{ case Shape::Circle(r) {{ let y: s32 = r; }} }} }}"
        )),
        ["Expected 's32', found 'f32'; use 'as s32' to convert"]
    );
    assert_eq!(
        messages(&format!(
            "enum Color {{ Red }} {SHAPE} fn f(c: Color) {{ switch c {{ case Shape::Circle(r) {{}} }} }}"
        )),
        ["'Shape::Circle' cannot match a value of type 'Color'"]
    );
    assert_eq!(
        check_body("switch p { case 1.5 {} }"),
        ["Expected 's32', found 'float'; use 'as s32' to convert"]
    );
}

#[test]
fn variant_field_types() {
    assert_eq!(
        messages(&format!(
            "{SHAPE} fn f() {{ Shape::Circle(true); Shape::Rect {{ w: 1, h: true }}; }}"
        )),
        [
            "Expected 'f32', found 'bool'",
            "Expected 'f32', found 'bool'"
        ]
    );
}

#[test]
fn only_fieldless_enums_cast_to_integers() {
    assert_eq!(
        messages(&format!("{SHAPE} fn f(s: Shape) {{ s as u8; }}")),
        ["Cannot cast 'Shape' to 'u8'"]
    );
}

#[test]
fn enum_values_are_typed() {
    assert_eq!(
        check("enum A: u8 { X = 256 }"),
        ["stdin:1:18: 256 does not fit in 'u8'"]
    );
    assert_eq!(
        check("enum A { X = 1.5 }"),
        ["stdin:1:14: Expected 'integer', found 'float'"]
    );
}

// -----------------------------------------------------------------------------
// Definite assignment
// -----------------------------------------------------------------------------

const UNASSIGNED: &str = "'a' may be used before it has been assigned";

#[test]
fn reading_before_assignment() {
    assert_eq!(check_body("let a: s32; a;"), [UNASSIGNED]);
    assert_eq!(check_body("let a: s32; a += 1;"), [UNASSIGNED]);
    ok_body("let a: s32; a = 1; a;");
    // Reported once, not at every read
    assert_eq!(check_body("let a: s32; a; a;"), [UNASSIGNED]);
}

#[test]
fn assignment_through_branches() {
    assert_eq!(
        check_body("let a: s32; if (flag) { a = 1; } a;"),
        [UNASSIGNED]
    );
    ok_body("let a: s32; if (flag) { a = 1; } else { a = 2; } a;");
    ok_body("let a: s32; if (flag) { a = 1; } else { return; } a;");
    ok_body("let n: usize; if (flag) { n = 1; } else if (p > 0) { n = 2; } else { return; } n;");
}

#[test]
fn assignment_through_loops() {
    // The body may not run
    assert_eq!(
        check_body("let a: s32; while (flag) { a = 1; } a;"),
        [UNASSIGNED]
    );
    assert_eq!(
        check_body("let a: s32; for x in numbers { a = 1; } a;"),
        [UNASSIGNED]
    );
    // `loop` is only left by `break`
    ok_body("let a: s32; loop { a = 1; break; } a;");
    assert_eq!(
        check_body("let a: s32; loop { if (flag) { break; } a = 1; } a;"),
        [UNASSIGNED]
    );
    // `do` runs at least once, but `continue` skips the rest of the body
    ok_body("let a: s32; do { a = 1; } while (flag); a;");
    assert_eq!(
        check_body("let a: s32; do { if (flag) { continue; } a = 1; } while (flag); a;"),
        [UNASSIGNED]
    );
}

#[test]
fn assignment_through_switch() {
    ok_body("let a: s32; switch p { case 1 { a = 1; break; } default { a = 2; } } a;");
    // Without a default, no case may run
    assert_eq!(
        check_body("let a: s32; switch p { case 1 { a = 1; break; } } a;"),
        [UNASSIGNED]
    );
}

#[test]
fn globals_start_zeroed() {
    ok("let g: s32; fn f() -> s32 { g }");
}

#[test]
fn functions_cannot_capture_variables() {
    let captured = "'x' belongs to an enclosing function; functions cannot capture variables";
    assert_eq!(
        messages("fn outer() { let x = 1; fn inner() { x; } }"),
        [captured]
    );
    assert_eq!(
        messages("fn outer() { let x = 1; fn inner() { x = 2; } }"),
        [captured]
    );
    ok("let g = 1; fn f() -> s32 { g }");
}

// -----------------------------------------------------------------------------
// C-style for and increments
// -----------------------------------------------------------------------------

#[test]
fn increment_types() {
    ok_body("let i: u8 = 0; i++; i--; let arr: s32[4]{0}; arr[1]++; point.x--;");
    assert_eq!(
        check_body("let f = 1.5; f++; flag--;"),
        ["Cannot apply '++' to 'f64'", "Cannot apply '--' to 'bool'"]
    );
    assert_eq!(
        check_body("const C = 1; C++;"),
        ["Cannot assign to constant 'C'"]
    );
    assert_eq!(check_body("let a: s32; a++;"), [UNASSIGNED]);
}

#[test]
fn for_clause_types_and_scopes() {
    ok_body(
        "let arr: u8[4]{0};
         for (let i: usize = 0; i < 4; i++; let item = arr[i]) { let x: u8 = item; i; }",
    );
    assert_eq!(
        check_body("for (let i = 0; i < 4; i++) {} i;"),
        ["Undefined name 'i'"]
    );
    // `each` is only visible in the body, not the step
    assert_eq!(
        check_body("for (let i = 0; i < 4; item++; let item = i) {}"),
        ["Undefined name 'item'"]
    );
    assert_eq!(
        check_body("for (let i = 0; i; i++) {}"),
        ["Expected 'bool', found 's32'"]
    );
}

#[test]
fn for_clause_with_user_functions() {
    ok("struct Bytes { data: u8[16], len: usize }
        fn bytes_len(b: &Bytes) -> usize { b.len }
        fn bytes_get(b: &Bytes, i: usize) -> u8 { b.data[i] }
        fn sum(b: &Bytes) -> u32 {
            let total: u32 = 0;
            for (let i: usize = 0; i < bytes_len(b); i++; let byte = bytes_get(b, i)) {
                total += byte;
            }
            total
        }");
}

#[test]
fn for_clause_flow() {
    // The condition may fail straight away
    assert_eq!(
        check_body("let a: s32; for (let i = 0; i < 4; i++) { a = 1; } a;"),
        [UNASSIGNED]
    );
    // Without a condition, only `break` leaves
    ok_body("let a: s32; for (;;) { a = 1; break; } a;");
    ok("fn f() -> s32 { for (;;) { return 1; } }");
    ok_body("for (let i = 0; i < 4; i++) { if (flag) { continue; } break; }");
}
