use crate::diagnostics::Diagnostic;
use crate::syntax::ast::Program;

pub struct SemanticAnalyzer {
    file_name: String,
}

impl SemanticAnalyzer {
    pub fn new(file_name: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
        }
    }

    pub fn analyze(&mut self, _program: &Program) -> Result<(), Diagnostic> {
        Ok(())
    }
}
