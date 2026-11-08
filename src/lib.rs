pub mod diagnostics;
pub mod ir;
pub mod semantics;
pub mod span;
pub mod syntax;
pub mod types;
pub mod vm;

use diagnostics::Diagnostic;
use syntax::ast::Program;
use syntax::lexer::Lexer;
use syntax::parser::Parser;

pub fn parse_source(file_name: &str, source: &str) -> Result<Program, Diagnostic> {
    let mut lexer = Lexer::new(file_name, source);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(file_name, tokens);
    parser.parse_program()
}
