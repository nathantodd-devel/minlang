# minlang

A small, C-like systems language with explicitly sized integer/float types —
and the Rust compiler that turns it into native executables through LLVM.

> **Status: early work-in-progress.** The whole pipeline works: lexing,
> parsing, type checking and LLVM IR generation, and every example compiles
> and runs. Expect breaking changes.

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
[`examples/iterators.pk`](examples/iterators.pk),
[`examples/modules`](examples/modules) (a program split across files) and
[`examples/errors.pk`](examples/errors.pk) (a catalog of intentionally
invalid literals, used to exercise the tokenizer's error reporting).

## Getting started

Requires a Rust toolchain (edition 2024), and `clang` on the `PATH` to build
executables. Install the `minlang` command with `cargo install --path .`, or
use `cargo run --` in its place below.

```bash
minlang run examples/hello.pk              # compile and run
minlang run examples/hello.pk -- a b       # pass arguments to the program
minlang build examples/hello.pk            # writes ./hello
minlang build -O2 -o hi examples/hello.pk  # optimized (-O0 to -O3), named hi
minlang build -c examples/hello.pk         # just an object file: ./hello.o
minlang check examples/hello.pk            # report errors without compiling
minlang emit llvm examples/hello.pk        # print the LLVM IR
minlang emit ast examples/hello.pk         # or the syntax tree, or `tokens`
```

`build` and `run` also compile and link the files the program imports (see
[Modules](#modules)). `run` exits with the program's exit code. `--time` prints how long each
compiler stage takes, and `minlang help <command>` describes each command's
options.

Errors show the line they're on, and the parser and checker recover after an
error, so every problem is reported in one run:

```text
error: 'count' may be used before it has been assigned
  --> demo.pk:15:14
   |
15 |         i += count;
   |              ^^^^^

error: could not compile `demo.pk` due to an error
```

They're colored when written to a terminal; `--color always|never` overrides
that, and so does setting `NO_COLOR`.

Run the tests and lints with:

```bash
cargo test
cargo clippy --all-targets --all-features
```

`cargo test` also builds and runs the programs in `tests/programs` and the
examples, comparing their output with the `.out` file next to each. Those
tests are skipped if clang isn't installed.

## Editor support

`minlang lsp` is a language server. It reports errors as you type, and
provides hover (types and signatures), go to definition, find references,
highlight of matching names, rename, the document outline, and completion of
keywords, types, names in scope, enum variants after `Enum::` and fields
after `value.`. Its semantic tokens color names by what they are, using the
checker: parameters, enum variants, struct fields, built-ins and so on.

For instant coloring before the server answers, there's a tree-sitter grammar
(`editors/tree-sitter-minlang`, used by Zed, Neovim and Helix) and a TextMate
grammar (used by VS Code).

All of them need `minlang` on the `PATH`:

```bash
cargo install --path .
```

**Zed:** run *zed: install dev extension* and pick `editors/zed`. Zed builds
the grammar from the commit named in `editors/zed/extension.toml`, so that
commit must contain `editors/tree-sitter-minlang`. For a local checkout, point
`repository` at `file:///path/to/minlang` and `rev` at a local commit.

**VS Code:** in `editors/vscode`, run `npm install`, then either open the
folder and press F5 to try it, or package it with
`npx @vscode/vsce package` and install the `.vsix`. The `minlang.server.path`
setting picks a different executable.

**Neovim (0.11+):** build the parser, then add the directory to the runtime
path. No plugins are needed:

```bash
editors/neovim/build-parser.sh
```

```lua
-- init.lua
vim.opt.rtp:prepend('/path/to/minlang/editors/neovim')
```

**Helix:** follow the comments in `editors/helix/languages.toml`.

The highlight queries are shared: Zed and Neovim link to
`editors/tree-sitter-minlang/queries`, and Helix's copy differs only in capture
names. `cargo test` checks that every keyword and built-in type is in each
grammar, and that the copies agree. When `tree-sitter` is installed it also
runs the grammar's tests and parses every example. After changing
`grammar.js`, run `tree-sitter generate` and `tree-sitter test` in
`editors/tree-sitter-minlang`.

## Syntax overview

Everything is an expression, including declarations and control flow. A
block's last expression, if it has no trailing `;`, is the block's value.

| Construct | Example |
|---|---|
| Import | `import std::printf;` `import std::{printf, strcat};` `import types::*;` `import geo;` (see [Modules](#modules)) |
| Public | `pub fn` `pub struct` `pub enum` `pub let` `pub const`: other files can import it |
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

## Modules

Every `.pk` file is a module, named after the file. A file imports another by
its path relative to the importing file, with `::` between directories:

```rust
// main.pk
import std::printf;
import shapes::geometry;                  // the module in shapes/geometry.pk
import shapes::geometry::{Point, area};   // or items from it
// import shapes::geometry::*;            // or all of its public items

fn main() -> s32 {
    let p: Point = Point { x: 1.0, y: 2.0 };
    printf("%f\n", area(geometry::Shape::Circle(p.x)));
    0
}
```

Only top-level declarations marked `pub` can be imported; everything else is
private to its file. `import shapes::geometry;` makes the module usable by
name, as `geometry::area` or `geometry::Shape::Circle`. Without it, only the
items named in an `import` are visible. `import a::b;` imports the item `b`
from `a.pk` if that file exists, and otherwise the module in `a/b.pk`. An item
reached through two imports is the same item, so a `Point` from `geometry`
passed through another module is still a `geometry::Point`. The built-in
`std` and `types` take precedence over files with those names.

**Compiling.** Each file is checked once, after the files it imports, and the
files importing it see only what its `pub` items look like. Each one compiles
to its own LLVM module: `pub` functions and globals get external linkage,
and the importing module declares them. `minlang build main.pk` compiles
every imported file and links them all; `minlang build -c` compiles just the
one file to an object file, so a program can be built piece by piece and
linked with clang:

```bash
minlang build -c shapes/geometry.pk     # writes geometry.o
minlang build -c main.pk                # writes main.o
clang main.o geometry.o -o main
```

`minlang emit llvm` prints the named file's module only. The language
server reads imported files from disk, so it sees their last saved contents.

**Limits:**

- Imports can't form a cycle, directly or not.
- Symbols are named after the file, not its directory, so two imported files
  can't have the same name (`a/geo.pk` and `b/geo.pk`). The build reports
  this rather than failing to link.
- A global set by code at startup (not to a constant) may not be set yet when
  another module's startup code reads it, since modules start up in link
  order.

## Checking

`minlang check`, and every command that compiles, runs the checker after
parsing. Besides undefined names and misplaced constructs, it checks the type
of every expression.

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

## Code generation

The compiler lowers the checked program to a backend-neutral, LLVM-shaped IR
(`src/codegen/ir.rs`). The text backend writes it as `.ll`, which clang
compiles. Lowering is written once, so an inkwell backend (building modules
through LLVM's API) can be added without touching it. The `inkwell` Cargo
feature compiles a stub of that backend; see `src/codegen/inkwell.rs`.

**How values are represented:**

| minlang | LLVM |
|---|---|
| `u8`…`u64`, `s8`…`s64`, `usize` | `i8`…`i64` (`usize` is 64-bit) |
| `f32`, `f64`, `bool` | `float`, `double`, `i1` |
| `T[N]` | `[N x T]`, a value that is copied on assignment |
| `T[?]`, `*T`, `&T` | `ptr`; `T[?]` points at the first element, like C |
| `"text"` | a private constant `[N x i8]` with a terminating zero |
| `struct` | a named struct type, e.g. `%"structs::Point"` |
| `enum` without data | its backing integer (`i32` by default) |
| `enum` with data | `{ tag, payload }`, the payload sized for the largest variant |
| `iter T` | a coroutine handle (`ptr`) |

**Generators** are LLVM coroutines. Calling one allocates its frame with
`malloc` and runs nothing until it's first looped over. Each `yield` stores
the value in the coroutine's promise and suspends. A `for x in` loop resumes
it until it finishes, then destroys it, which frees the frame. That also
happens when the loop is left with `break`.

**Globals** with constant values (literals, strings, struct literals of
constants, array initializers) are stored directly in the binary. Others
start zeroed and are set by a function LLVM runs before `main`, in source
order.

**Other rules:** integer arithmetic wraps; float-to-integer `as` casts
saturate, as in Rust; the extra arguments of variadic C functions like
`printf` get C's promotions (`f32` to `double`, narrow integers and `bool` to
`int`, arrays to pointers).

**Symbol names** follow an uncompressed subset of the Itanium C++ ABI, so
`c++filt`, gdb and perf demangle them. A function's name includes its module
(the file name), any enclosing functions, and its parameter types:
`_ZN9iterators9bytes_getERN9iterators5BytesEm` is
`iterators::bytes_get(iterators::Bytes&, unsigned long)`. Same-named
functions in sibling blocks get a `$2` segment (`f::$2::g()`). Everything
except `main` and `pub` items has internal linkage, so nothing else can clash
with C libraries at link time.

**Known limitations:**

- A generator that isn't looped over to the end leaks its frame. This
  happens when it's never looped over at all, or when a `return` leaves the
  loop over it.
- There are no bounds checks. Shifting by the type's width or more, and
  dividing by zero, are undefined.
- Layout assumes x86-64. Structs and arrays are passed by value between
  minlang functions, which isn't the C ABI.
- No debug info.

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
               imports.rs (copying in what other files export),
               builtins.rs (built-in types and modules), errors.rs
  codegen/     Code generation: ir.rs (backend-neutral IR), lower/ (lowering:
               expressions, control flow, switch patterns, generators),
               text.rs (.ll backend), inkwell.rs (feature-gated stub),
               mangle.rs (symbol names), layout.rs (sizes and alignment),
               driver.rs (runs clang)
  lsp/         Language server: mod.rs (protocol), analysis.rs (diagnostics,
               semantic tokens, hover, definitions, completion)
  modules.rs   Finds, parses and checks the files a program imports
  diagnostic.rs  Renders errors with their source line
  error.rs     Shared GenericError<K> positioned error type
  lib.rs       Library root
  main.rs      The `minlang` command
editors/       Editor support: tree-sitter-minlang/ (grammar and queries),
               zed/, vscode/, neovim/, helix/
examples/      Sample .pk source files, with expected output in .out files
tests/         Integration tests (tokenizer.rs, parser.rs, checker.rs,
               modules.rs, codegen.rs, cli.rs, lsp.rs, editors.rs) and end-to-end
               programs/ with expected output
```

## Roadmap

- [x] Tokenizer with structured, position-aware errors
- [x] Parser / AST with error recovery
- [x] Type checking
- [x] Code generation (LLVM IR)
- [x] Language server and editor support
- [x] Modules: importing other files, separate compilation

## License

No license has been chosen yet.
