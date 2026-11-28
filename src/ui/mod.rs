//! NEXA UI 1.0 support (NEXA UI Language Specification 1.0).
//!
//! The syntax, semantic analysis and native code generation for UI programs
//! live in `syntax`, `semantics` and `codegen`. This module owns the native
//! UI runtime that generated programs link against (`runtime.c` in this
//! directory) and the small helpers the compiler driver needs to decide
//! whether a source file is a UI program.

use crate::syntax::ast::{Program, Stmt};

/// True when the program is a UI application (`app("Name") { ... }`).
pub fn is_ui_program(program: &Program) -> bool {
    program
        .statements
        .iter()
        .any(|stmt| matches!(stmt, Stmt::UiApp { .. }))
}

/// The embedded C source of the native UI runtime, linked into every UI
/// binary by the compiler driver.
pub const RUNTIME_C: &str = include_str!("runtime.c");
