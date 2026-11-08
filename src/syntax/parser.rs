use crate::diagnostics::Diagnostic;
use crate::span::Span;
use crate::syntax::ast::*;
use crate::syntax::token::{Token, TokenKind};

pub struct Parser<'a> {
    file_name: &'a str,
    tokens: Vec<Token>,
    cursor: usize,
}

impl<'a> Parser<'a> {
    pub fn new(file_name: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            file_name,
            tokens,
            cursor: 0,
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, Diagnostic> {
        let mut statements = Vec::new();
        let start_span = self.current_span();

        self.skip_newlines();

        while !self.is_at_end() {
            let stmt = self.parse_statement()?;
            statements.push(stmt);

            if self.is_at_end() {
                break;
            }

            if !self.check(&TokenKind::Newline) && !self.check(&TokenKind::RBrace) {
                let span = self.current_span();
                return Err(Diagnostic::error(
                    "Syntax Error",
                    self.file_name,
                    span,
                    "Expected newline to terminate statement.",
                    Some("Separate statements with a newline.".into()),
                ));
            }

            self.skip_newlines();
        }

        let end_span = self.previous_span();
        let full_span = start_span.to(end_span);

        Ok(Program {
            statements,
            span: full_span,
        })
    }

    fn parse_statement(&mut self) -> Result<Stmt, Diagnostic> {
        match self.peek_kind() {
            TokenKind::Let => self.parse_var_decl(DeclKind::Let),
            TokenKind::Var => self.parse_var_decl(DeclKind::Var),
            TokenKind::Const => self.parse_var_decl(DeclKind::Const),
            TokenKind::Action => self.parse_action_decl(),
            TokenKind::If => self.parse_if_stmt(),
            TokenKind::While => self.parse_while_stmt(),
            TokenKind::Repeat => self.parse_repeat_stmt(),
            TokenKind::Ident(_) if self.peek_next_kind() == Some(&TokenKind::Assign) => {
                self.parse_assign_stmt()
            }
            _ => {
                let expr = self.parse_expression(0)?;
                Ok(Stmt::Expr(expr))
            }
        }
    }

    fn parse_var_decl(&mut self, kind: DeclKind) -> Result<Stmt, Diagnostic> {
        let start_span = self.advance().span;
        let name = self.expect_identifier("variable name")?;

        self.expect(&TokenKind::Assign, "Expected '=' after variable name.")?;
        let value = self.parse_expression(0)?;
        let span = start_span.to(value.span());

        let stmt = match kind {
            DeclKind::Let => Stmt::Let { name, value, span },
            DeclKind::Var => Stmt::Var { name, value, span },
            DeclKind::Const => Stmt::Const { name, value, span },
        };
        Ok(stmt)
    }

    fn parse_assign_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let name = self.expect_identifier("assignment target")?;
        self.expect(&TokenKind::Assign, "Expected '=' in assignment.")?;
        let value = self.parse_expression(0)?;
        let span = name.span.to(value.span());

        Ok(Stmt::Assign {
            target: name,
            value,
            span,
        })
    }

    fn parse_action_decl(&mut self) -> Result<Stmt, Diagnostic> {
        let start_span = self.advance().span;
        let name = self.expect_identifier("function name")?;

        self.expect(&TokenKind::LParen, "Expected '(' after function name.")?;
        let mut params = Vec::new();

        if !self.check(&TokenKind::RParen) {
            loop {
                let param = self.expect_identifier("parameter name")?;
                params.push(param);

                if self.match_token(&TokenKind::Comma) {
                    continue;
                }
                break;
            }
        }

        self.expect(&TokenKind::RParen, "Expected ')' after parameter list.")?;
        let body = self.parse_block()?;
        let span = start_span.to(body.span);

        Ok(Stmt::Action {
            name,
            params,
            body,
            span,
        })
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start_span = self.advance().span;
        let condition = self.parse_expression(0)?;
        let then_branch = self.parse_block()?;

        self.skip_newlines();

        let else_branch = if self.match_token(&TokenKind::Else) {
            if self.check(&TokenKind::If) {
                let nested_if = self.parse_if_stmt()?;
                let span = nested_if.span();
                Some(Block {
                    statements: vec![nested_if],
                    span,
                })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };

        let span = match &else_branch {
            Some(eb) => start_span.to(eb.span),
            None => start_span.to(then_branch.span),
        };

        Ok(Stmt::If {
            condition,
            then_branch,
            else_branch,
            span,
        })
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start_span = self.advance().span;
        let condition = self.parse_expression(0)?;
        let body = self.parse_block()?;
        let span = start_span.to(body.span);

        Ok(Stmt::While {
            condition,
            body,
            span,
        })
    }

    fn parse_repeat_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start_span = self.advance().span;
        let count = self.parse_expression(0)?;
        let body = self.parse_block()?;
        let span = start_span.to(body.span);

        Ok(Stmt::Repeat { count, body, span })
    }

    fn parse_block(&mut self) -> Result<Block, Diagnostic> {
        self.skip_newlines();
        let lbrace = self.expect(&TokenKind::LBrace, "Expected '{' to start block.")?;
        self.skip_newlines();

        let mut statements = Vec::new();

        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let stmt = self.parse_statement()?;
            statements.push(stmt);

            if self.check(&TokenKind::RBrace) {
                break;
            }

            if !self.check(&TokenKind::Newline) {
                let span = self.current_span();
                return Err(Diagnostic::error(
                    "Syntax Error",
                    self.file_name,
                    span,
                    "Expected newline after statement in block.",
                    Some("Add a newline between statements.".into()),
                ));
            }

            self.skip_newlines();
        }

        let rbrace = self.expect(&TokenKind::RBrace, "Expected '}' to end block.")?;
        let span = lbrace.span.to(rbrace.span);

        Ok(Block { statements, span })
    }

    fn parse_expression(&mut self, min_precedence: u8) -> Result<Expr, Diagnostic> {
        let mut left = self.parse_unary()?;

        loop {
            let (op, precedence) = match self.peek_binary_op() {
                Some((op, prec)) if prec >= min_precedence => (op, prec),
                _ => break,
            };

            self.advance();
            let right = self.parse_expression(precedence + 1)?;
            let span = left.span().to(right.span());

            left = Expr::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        if self.match_token(&TokenKind::Minus) {
            let op_span = self.previous_span();
            let expr = self.parse_unary()?;
            let span = op_span.to(expr.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Negate,
                expr: Box::new(expr),
                span,
            });
        }

        if self.match_token(&TokenKind::Not) {
            let op_span = self.previous_span();
            let expr = self.parse_unary()?;
            let span = op_span.to(expr.span());
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
                span,
            });
        }

        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        let tok = self.advance();
        match tok.kind {
            TokenKind::Int(val) => Ok(Expr::Literal {
                lit: Literal::Int(val),
                span: tok.span,
            }),
            TokenKind::Float(val) => Ok(Expr::Literal {
                lit: Literal::Float(val),
                span: tok.span,
            }),
            TokenKind::Str(val) => Ok(Expr::Literal {
                lit: Literal::Str(val),
                span: tok.span,
            }),
            TokenKind::Bool(val) => Ok(Expr::Literal {
                lit: Literal::Bool(val),
                span: tok.span,
            }),
            TokenKind::Ident(name) => {
                let ident = Ident {
                    name,
                    span: tok.span,
                };

                if self.match_token(&TokenKind::LParen) {
                    let mut args = Vec::new();
                    if !self.check(&TokenKind::RParen) {
                        loop {
                            let arg = self.parse_expression(0)?;
                            args.push(arg);
                            if self.match_token(&TokenKind::Comma) {
                                continue;
                            }
                            break;
                        }
                    }
                    let rparen = self.expect(&TokenKind::RParen, "Expected ')' after call arguments.")?;
                    let span = ident.span.to(rparen.span);

                    Ok(Expr::Call {
                        callee: ident,
                        args,
                        span,
                    })
                } else {
                    Ok(Expr::Variable { name: ident })
                }
            }
            TokenKind::LParen => {
                let expr = self.parse_expression(0)?;
                self.expect(&TokenKind::RParen, "Expected ')' after grouping expression.")?;
                Ok(expr)
            }
            _ => Err(Diagnostic::error(
                "Syntax Error",
                self.file_name,
                tok.span,
                format!("Unexpected token '{:?}' in expression.", tok.kind),
                Some("Check your expression syntax.".into()),
            )),
        }
    }

    fn peek_binary_op(&self) -> Option<(BinaryOp, u8)> {
        match self.peek_kind() {
            TokenKind::Or => Some((BinaryOp::Or, 1)),
            TokenKind::And => Some((BinaryOp::And, 2)),
            TokenKind::EqEqual => Some((BinaryOp::Equal, 3)),
            TokenKind::NotEqual => Some((BinaryOp::NotEqual, 3)),
            TokenKind::Less => Some((BinaryOp::Less, 4)),
            TokenKind::LessEq => Some((BinaryOp::LessEq, 4)),
            TokenKind::Greater => Some((BinaryOp::Greater, 4)),
            TokenKind::GreaterEq => Some((BinaryOp::GreaterEq, 4)),
            TokenKind::Plus => Some((BinaryOp::Add, 5)),
            TokenKind::Minus => Some((BinaryOp::Sub, 5)),
            TokenKind::Star => Some((BinaryOp::Mul, 6)),
            TokenKind::Slash => Some((BinaryOp::Div, 6)),
            TokenKind::Percent => Some((BinaryOp::Mod, 6)),
            _ => None,
        }
    }

    fn expect_identifier(&mut self, context: &str) -> Result<Ident, Diagnostic> {
        let tok = self.advance();
        if let TokenKind::Ident(name) = tok.kind {
            Ok(Ident {
                name,
                span: tok.span,
            })
        } else {
            Err(Diagnostic::error(
                "Syntax Error",
                self.file_name,
                tok.span,
                format!("Expected {}, found '{:?}'.", context, tok.kind),
                None,
            ))
        }
    }

    fn expect(&mut self, kind: &TokenKind, msg: &str) -> Result<Token, Diagnostic> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            let span = self.current_span();
            Err(Diagnostic::error(
                "Syntax Error",
                self.file_name,
                span,
                msg,
                None,
            ))
        }
    }

    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() {
            return matches!(kind, TokenKind::Eof);
        }
        self.peek_kind() == kind
    }

    fn advance(&mut self) -> Token {
        if !self.is_at_end() {
            self.cursor += 1;
        }
        self.previous_token()
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.tokens.get(self.cursor).unwrap().kind
    }

    fn peek_next_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.cursor + 1).map(|t| &t.kind)
    }

    fn current_span(&self) -> Span {
        self.tokens.get(self.cursor).unwrap().span
    }

    fn previous_token(&self) -> Token {
        self.tokens.get(self.cursor - 1).unwrap().clone()
    }

    fn previous_span(&self) -> Span {
        self.previous_token().span
    }

    fn skip_newlines(&mut self) {
        while self.match_token(&TokenKind::Newline) {}
    }
}

enum DeclKind {
    Let,
    Var,
    Const,
}
