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

Numeric literals carry explicit size/base information: `2u32`, `0x44`,
`0b110110`, `0673` (octal), `2.5f32`, `2e10`, etc. See
[`examples/basic.pk`](examples/basic.pk) and
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

Errors (with file/line/column) go to stderr. The parser recovers after an
error, so it reports every bad statement in one run.

## Syntax overview

Everything is an expression, including declarations and control flow. A
block's last expression, if it has no trailing `;`, is the block's value.

| Construct | Example |
|---|---|
| Import | `import std::printf;` `import std::{printf, strcat};` `import types::*;` |
| Function | `fn add(a: s32, b: s32) -> s32 { a + b }` (return type optional) |
| Variable | `let x: u32 = 1;` `const y = 2.5;` (type and value optional) |
| Struct / enum | `struct Point { x: f32, y: f32 }` `enum Color { Red, Green }` |
| If | `if (a) { ... } else if (b) { ... } else { ... }` |
| Switch | `switch (x) { case (1) { ...; break; } default { ... } }` |
| For | `for x in xs { ... }` or `for (x in xs) { ... }` |
| Loop | `loop { ... }` (repeats until `break`) |
| Do / do-while | `do { ... }` (runs once) or `do { ... } while (cond);` |
| Jumps | `return;` `return x;` `break;` `continue;` |
| Block | `{ strcat(@, "hi"); @ }`, where `@` is the value being initialized |
| Literals | `42` `0xffu8` `2.5f32` `"text"` `true` `false` |

Types are named (`s32`, `types::Foo`), pointers (`*T`), references (`&T`) or
arrays (`T[N]`, or `T[?]` for unknown size). In a `let`, an array type may
carry an initializer: `u8[0x40]{0}`. Array suffixes wrap the type to their
left, so `u8[4][8]` is 8 elements of `u8[4]` (the reverse of C's `u8 x[4][8]`).
Likewise `*u8[4]` is an array of four pointers; write `*(u8[4])` for a pointer
to an array.

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
| `+` `-` | left |
| `*` `/` `%` | left |
| unary `-` `!` `*` `&` | prefix |
| call `f(x)`, index `a[i]`, member `a.b` | postfix |

## Project layout

```
src/
  tokenizer/   Lexer: tokens.rs (token/keyword/punctuation types),
               mod.rs (Tokenizer), errors.rs (lexer error kinds), utils.rs
  parser/      Parser: mod.rs (recursive-descent Parser),
               expression.rs (AST node types),
               typeinfo.rs, errors.rs
  error.rs     Shared GenericError<K> positioned error type
  lib.rs       Library root
  main.rs      CLI entry point
examples/      Sample .pk source files
tests/         Integration tests (tokenizer.rs, parser.rs)
```

## Roadmap

- [x] Tokenizer with structured, position-aware errors
- [x] Parser / AST with error recovery
- [ ] Type checking
- [ ] Code generation

## License

No license has been chosen yet.
