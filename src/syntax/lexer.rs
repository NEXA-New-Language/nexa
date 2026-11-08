use crate::diagnostics::Diagnostic;
use crate::span::Span;
use crate::syntax::token::{Token, TokenKind};

pub struct Lexer<'a> {
    file_name: &'a str,
    source: &'a str,
    chars: Vec<(usize, char)>,
    cursor: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(file_name: &'a str, source: &'a str) -> Self {
        Self {
            file_name,
            source,
            chars: source.char_indices().collect(),
            cursor: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, Diagnostic> {
        let mut tokens: Vec<Token> = Vec::new();

        while let Some(ch) = self.peek_char() {
            let start_pos = self.current_byte_pos();
            let start_line = self.line;
            let start_col = self.col;

            match ch {
                ' ' | '\t' | '\r' => {
                    self.advance_char();
                }
                '\n' => {
                    self.advance_char();
                    let span = Span::new(start_pos, self.current_byte_pos(), start_line, start_col);
                    if let Some(last) = tokens.last() {
                        if last.kind != TokenKind::Newline {
                            tokens.push(Token::new(TokenKind::Newline, span));
                        }
                    }
                }
                '/' if self.peek_next_char() == Some('/') => {
                    while let Some(c) = self.peek_char() {
                        if c == '\n' {
                            break;
                        }
                        self.advance_char();
                    }
                }
                ';' => {
                    self.advance_char();
                    let span = Span::new(start_pos, self.current_byte_pos(), start_line, start_col);
                    return Err(Diagnostic::error(
                        "Forbidden syntax",
                        self.file_name,
                        span,
                        "Semicolons are forbidden in NEXA.",
                        Some("Remove ';' and use newlines to separate statements.".into()),
                    ));
                }
                '(' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::LParen, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                ')' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::RParen, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '{' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::LBrace, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '}' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::RBrace, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                ',' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::Comma, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '+' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::Plus, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '-' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::Minus, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '*' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::Star, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '/' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::Slash, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '%' => {
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::Percent, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '=' => {
                    self.advance_char();
                    if self.peek_char() == Some('=') {
                        self.advance_char();
                        tokens.push(Token::new(TokenKind::EqEqual, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    } else {
                        tokens.push(Token::new(TokenKind::Assign, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    }
                }
                '!' => {
                    self.advance_char();
                    if self.peek_char() == Some('=') {
                        self.advance_char();
                        tokens.push(Token::new(TokenKind::NotEqual, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    } else {
                        tokens.push(Token::new(TokenKind::Not, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    }
                }
                '<' => {
                    self.advance_char();
                    if self.peek_char() == Some('=') {
                        self.advance_char();
                        tokens.push(Token::new(TokenKind::LessEq, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    } else {
                        tokens.push(Token::new(TokenKind::Less, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    }
                }
                '>' => {
                    self.advance_char();
                    if self.peek_char() == Some('=') {
                        self.advance_char();
                        tokens.push(Token::new(TokenKind::GreaterEq, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    } else {
                        tokens.push(Token::new(TokenKind::Greater, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                    }
                }
                '&' if self.peek_next_char() == Some('&') => {
                    self.advance_char();
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::And, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '|' if self.peek_next_char() == Some('|') => {
                    self.advance_char();
                    self.advance_char();
                    tokens.push(Token::new(TokenKind::Or, Span::new(start_pos, self.current_byte_pos(), start_line, start_col)));
                }
                '"' => {
                    let tok = self.read_string(start_pos, start_line, start_col)?;
                    tokens.push(tok);
                }
                '0'..='9' => {
                    let tok = self.read_number(start_pos, start_line, start_col)?;
                    tokens.push(tok);
                }
                'a'..='z' | 'A'..='Z' | '_' => {
                    let tok = self.read_identifier_or_keyword(start_pos, start_line, start_col);
                    tokens.push(tok);
                }
                unexpected => {
                    self.advance_char();
                    let span = Span::new(start_pos, self.current_byte_pos(), start_line, start_col);
                    return Err(Diagnostic::error(
                        "Lexical error",
                        self.file_name,
                        span,
                        format!("Unexpected character '{}'.", unexpected),
                        None,
                    ));
                }
            }
        }

        let eof_span = Span::new(self.source.len(), self.source.len(), self.line, self.col);
        tokens.push(Token::new(TokenKind::Eof, eof_span));
        Ok(tokens)
    }

    fn read_string(&mut self, start_pos: usize, start_line: usize, start_col: usize) -> Result<Token, Diagnostic> {
        self.advance_char();
        let mut text = String::new();

        while let Some(ch) = self.peek_char() {
            match ch {
                '"' => {
                    self.advance_char();
                    let span = Span::new(start_pos, self.current_byte_pos(), start_line, start_col);
                    return Ok(Token::new(TokenKind::Str(text), span));
                }
                '\\' => {
                    self.advance_char();
                    match self.peek_char() {
                        Some('n') => { self.advance_char(); text.push('\n'); }
                        Some('t') => { self.advance_char(); text.push('\t'); }
                        Some('r') => { self.advance_char(); text.push('\r'); }
                        Some('\\') => { self.advance_char(); text.push('\\'); }
                        Some('"') => { self.advance_char(); text.push('"'); }
                        Some(other) => {
                            let span = Span::new(self.current_byte_pos(), self.current_byte_pos() + 1, self.line, self.col);
                            return Err(Diagnostic::error(
                                "Invalid escape sequence",
                                self.file_name,
                                span,
                                format!("Unknown escape sequence '\\{}'.", other),
                                Some("Valid escapes are: \\n, \\t, \\r, \\\\, \\\"".into()),
                            ));
                        }
                        None => break,
                    }
                }
                '\n' => {
                    let span = Span::new(start_pos, self.current_byte_pos(), start_line, start_col);
                    return Err(Diagnostic::error(
                        "Unterminated string literal",
                        self.file_name,
                        span,
                        "String literal cannot span across multiple lines.",
                        Some("Close the string with '\"' before the newline.".into()),
                    ));
                }
                other => {
                    self.advance_char();
                    text.push(other);
                }
            }
        }

        let span = Span::new(start_pos, self.current_byte_pos(), start_line, start_col);
        Err(Diagnostic::error(
            "Unterminated string literal",
            self.file_name,
            span,
            "Expected closing quote '\"' before end of file.",
            None,
        ))
    }

    fn read_number(&mut self, start_pos: usize, start_line: usize, start_col: usize) -> Result<Token, Diagnostic> {
        let mut is_float = false;

        while let Some(ch) = self.peek_char() {
            if ch.is_ascii_digit() {
                self.advance_char();
            } else if ch == '.' && !is_float && self.peek_next_char().map_or(false, |c| c.is_ascii_digit()) {
                is_float = true;
                self.advance_char();
            } else {
                break;
            }
        }

        let end_pos = self.current_byte_pos();
        let raw = &self.source[start_pos..end_pos];
        let span = Span::new(start_pos, end_pos, start_line, start_col);

        if is_float {
            match raw.parse::<f64>() {
                Ok(val) => Ok(Token::new(TokenKind::Float(val), span)),
                Err(_) => Err(Diagnostic::error(
                    "Invalid float literal",
                    self.file_name,
                    span,
                    format!("Cannot parse '{}' as 64-bit float.", raw),
                    None,
                )),
            }
        } else {
            match raw.parse::<i64>() {
                Ok(val) => Ok(Token::new(TokenKind::Int(val), span)),
                Err(_) => Err(Diagnostic::error(
                    "Invalid integer literal",
                    self.file_name,
                    span,
                    format!("Cannot parse '{}' as 64-bit signed integer.", raw),
                    None,
                )),
            }
        }
    }

    fn read_identifier_or_keyword(&mut self, start_pos: usize, start_line: usize, start_col: usize) -> Token {
        while let Some(ch) = self.peek_char() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance_char();
            } else {
                break;
            }
        }

        let end_pos = self.current_byte_pos();
        let word = &self.source[start_pos..end_pos];
        let span = Span::new(start_pos, end_pos, start_line, start_col);

        let kind = match word {
            "let" => TokenKind::Let,
            "var" => TokenKind::Var,
            "const" => TokenKind::Const,
            "action" => TokenKind::Action,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "repeat" => TokenKind::Repeat,
            "and" => TokenKind::And,
            "or" => TokenKind::Or,
            "not" => TokenKind::Not,
            "true" => TokenKind::Bool(true),
            "false" => TokenKind::Bool(false),
            _ => TokenKind::Ident(word.to_string()),
        };

        Token::new(kind, span)
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.cursor).map(|&(_, c)| c)
    }

    fn peek_next_char(&self) -> Option<char> {
        self.chars.get(self.cursor + 1).map(|&(_, c)| c)
    }

    fn advance_char(&mut self) -> Option<char> {
        if let Some(&(_, c)) = self.chars.get(self.cursor) {
            self.cursor += 1;
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            Some(c)
        } else {
            None
        }
    }

    fn current_byte_pos(&self) -> usize {
        self.chars
            .get(self.cursor)
            .map(|&(idx, _)| idx)
            .unwrap_or(self.source.len())
    }
}
