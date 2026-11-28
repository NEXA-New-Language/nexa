#![allow(clippy::result_large_err)]

pub mod cli;
pub mod codegen;
pub mod diagnostics;
pub mod ir;
pub mod project;
pub mod semantics;
pub mod span;
pub mod syntax;
pub mod types;
pub mod ui;
pub mod vm;

use codegen::X86_64CodeGen;
use diagnostics::Diagnostic;
use semantics::SemanticAnalyzer;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use syntax::ast::Program;
use syntax::lexer::Lexer;
use syntax::parser::Parser;

pub fn parse_source(file_name: &str, source: &str) -> Result<Program, Diagnostic> {
    let mut lexer = Lexer::new(file_name, source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(file_name, tokens);
    parser.parse_program()
}

pub fn analyze_source(file_name: &str, source: &str) -> Result<Program, Diagnostic> {
    let program = parse_source(file_name, source)?;
    let mut analyzer = SemanticAnalyzer::new(file_name);
    analyzer.analyze(&program)?;
    Ok(program)
}

/// True when the source is a NEXA UI 1.0 application (`app("Name") { ... }`).
/// Parse failures return false; the real parse reports the error later.
pub fn source_has_ui_app(source: &str) -> bool {
    parse_source("<ui-check>", source)
        .map(|program| ui::is_ui_program(&program))
        .unwrap_or(false)
}

pub fn compile_to_assembly(file_name: &str, source: &str) -> Result<String, Diagnostic> {
    let program = parse_source(file_name, source)?;
    let mut analyzer = SemanticAnalyzer::new(file_name);
    let analyzed = analyzer.analyze_program(&program)?;
    let codegen = X86_64CodeGen::new();
    Ok(codegen.generate_analyzed(&analyzed))
}

pub fn compile_to_binary(
    file_name: &str,
    source: &str,
    output_path: &str,
) -> Result<(), Diagnostic> {
    let ui = source_has_ui_app(source);
    let asm_code = compile_to_assembly(file_name, source)?;
    if let Some(parent) = std::path::Path::new(output_path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                Diagnostic::error(
                    "IO Error",
                    file_name,
                    crate::span::Span::new(0, 0, 1, 1),
                    format!("Cannot create output directory: {}", e),
                    None,
                )
            })?;
        }
    }
    let temp_asm = format!("{}.{}.s", output_path, std::process::id());

    fs::write(&temp_asm, asm_code).map_err(|e| {
        Diagnostic::error(
            "IO Error",
            file_name,
            crate::span::Span::new(0, 0, 1, 1),
            format!("Cannot write assembly: {}", e),
            None,
        )
    })?;

    let result = link_native_binary(Path::new(&temp_asm), Path::new(output_path), ui, &[]);
    let _ = fs::remove_file(&temp_asm);

    result.map_err(|msg| {
        Diagnostic::error(
            "Linker Error",
            file_name,
            crate::span::Span::new(0, 0, 1, 1),
            msg,
            None,
        )
    })
}

/// Link generated assembly into a native binary with `cc`.
///
/// For UI programs the native UI runtime (`src/ui/runtime.c`) is compiled
/// and linked as well. It first tries to link the windowed GTK 3 backend;
/// if GTK cannot be linked on this machine it falls back to the headless
/// backend (`-DNEXA_UI_NO_GTK`), so UI programs always build.
pub fn link_native_binary(
    asm_path: &Path,
    output_path: &Path,
    ui: bool,
    extra_cc_args: &[&str],
) -> Result<(), String> {
    let runtime_path: Option<PathBuf> = if ui {
        let path = PathBuf::from(format!("{}.nexa-ui-runtime.c", output_path.display()));
        fs::write(&path, ui::RUNTIME_C)
            .map_err(|e| format!("Cannot write UI runtime source: {}", e))?;
        Some(path)
    } else {
        None
    };

    let result = link_attempt(asm_path, runtime_path.as_deref(), output_path, extra_cc_args, true);

    if let Some(path) = &runtime_path {
        let _ = fs::remove_file(path);
    }
    result
}

fn link_attempt(
    asm_path: &Path,
    runtime_path: Option<&Path>,
    output_path: &Path,
    extra_cc_args: &[&str],
    with_gtk: bool,
) -> Result<(), String> {
    let mut args: Vec<String> = vec![
        // Position-independent executable with full RELRO and an
        // immediate binding of lazy symbols (hardening; see README).
        "-pie".to_string(),
        "-fPIE".to_string(),
        "-Wl,-z,relro,-z,now,-z,noexecstack".to_string(),
    ];
    args.extend(extra_cc_args.iter().map(|a| a.to_string()));
    if let Some(runtime) = runtime_path {
        if !with_gtk {
            args.push("-DNEXA_UI_NO_GTK".to_string());
        }
        args.push(runtime.to_string_lossy().into_owned());
    }
    args.push(asm_path.to_string_lossy().into_owned());
    args.push("-o".to_string());
    args.push(output_path.to_string_lossy().into_owned());
    args.push("-lm".to_string());
    if with_gtk && runtime_path.is_some() {
        args.extend(gtk_link_args());
    }

    let output = Command::new("cc")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to invoke linker 'cc': {}", e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    // GTK is optional: a link failure mentioning GTK means this machine
    // cannot link the windowed backend, so retry headless-only.
    if with_gtk
        && runtime_path.is_some()
        && (stderr.contains("gtk")
            || stderr.contains("gdk")
            || stderr.contains("pango")
            || stderr.contains("cannot find -l"))
    {
        return link_attempt(asm_path, runtime_path, output_path, extra_cc_args, false);
    }
    Err(format!(
        "Linker failed to generate native binary.{}",
        if stderr.trim().is_empty() {
            String::new()
        } else {
            format!("\n{}", stderr.trim())
        }
    ))
}

/// Link arguments for GTK 3. The runtime library is usually present
/// without its development symlinks, so versioned `.so.0` files are passed
/// by path when the unversioned `.so` is missing.
fn gtk_link_args() -> Vec<String> {
    let libs = [
        "gtk-3",
        "gdk-3",
        "pangocairo-1.0",
        "pango-1.0",
        "atk-1.0",
        "cairo-gobject",
        "cairo",
        "gdk_pixbuf-2.0",
        "gio-2.0",
        "gobject-2.0",
        "glib-2.0",
    ];
    let dir = "/usr/lib/x86_64-linux-gnu";
    libs.iter()
        .map(|lib| {
            let so = format!("{}/lib{}.so", dir, lib);
            if Path::new(&so).exists() {
                return format!("-l{}", lib);
            }
            let so0 = format!("{}/lib{}.so.0", dir, lib);
            if Path::new(&so0).exists() {
                return so0;
            }
            format!("-l{}", lib)
        })
        .collect()
}
