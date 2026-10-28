# minlang

A small, C-like systems language with explicitly sized integer/float types —
and the Rust toolchain that tokenizes and parses it.

> **Status: early work-in-progress.** The tokenizer and parser handle
> everything in the examples and produce a full AST with structured,
> position-aware errors. There is no type checker or code generator yet.
> Expect breaking changes.

## What it looks like

`.pk` source files look roughly like this ([`examples/hello.pk`](examples/hello.pk)):

```rust
import std::{printf, strcat};
import types::{*};

fn main(argc: s32, argv: u8[?][?], envp: u8[?][?]) -> s32 {
    let message: u8[0x40]{0} = {
        strcat(@, "Hello, ");
        strcat(@, "World!\n");
        @
    };

    let xor: u32 = 100u32 ^ 44u32;
    let float: f32 = 1.5f32;

    switch (float) {
        case (1.5) {
            printf("good\n");
            break;
        }
        default {
            printf("WHAT\n");
        }
    }

    printf("%s", message);
    0
}
```

A `case` pattern can be a constant (`1.5`, `-1`, `Color::Red`, `LIMIT + 1`), `_`
to match anything, or a variant pattern. Inside a variant pattern a bare name
binds that part of the value: `case Shape::Circle(radius)` makes `radius`
available in the case body.

Numeric literals carry explicit size/base information: `2u32`, `0x44`,
`0b110110`, `0673` (octal), `2.5f32`, `2e10`, etc. See
[`examples/basic.pk`](examples/basic.pk),
[`examples/structs.pk`](examples/structs.pk),
[`examples/iterators.pk`](examples/iterators.pk) and
[`examples/errors.pk`](examples/errors.pk) (a catalog of intentionally
invalid literals, used to exercise the tokenizer's error reporting).

## Getting started

Requires a Rust toolchain (edition 2024).

Run the test suite:

```bash
cargo test
```

Lint:

```bash
cargo clippy --all-targets
```

Tokenize a file:

```bash
cargo run -- examples/hello.pk
```

This prints the token stream as JSON to stdout. Pass `--ast` to parse the
file and print the AST instead:

```bash
cargo run -- --ast examples/hello.pk
```

Pass `--check` to also run the checker, which reports name, type and
assignment errors (see [Checking](#checking)):

```bash
cargo run -- --check examples/hello.pk
```

Errors (with file/line/column, counted from 1) go to stderr. The parser and
checker recover after an error, so they report every problem in one run.

## Syntax overview

Everything is an expression, including declarations and control flow. A
block's last expression, if it has no trailing `;`, is the block's value.

| Construct | Example |
|---|---|
| Import | `import std::printf;` `import std::{printf, strcat};` `import types::*;` |
| Function | `fn add(a: s32, b: s32) -> s32 { a + b }` (return type optional) |
| Variable | `let x: u32 = 1;` `const y = 2.5;` (type and value optional) |
| Struct | `struct Point { x: f32, y: f32 }`, built with `Point { x: 1.0, y: 2.0 }` |
| Enum | `enum Color { Red, Green }` or `enum Flag: u8 { Read = 1, Write = 2 }`, used as `Color::Red` |
| Enum with data | `enum Shape { Circle(f32), Rect { w: f32, h: f32 }, Empty }`, built as `Shape::Circle(1.0)` or `Shape::Rect { w: 1.0, h: 2.0 }` |
| Generator | `fn evens(n: s32) -> iter s32 { ...; yield i; }`, looped over with `for x in evens(10) { ... }` |
| If | `if (a) { ... } else if (b) { ... } else { ... }` |
| Switch | `switch (x) { case (1) { ...; break; } default { ... } }` (cases fall through without `break`, as in C) |
| Patterns | `case Shape::Circle(r) { ... }` `case Shape::Rect { w, h: height } { ... }` `case _ { ... }` |
| For | `for x in xs { ... }` or `for (x in xs) { ... }` |
| C-style for | `for (let i: usize = 0; i < n; i++) { ... }`, with any clause optional: `for (;;)` |
| For with item | `for (let i: usize = 0; i < len(c); i++; let item = get(c, i)) { ... }` |
| Increment | `i++;` `i--;` (statements only, not expressions) |
| While | `while (cond) { ... }` |
| Loop | `loop { ... }` (repeats until `break`) |
| Do / do-while | `do { ... }` (runs once) or `do { ... } while (cond);` |
| Jumps | `return;` `return x;` `break;` `continue;` |
| Assignment | `x = 1;` and compound forms `+=` `-=` `*=` `/=` `%=` `&=` `\|=` `^=` `<<=` `>>=` |
| Cast | `x as u8` |
| Block | `{ strcat(@, "hi"); @ }`, where `@` is the value being initialized |
| Literals | `42` `0xffu8` `2.5f32` `"text"` `true` `false` |

The built-in types are `u8` `u16` `u32` `u64` `usize` `s8` `s16` `s32` `s64`
`f32` `f64` and `bool`; they need no import. Types are named (`s32`, `types::Foo`), pointers (`*T`), references (`&T`),
iterators (`iter T`, returned by generators) or arrays (`T[N]`, or `T[?]` for
unknown size). `iter` takes the whole type to its right, so `iter u8[4]` yields
arrays; write `(iter u8)[4]` for an array of iterators. In a `let`, an array type may
carry an initializer: `u8[0x40]{0}`. Dimensions read as in C, so `u8[4][8]`
is a 4×8 array: 4 elements, each a `u8[8]`. `*u8[4]` is an array of four
pointers; write `*(u8[4])` for a pointer to an array.

Operator precedence, lowest to highest:

| Operators | Associativity |
|---|---|
| `=` | right |
| `\|\|` | left |
| `&&` | left |
| `\|` | left |
| `^` | left |
| `&` | left |
| `==` `!=` | left |
| `<` `>` `<=` `>=` | left |
| `<<` `>>` | left |
| `+` `-` | left |
| `*` `/` `%` | left |
| `as` | left |
| unary `-` `!` `*` `&` | prefix |
| call `f(x)`, index `a[i]`, member `a.b` | postfix |

A bare `do { ... }` directly followed by a `while` loop needs a `;` after its
block, or the `while` is read as the `do`'s condition.

In a C-style `for`, each pass runs the condition, then the optional fourth
clause, then the body, then the step; `continue` jumps to the step. The fourth
clause must be a `let`, and its variable is visible in the body only. It's
meant for containers with their own length and accessor functions, without
needing a generator.

## Checking

`--check` runs the checker after parsing. Besides undefined names and
misplaced constructs, it checks the type of every expression.

**Types of literals.** A number without a suffix takes the type expected where
it's used: `let a: u8 = 255;`, or `yield 0xFFFD;` in a generator of `u32`. It
must fit that type (`let a: u8 = 256;` is an error). Where nothing is
expected it defaults to `s32` or `f64`. A suffix fixes the type: `2u8`. A
string literal is a null-terminated `u8[N]`, where `N` counts the terminator,
so `"hello"` is a `u8[6]`. Any `u8[N]` can be passed where `u8[?]` is expected.

**Conversions.** Only conversions that can't lose information happen
implicitly: `u8` to `u32`, `u8` to `s16`, `s8` to `s64`, `u32` to `usize`,
`usize` to `u64`, `f32` to `f64`, and `&T` to `*T`. Everything else needs
`as`, which converts between any numbers, from `bool` or a field-less enum to
an integer, between pointers, and between pointers and `usize`. Operators
convert the narrower side to the wider (`u8 + u32` is a `u32`); if neither
side widens to the other (`s8 + u32`), it's an error.

**Assignment before use.** A variable declared without a value must be
assigned on every path before it's read. Paths through `if`, loops, `switch`,
`break`, `continue` and `return` are all followed:

```rust
let count: usize;
if (a < 0x80) { count = 1; } else if (a < 0xE0) { count = 2; } else { continue; }
i += count; // fine: every path that gets here assigned `count`
```

Global variables start out zeroed, so they're always considered assigned.

**Other type rules:**

- Conditions of `if`, `while` and `do ... while` must be `bool`.
- A function with a return type must return a value of that type on every
  path. A generator (`-> iter T`) yields `T` values and ends with a bare
  `return`.
- `for` loops over an `iter T`, binding each item as a `T`.
- `@` has the type declared by its `let`, so that `let` needs a type. It
  refers to the innermost `let` being initialized.
- `.field` and indexing look through `&T` and `*T`.
- `case` patterns must match the type being switched on, and the names they
  bind get the variant's field types.
- Functions can't use the local variables of the function around them (there
  are no closures), but they can use globals.

**Names and context** (checked as before):

- Undefined names, including enum variants and members of `std` and `types`.
  Functions, structs, enums and imports can be used before they're declared;
  `let` and `const` cannot.
- Clashing declarations in one scope (shadowing in an inner block is fine),
  and duplicate fields, variants, parameters and pattern bindings.
- Wrong argument or field counts, a variant built or matched in the wrong
  style (`Shape::Empty()`), and missing or unknown fields. Struct patterns may
  leave fields out.
- `break` outside a loop, `do` or `switch`; `continue` outside a loop;
  `yield` outside a generator; `@` outside a `let` initializer; anything other
  than declarations at the top level; assignments to constants.
- A `case` that binds names can't be reached by falling through from the case
  before it.

`std` contains `printf` and `strcat`; their arguments aren't checked and their
results have an unknown type. `types` contains the built-in types.

## Project layout

```
src/
  tokenizer/   Lexer: tokens.rs (token/keyword/punctuation types),
               mod.rs (Tokenizer), errors.rs (lexer error kinds), utils.rs
  parser/      Parser: mod.rs (recursive-descent Parser),
               expression.rs (AST node types),
               typeinfo.rs, errors.rs
  checker/     Checker: mod.rs (names, types and assignment checks),
               types.rs (types and conversion rules),
               builtins.rs (built-in types and modules), errors.rs
  error.rs     Shared GenericError<K> positioned error type
  lib.rs       Library root
  main.rs      CLI entry point
examples/      Sample .pk source files
tests/         Integration tests (tokenizer.rs, parser.rs, checker.rs)
```

## Roadmap

- [x] Tokenizer with structured, position-aware errors
- [x] Parser / AST with error recovery
- [x] Type checking
- [ ] Code generation

## License

No license has been chosen yet.
