# NEXA

NEXA is a small, statically checked language that compiles directly to native
x86-64 Linux executables. The compiler is written in Rust and emits GNU as
Intel-syntax assembly; there is no interpreter or bytecode VM in the execution
path.

## Requirements

- Rust 1.85 or newer (the dependency graph uses edition 2024).
- A C toolchain (`cc`, assembler, linker) and libc/libm development files.
- Linux x86-64 for generated binaries.

## Build the compiler

```sh
cargo build
cargo test --all-targets
```

## CLI

```sh
nexa new my_app
cd my_app
nexa check
nexa build
nexa build --release
nexa run
nexa test
nexa add helper --path ../helper
nexa remove helper
nexa update
nexa clean
nexa --version
```

You can also compile and run a single file:

```sh
nexa run example.nexa
nexa example.nexa
```

Project metadata lives in `nexa.toml`; resolved package state lives in
`nexa.lock`. Build output is written under the configured `build` directory.

## Implemented language surface

The native backend supports the existing NEXA grammar:

- `let`, `var`, and `const` bindings.
- Assignment to mutable variables.
- Integer, floating-point, string, and boolean literals.
- Arithmetic: `+`, `-`, `*`, `/`, `%`.
- Comparisons: `==`, `!=`, `<`, `<=`, `>`, `>=`.
- Boolean logic: `and`, `or`, `not` (the lexer also accepts `&&`, `||`, `!`).
- String concatenation with `+` and content-based string equality.
- `if` / `else` / `else if`.
- `while` loops.
- `repeat <integer-expression> { ... }` loops.
- Top-level `action name(params) { ... }` declarations and calls.
- Built-in `print(value)` for integers, floats, strings, and booleans.
- Line comments with `//`.
- Newline-separated statements; semicolons are forbidden.

Example:

```nexa
let name = "NEXA"
var score = 100

action greet(target) {
    print("Hello " + target)
}

if score >= 100 {
    greet(name)
} else {
    print("Keep going")
}

repeat 3 {
    print(score)
    score = score - 1
}
```

## Type checking and actions

Semantic analysis enforces lexical scoping, immutable/mutable binding rules,
matching operand types, boolean conditions, integer repeat counts, action arity,
and action argument types. Actions are void. Because the current grammar has no
parameter type annotations or return statements, action parameter types are
inferred monomorphically from their uses and call sites; conflicting uses are
rejected. Unconstrained parameters default to `String`.

## Native runtime behavior

Generated programs use the System V AMD64 ABI. The backend maintains 16-byte
stack alignment, supports integer and floating-point register/stack arguments,
and emits runtime helpers for:

- String concatenation and equality.
- Checked integer division/modulo (divide-by-zero and signed overflow report a
  runtime error and exit with status 1).
- Floating-point remainder via `fmod`.
- Printing integers, floats, strings, and booleans.

String concatenation allocates native heap memory; allocations are reclaimed by
the operating system when the process exits.

## Project system notes

`nexa.toml` and `nexa.lock` are preserved. Local path dependencies are resolved
transitively, cycle-checked, version-checked when a version is supplied, and
verified against `nexa.lock` during builds. Remote registry dependencies are not
available in this 1.0 implementation and produce an explicit error. The current
grammar does not define an `import`/module syntax, so dependency source files are
not automatically merged into the entry program.

## Repository layout

- `src/syntax`: lexer, tokens, AST, parser.
- `src/semantics`: scope/type checking and typed analysis output.
- `src/codegen`: x86-64 native assembly backend and runtime helpers.
- `src/project`: manifest, lockfile, dependency resolution, builder, templates.
- `src/cli`: project command handlers.
- `tests`: syntax, semantics, native codegen, manifest, and project tests.
