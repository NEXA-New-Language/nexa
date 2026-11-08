pub mod codegen;
pub mod diagnostics;
pub mod ir;
pub mod semantics;
pub mod span;
pub mod syntax;
pub mod types;
pub mod vm;

use codegen::X86_64CodeGen;
use diagnostics::Diagnostic;
use semantics::SemanticAnalyzer;
use syntax::ast::Program;
use syntax::lexer::Lexer;
use syntax::parser::Parser;
use std::fs;
use std::process::Command;

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

/// NEXAコードから x86-64 アセンブリを生成
pub fn compile_to_assembly(file_name: &str, source: &str) -> Result<String, Diagnostic> {
    let program = analyze_source(file_name, source)?;
    let codegen = X86_64CodeGen::new();
    Ok(codegen.generate(&program))
}

/// NEXAコードを直接ネイティブELF実行可能ファイルにコンパイル
pub fn compile_to_binary(file_name: &str, source: &str, output_path: &str) -> Result<(), Diagnostic> {
    let asm_code = compile_to_assembly(file_name, source)?;
    let temp_asm = format!("{}.s", output_path);

    fs::write(&temp_asm, asm_code).map_err(|e| {
        Diagnostic::error("IO Error", file_name, crate::span::Span::new(0, 0, 1, 1), format!("Cannot write assembly: {}", e), None)
    })?;

    // システムリンカ (cc) でネイティブバイナリへコンパイル
    let status = Command::new("cc")
        .args(["-no-pie", &temp_asm, "-o", output_path])
        .status()
        .map_err(|e| {
            Diagnostic::error("Linker Error", file_name, crate::span::Span::new(0, 0, 1, 1), format!("Failed to invoke linker 'cc': {}", e), None)
        })?;

    let _ = fs::remove_file(&temp_asm); // 一時アセンブリを削除

    if !status.success() {
        return Err(Diagnostic::error(
            "Linker Error",
            file_name,
            crate::span::Span::new(0, 0, 1, 1),
            "Linker failed to generate native binary.",
            None,
        ));
    }

    Ok(())
}
