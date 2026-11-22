use crate::span::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: String,
    pub file_name: String,
    pub span: Span,
    pub message: String,
    pub hint: Option<String>,
}

impl Diagnostic {
    pub fn error(
        kind: impl Into<String>,
        file_name: impl Into<String>,
        span: Span,
        message: impl Into<String>,
        hint: Option<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            file_name: file_name.into(),
            span,
            message: message.into(),
            hint,
        }
    }

    pub fn render(&self, source: &str) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let line_idx = self.span.line.saturating_sub(1);
        let source_line = lines.get(line_idx).copied().unwrap_or("");

        let indent = " ".repeat(self.span.col.saturating_sub(1));
        let line_start: usize = lines.iter().take(line_idx).map(|l| l.len() + 1).sum();
        let start_in_line = self.span.start.saturating_sub(line_start);
        let end_in_line = self.span.end.saturating_sub(line_start);
        let width = if start_in_line <= source_line.len()
            && end_in_line <= source_line.len()
            && end_in_line > start_in_line
        {
            source_line[start_in_line..end_in_line]
                .chars()
                .count()
                .max(1)
        } else {
            self.span.end.saturating_sub(self.span.start).max(1)
        };
        let carets = "^".repeat(width);

        let mut out = String::new();
        out.push_str(&format!("Error: {}\n\n", self.kind));
        out.push_str(&format!(
            "{}:{}:{}\n\n",
            self.file_name, self.span.line, self.span.col
        ));
        out.push_str(&format!("{}\n", source_line));
        out.push_str(&format!("{}{}\n\n", indent, carets));
        out.push_str(&format!("{}\n", self.message));

        if let Some(ref hint) = self.hint {
            out.push_str(&format!("{}\n", hint));
        }

        out
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Error: {} ({}:{}:{}) - {}",
            self.kind, self.file_name, self.span.line, self.span.col, self.message
        )
    }
}

impl std::error::Error for Diagnostic {}
