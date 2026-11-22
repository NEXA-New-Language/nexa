#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, col: usize) -> Self {
        Self {
            start,
            end,
            line,
            col,
        }
    }

    pub fn to(self, other: Span) -> Self {
        let first = if other.start < self.start {
            other
        } else {
            self
        };
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: first.line,
            col: first.col,
        }
    }
}
