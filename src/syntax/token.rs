use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Ident(String),

    Let,
    Var,
    Const,

    Action,
    If,
    Else,
    While,
    Repeat,

    // NEXA UI 1.0 (see NEXA UI Language Specification 1.0, section 7).
    App,
    State,

    And,
    Or,
    Not,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    EqEqual,
    NotEqual,
    Less,
    LessEq,
    Greater,
    GreaterEq,

    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semicolon,

    Newline,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}
