
use crate::diagnostics::Diagnostic;
use crate::span::Span;
use crate::syntax::ast::*;
use crate::types::Type;
use std::collections::HashMap;

#[derive(Clone)]
struct Variable {
    ty: Type,
    mutable: bool,
}

#[derive(Clone)]
struct ActionSignature {
    params: Vec<Type>,
}

pub struct SemanticAnalyzer {
    file_name: String,
    scopes: Vec<HashMap<String, Variable>>,
    actions: HashMap<String, ActionSignature>,
}

impl SemanticAnalyzer {
    pub fn new(file_name: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            scopes: vec![HashMap::new()],
            actions: HashMap::new(),
        }
    }

    pub fn analyze(&mut self, program: &Program) -> Result<(), Diagnostic> {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
        self.actions.clear();

        // 関数を先に登録し、前方参照を可能にする。
        for stmt in &program.statements {
            if let Stmt::Action { name, params, .. } = stmt {
                if name.name == "print" {
                    return Err(self.error(
                        name.span,
                        "The name 'print' is reserved.",
                        Some("Choose another action name.".into()),
                    ));
                }

                if self.actions.contains_key(&name.name) {
                    return Err(self.error(
                        name.span,
                        format!("Action '{}' is already declared.", name.name),
                        None,
                    ));
                }

                let mut names = Vec::new();
                for param in params {
                    if names.contains(&param.name) {
                        return Err(self.error(
                            param.span,
                            format!("Duplicate parameter '{}'.", param.name),
                            None,
                        ));
                    }
                    names.push(param.name.clone());
                }

                // 現在のネイティブコード生成ではactionの引数は文字列として扱う。
                self.actions.insert(
                    name.name.clone(),
                    ActionSignature {
                        params: vec![Type::String; params.len()],
                    },
                );
            }
        }

        for stmt in &program.statements {
            self.analyze_stmt(stmt)?;
        }

        Ok(())
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) -> Result<(), Diagnostic> {
        match stmt {
            Stmt::Let { name, value, .. } => {
                let ty = self.analyze_expr(value)?;
                self.declare(name, ty, false)?;
            }

            Stmt::Var { name, value, .. } => {
                let ty = self.analyze_expr(value)?;
                self.declare(name, ty, true)?;
            }

            Stmt::Const { name, value, .. } => {
                let ty = self.analyze_expr(value)?;
                self.declare(name, ty, false)?;
            }

            Stmt::Assign { target, value, .. } => {
                let value_type = self.analyze_expr(value)?;

                let variable = self.lookup_variable(&target.name)
                    .ok_or_else(|| self.error(
                        target.span,
                        format!("Undefined variable '{}'.", target.name),
                        Some("Declare the variable before assigning to it.".into()),
                    ))?;

                if !variable.mutable {
                    return Err(self.error(
                        target.span,
                        format!("Cannot assign to immutable variable '{}'.", target.name),
                        Some("Use 'var' when the variable needs reassignment.".into()),
                    ));
                }

                if variable.ty != value_type {
                    return Err(self.type_error(
                        target.span,
                        &variable.ty,
                        &value_type,
                    ));
                }
            }

            Stmt::Action { name, params, body, .. } => {
                // actionはトップレベルでのみ宣言可能。
                if self.scopes.len() != 1 {
                    return Err(self.error(
                        name.span,
                        "Nested action declarations are not supported.",
                        None,
                    ));
                }

                self.push_scope();

                for param in params {
                    self.declare(param, Type::String, true)?;
                }

                for statement in &body.statements {
                    self.analyze_stmt(statement)?;
                }

                self.pop_scope();
            }

            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let condition_type = self.analyze_expr(condition)?;
                self.require_type(&condition_type, &Type::Bool, condition.span())?;

                self.analyze_block(then_branch)?;

                if let Some(block) = else_branch {
                    self.analyze_block(block)?;
                }
            }

            Stmt::While { condition, body, .. } => {
                let condition_type = self.analyze_expr(condition)?;
                self.require_type(&condition_type, &Type::Bool, condition.span())?;
                self.analyze_block(body)?;
            }

            Stmt::Repeat { count, body, .. } => {
                let count_type = self.analyze_expr(count)?;
                self.require_type(&count_type, &Type::Int, count.span())?;
                self.analyze_block(body)?;
            }

            Stmt::Expr(expr) => {
                self.analyze_expr(expr)?;
            }
        }

        Ok(())
    }

    fn analyze_block(&mut self, block: &Block) -> Result<(), Diagnostic> {
        self.push_scope();

        for stmt in &block.statements {
            self.analyze_stmt(stmt)?;
        }

        self.pop_scope();
        Ok(())
    }

    fn analyze_expr(&mut self, expr: &Expr) -> Result<Type, Diagnostic> {
        match expr {
            Expr::Literal { lit, .. } => {
                Ok(match lit {
                    Literal::Int(_) => Type::Int,
                    Literal::Float(_) => Type::Float,
                    Literal::Str(_) => Type::String,
                    Literal::Bool(_) => Type::Bool,
                })
            }

            Expr::Variable { name } => {
                self.lookup_variable(&name.name)
                    .map(|variable| variable.ty)
                    .ok_or_else(|| self.error(
                        name.span,
                        format!("Undefined variable '{}'.", name.name),
                        Some("Declare the variable before using it.".into()),
                    ))
            }

            Expr::Unary { op, expr, span } => {
                let operand = self.analyze_expr(expr)?;

                match op {
                    UnaryOp::Negate => {
                        if operand != Type::Int && operand != Type::Float {
                            return Err(self.error(
                                *span,
                                "Unary '-' requires a numeric value.",
                                None,
                            ));
                        }
                        Ok(operand)
                    }

                    UnaryOp::Not => {
                        self.require_type(&operand, &Type::Bool, *span)?;
                        Ok(Type::Bool)
                    }
                }
            }

            Expr::Binary { left, op, right, span } => {
                let left_type = self.analyze_expr(left)?;
                let right_type = self.analyze_expr(right)?;

                match op {
                    BinaryOp::Add => {
                        if left_type == Type::String && right_type == Type::String {
                            Ok(Type::String)
                        } else if left_type == right_type
                            && (left_type == Type::Int || left_type == Type::Float)
                        {
                            Ok(left_type)
                        } else {
                            Err(self.error(
                                *span,
                                "Addition requires two values of the same numeric type or two strings.",
                                None,
                            ))
                        }
                    }

                    BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod => {
                        if left_type == right_type
                            && (left_type == Type::Int || left_type == Type::Float)
                        {
                            Ok(left_type)
                        } else {
                            Err(self.error(
                                *span,
                                "Arithmetic operation requires matching numeric types.",
                                None,
                            ))
                        }
                    }

                    BinaryOp::Equal | BinaryOp::NotEqual => {
                        if left_type == right_type {
                            Ok(Type::Bool)
                        } else {
                            Err(self.error(
                                *span,
                                "Equality comparison requires matching types.",
                                None,
                            ))
                        }
                    }

                    BinaryOp::Less
                    | BinaryOp::LessEq
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEq => {
                        if left_type == right_type
                            && (left_type == Type::Int || left_type == Type::Float)
                        {
                            Ok(Type::Bool)
                        } else {
                            Err(self.error(
                                *span,
                                "Comparison requires matching numeric types.",
                                None,
                            ))
                        }
                    }

                    BinaryOp::And | BinaryOp::Or => {
                        self.require_type(&left_type, &Type::Bool, left.span())?;
                        self.require_type(&right_type, &Type::Bool, right.span())?;
                        Ok(Type::Bool)
                    }
                }
            }

            Expr::Call { callee, args, span } => {
                if callee.name == "print" {
                    if args.len() != 1 {
                        return Err(self.error(
                            *span,
                            "print() requires exactly one argument.",
                            Some("Example: print(\"Hello\")".into()),
                        ));
                    }

                    let argument_type = self.analyze_expr(&args[0])?;

                    if argument_type == Type::Void {
                        return Err(self.error(
                            args[0].span(),
                            "Cannot print a void value.",
                            None,
                        ));
                    }

                    return Ok(Type::Void);
                }

                let signature = self.actions.get(&callee.name)
                    .cloned()
                    .ok_or_else(|| self.error(
                        callee.span,
                        format!("Undefined action '{}'.", callee.name),
                        None,
                    ))?;

                if args.len() != signature.params.len() {
                    return Err(self.error(
                        *span,
                        format!(
                            "Action '{}' expects {} arguments, but received {}.",
                            callee.name,
                            signature.params.len(),
                            args.len()
                        ),
                        None,
                    ));
                }

                for (arg, expected) in args.iter().zip(signature.params.iter()) {
                    let actual = self.analyze_expr(arg)?;

                    if &actual != expected {
                        return Err(self.type_error(arg.span(), expected, &actual));
                    }
                }

                Ok(Type::Void)
            }
        }
    }

    fn declare(
        &mut self,
        name: &Ident,
        ty: Type,
        mutable: bool,
    ) -> Result<(), Diagnostic> {
        // 現在のコード生成との名前解決の不一致を避けるため、
        // 有効なスコープ内での変数名の再宣言を禁止する。
        if self.scopes.iter().any(|scope| scope.contains_key(&name.name)) {
            return Err(self.error(
                name.span,
                format!("Variable '{}' is already declared.", name.name),
                None,
            ));
        }

        self.scopes.last_mut().unwrap().insert(
            name.name.clone(),
            Variable { ty, mutable },
        );

        Ok(())
    }

    fn lookup_variable(&self, name: &str) -> Option<Variable> {
        self.scopes.iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn require_type(
        &self,
        actual: &Type,
        expected: &Type,
        span: Span,
    ) -> Result<(), Diagnostic> {
        if actual != expected {
            return Err(self.type_error(span, expected, actual));
        }
        Ok(())
    }

    fn type_error(
        &self,
        span: Span,
        expected: &Type,
        actual: &Type,
    ) -> Diagnostic {
        self.error(
            span,
            format!("Type mismatch: expected {:?}, found {:?}.", expected, actual),
            None,
        )
    }

    fn error(
        &self,
        span: Span,
        message: impl Into<String>,
        hint: Option<String>,
    ) -> Diagnostic {
        Diagnostic::error(
            "Semantic Error",
            &self.file_name,
            span,
            message,
            hint,
        )
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }
}
