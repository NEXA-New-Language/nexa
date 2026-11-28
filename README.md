# NEXA

NEXA is a small, statically checked language that compiles directly to native
x86-64 Linux executables. The compiler is written in Rust and emits GNU as
Intel-syntax assembly; there is no interpreter or bytecode VM in the execution
path.

## Requirements

- Rust 1.85 or newer (the dependency graph uses edition 2024).
- A C toolchain (`cc`, assembler, linker) and libc/libm development files.
- Linux x86-64 for generated binaries.

## Installation

**Prebuilt binary (recommended, Linux x86-64):**

```sh
./install.sh
```

Downloads the latest release from
[GitHub Releases](https://github.com/NEXA-New-Language/nexa/releases),
verifies its SHA-256 checksum, and installs `nexa` into `~/.cargo/bin`.
If no prebuilt binary is available yet, it builds from source
automatically. `./install.sh --from-source` always builds from source.

**From source:**

```sh
cargo install --git https://github.com/NEXA-New-Language/nexa --locked nexa
```

Requires cargo and `cc`. Verify with `nexa --version`. The release
profile (`opt-level = 3`, LTO, stripped) keeps the installed compiler
around 1.2 MB.

## Releasing a new version

Push a version tag and the release workflow builds and publishes the
prebuilt binary:

```sh
git tag v1.0.0
git push origin v1.0.0
```

`.github/workflows/release.yml` builds the release compiler, runs the
full test suite, and uploads `nexa-<version>-x86_64-linux.tar.gz` (plus
its `.sha256`) to the GitHub Release for that tag.

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
nexa lint
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
- Built-in `assert(condition)`; a failed assertion prints
  `assertion failed` to stderr and exits with status 1. The argument
  must be a `Bool`, and the name `assert` is reserved.
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

## NEXA UI 1.0

A UI program is a whole program: `app("Name") { ... }` is the only
top-level item. It declares `state` values and exactly one `Window`
(spec: *NEXA UI Language Specification 1.0*, section 7). State is mutable,
its type is inferred from the initializer (Int, Float, String or Bool), and
the UI re-evaluates whenever state changes. Components are `Window`,
`Column`, `Row`, `Container`, `Text`, `Button`, `TextField`, `Checkbox`,
`Spacer`, `Divider` and `Image`; common properties are `width`, `height`,
`padding`, `margin`, `spacing`, `background` (a `"#RRGGBB"` string),
`align` and `visible`, plus `title` and `size(w, h)` on `Window` only.
Events are `on_click { ... }` on `Button` and `on_change(value) { ... }`
on `TextField` (String) and `Checkbox` (Bool); handler bodies are normal
NEXA statements, and `TextField`/`Checkbox` must be bound to a `state`
variable of the matching type.

```nexa
app("Counter") {
    state count = 0

    Window {
        title("Counter")
        size(480, 320)

        Column {
            spacing(12)
            padding(16)

            Text("Hello, NEXA!")
            Text(count)

            Button("Increment") {
                on_click {
                    count = count + 1
                }
            }

            Button("Reset") {
                on_click {
                    count = 0
                }
            }
        }
    }
}
```

UI programs compile to native binaries through the same pipeline (see
`examples/ui_counter.nexa` and `examples/ui_form.nexa`). The backend is a
native UI runtime (`src/ui/runtime.c`), not HTML/CSS/JS and not a browser:

- **Windowed backend (GTK 3):** when a display is available, the binary
  opens a real native window and updates its widgets in place after every
  event. GTK is linked through hand-written declarations; if GTK cannot be
  linked on the build machine, the program still builds and runs headless.
- **Headless backend:** otherwise, and with `NEXA_UI_HEADLESS=1`, the
  binary prints a snapshot of the UI tree to stdout and reads events from
  stdin: `click <button label>`, `type <field key> <value>`,
  `toggle <checkbox key>`, `help`, `quit` (EOF also exits). Each event
  re-renders, so state changes are visible in the next snapshot. This is
  the backend the tests use.

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

The backend also applies a small set of safe optimizations at code
generation time:

- Constant folding of pure literal expressions (arithmetic, comparisons,
  boolean logic, string concatenation). Division or modulo by zero is never
  folded, so it still traps at runtime as specified.
- In-place updates for integer self-assignments (`i = i + 1` becomes a
  single `add` on the variable's slot).
- Comparison peepholes: a variable compared against a constant becomes a
  single memory-vs-immediate `cmp`, and `if`/`while` conditions jump
  directly off the flags instead of materializing a 0/1 first.
- Constant `if` conditions keep only the taken branch; `while false`
  emits nothing.

On the 10,000,000-iteration summation loop used for benchmarking, these
bring the release binary from about 20 ms to about 12 ms, on par with
`g++ -O2` for the same loop.

## Security

Generated native binaries are hardened by default:

- **Checked integer arithmetic.** Signed `+`, `-`, `*`, unary `-`,
  and in-place `x = x ± e` trap at runtime on overflow instead of
  silently wrapping: the program prints `integer overflow` to stderr
  and exits with status 1 (same mechanism as the pre-existing
  divide-by-zero and division-overflow checks). The constant folder
  uses checked arithmetic too, so an overflowing constant expression
  is never silently folded into a wrong value.
- **Position-independent executables.** Binaries link as PIE with
  full RELRO (`-z relro -z now`), a non-executable stack, and the UI
  runtime compiled with `-fPIE`. The benchmark loop is unaffected
  (still ~12 ms for 10M iterations, tied with `g++ -O2`).
- **Bounded string work.** Concatenation allocates exactly
  `len(a) + len(b) + 1` with an overflow check on the size, and the
  headless UI runtime reads stdin through a bounded buffer.

`tests/security_test.rs` covers the overflow traps and verifies the
binary is a PIE (`DYN`) with a `GNU_RELRO` segment.

## Lint

`nexa lint` reports advisory warnings without failing the build:

- unused variables (`let` / `var` / `const`), action parameters, and
  UI `state` that is never read (an assignment is a write, not a read);
- unused actions that are never called;
- shadowing: a declaration that hides a variable from an outer scope.

Warnings render with the file, line, column, and source line, like
errors, but are headed `Warning:` and never stop compilation.
`tests/lint_test.rs` covers the rules, including a clean run over the
UI counter example.

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
- `src/ui`: NEXA UI 1.0 support and the native UI runtime (`runtime.c`).
- `src/project`: manifest, lockfile, dependency resolution, builder, templates.
- `src/cli`: project command handlers.
- `examples`: UI examples (`ui_counter.nexa`, `ui_form.nexa`).
- `tests`: syntax, semantics, native codegen, manifest, project, and UI tests.
