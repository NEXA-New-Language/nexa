//! `nexa lint`: advisory checks over the parsed AST.
//!
//! The linter is purely syntactic: it runs on a successful parse, before
//! (and independently of) semantic analysis, and only ever produces
//! warnings. Rules in this first version:
//!
//! * unused variables (`let` / `var` / `const`), parameters and UI state
//!   — a declaration that is never read;
//! * unused actions — an `action` that is never called;
//! * shadowing — a declaration that hides a variable from an outer scope.
//!
//! An assignment target is a write, not a read: `x = x + 1` counts as a
//! read of `x` through its right-hand side, but `x = 5` alone does not.

use crate::diagnostics::Diagnostic;
use crate::span::Span;
use crate::syntax::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclKind {
    Variable,
    Param,
    State,
    Action,
}

impl DeclKind {
    fn unused_kind(self) -> &'static str {
        match self {
            DeclKind::Variable => "unused variable",
            DeclKind::Param => "unused parameter",
            DeclKind::State => "unused state",
            DeclKind::Action => "unused action",
        }
    }

    fn unused_noun(self) -> &'static str {
        match self {
            DeclKind::Variable => "Variable",
            DeclKind::Param => "Parameter",
            DeclKind::State => "State",
            DeclKind::Action => "Action",
        }
    }

    fn unused_verb(self) -> &'static str {
        match self {
            DeclKind::Action => "called",
            _ => "read",
        }
    }
}

#[derive(Debug, Clone)]
struct Decl {
    name: String,
    span: Span,
    kind: DeclKind,
    used: bool,
}

struct Linter {
    file_name: String,
    /// Lexical scopes, innermost last. Actions live separately because
    /// they are visible across the whole program.
    scopes: Vec<HashMap<String, Decl>>,
    actions: HashMap<String, Decl>,
    warnings: Vec<Diagnostic>,
}

impl Linter {
    fn new(file_name: &str) -> Self {
        Self {
            file_name: file_name.to_string(),
            scopes: Vec::new(),
            actions: HashMap::new(),
            warnings: Vec::new(),
        }
    }

    fn warn(&mut self, kind: &str, span: Span, message: String, hint: Option<String>) {
        self.warnings.push(Diagnostic::warning(
            kind,
            self.file_name.clone(),
            span,
            message,
            hint,
        ));
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop a scope, warning about every declaration in it that was never used.
    fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for decl in scope.values() {
                if !decl.used {
                    let kind = decl.kind.unused_kind();
                    let message = format!(
                        "{} '{}' is never {}.",
                        decl.kind.unused_noun(),
                        decl.name,
                        decl.kind.unused_verb()
                    );
                    let hint = if decl.kind == DeclKind::Action {
                        Some("Remove the action if it is not needed.".to_string())
                    } else {
                        Some("Remove the declaration if it is not needed.".to_string())
                    };
                    self.warn(kind, decl.span, message, hint);
                }
            }
        }
    }

    /// Declare a name in the current scope. Warns when it hides a name
    /// from an enclosing scope (a duplicate in the *same* scope is the
    /// semantic analyzer's error, not a lint).
    fn declare(&mut self, name: &Ident, kind: DeclKind) {
        if self
            .scopes
            .iter()
            .rev()
            .skip(1)
            .any(|scope| scope.contains_key(&name.name))
        {
            self.warn(
                "shadowing",
                name.span,
                format!(
                    "{} '{}' shadows a variable from an outer scope.",
                    kind.unused_noun(),
                    name.name
                ),
                Some("Rename one of them to keep the code clear.".to_string()),
            );
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name.name.clone(),
                Decl {
                    name: name.name.clone(),
                    span: name.span,
                    kind,
                    used: false,
                },
            );
        }
    }

    fn mark_used(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(decl) = scope.get_mut(name) {
                decl.used = true;
                return;
            }
        }
    }

    fn walk_block(&mut self, block: &Block) {
        self.push_scope();
        for stmt in &block.statements {
            self.walk_stmt(stmt);
        }
        self.pop_scope();
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, value, .. }
            | Stmt::Var { name, value, .. }
            | Stmt::Const { name, value, .. } => {
                self.walk_expr(value);
                self.declare(name, DeclKind::Variable);
            }
            Stmt::Assign { value, .. } => {
                // The target is a write, not a read.
                self.walk_expr(value);
            }
            Stmt::Action {
                name, params, body, ..
            } => {
                let _ = name;
                self.push_scope();
                for param in params {
                    self.declare(param, DeclKind::Param);
                }
                for stmt in &body.statements {
                    self.walk_stmt(stmt);
                }
                self.pop_scope();
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.walk_expr(condition);
                self.walk_block(then_branch);
                if let Some(other) = else_branch {
                    self.walk_block(other);
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                self.walk_expr(condition);
                self.walk_block(body);
            }
            Stmt::Repeat { count, body, .. } => {
                self.walk_expr(count);
                self.walk_block(body);
            }
            Stmt::UiApp { app, .. } => self.walk_ui_app(app),
            Stmt::Expr(expr) => self.walk_expr(expr),
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal { .. } => {}
            Expr::Variable { name } => self.mark_used(&name.name),
            Expr::Unary { expr, .. } => self.walk_expr(expr),
            Expr::Binary { left, right, .. } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::Call { callee, args, .. } => {
                if callee.name != "print" && callee.name != "assert" {
                    if let Some(action) = self.actions.get_mut(&callee.name) {
                        action.used = true;
                    }
                }
                for arg in args {
                    self.walk_expr(arg);
                }
            }
        }
    }

    fn walk_ui_app(&mut self, app: &UiApp) {
        self.push_scope();
        for state in &app.states {
            self.declare(&state.name, DeclKind::State);
        }
        for state in &app.states {
            self.walk_expr(&state.value);
        }
        if let Some(window) = &app.window {
            for item in &window.items {
                self.walk_ui_item(item);
            }
        }
        self.pop_scope();
    }

    fn walk_ui_item(&mut self, item: &UiItem) {
        match item {
            UiItem::Property(property) => {
                for arg in &property.args {
                    self.walk_expr(arg);
                }
            }
            UiItem::Component(component) => self.walk_ui_component(component),
            UiItem::EventHandler(handler) => self.walk_block(&handler.body),
            UiItem::ChangeHandler(handler) => {
                self.push_scope();
                self.declare(&handler.param, DeclKind::Param);
                for stmt in &handler.body.statements {
                    self.walk_stmt(stmt);
                }
                self.pop_scope();
            }
        }
    }

    fn walk_ui_component(&mut self, component: &UiComponent) {
        match component {
            UiComponent::Layout { items, .. } => {
                for item in items {
                    self.walk_ui_item(item);
                }
            }
            UiComponent::Text { value, .. } => self.walk_expr(value),
            UiComponent::Button {
                label, handler, ..
            } => {
                self.walk_expr(label);
                if let Some(handler) = handler {
                    self.walk_block(&handler.body);
                }
            }
            UiComponent::TextField { bound, handler, .. }
            | UiComponent::Checkbox { bound, handler, .. } => {
                self.walk_expr(bound);
                if let Some(handler) = handler {
                    self.push_scope();
                    self.declare(&handler.param, DeclKind::Param);
                    for stmt in &handler.body.statements {
                        self.walk_stmt(stmt);
                    }
                    self.pop_scope();
                }
            }
            UiComponent::Spacer { .. } | UiComponent::Divider { .. } => {}
            UiComponent::Image { source, .. } => self.walk_expr(source),
        }
    }
}

/// Run the linter over an already-parsed program. Warnings are sorted by
/// source position so output is deterministic.
pub fn lint_program(file_name: &str, program: &Program) -> Vec<Diagnostic> {
    let mut linter = Linter::new(file_name);

    // Actions are visible across the whole program, so collect them first.
    for stmt in &program.statements {
        if let Stmt::Action { name, .. } = stmt {
            linter.actions.entry(name.name.clone()).or_insert(Decl {
                name: name.name.clone(),
                span: name.span,
                kind: DeclKind::Action,
                used: false,
            });
        }
    }

    linter.push_scope();
    for stmt in &program.statements {
        linter.walk_stmt(stmt);
    }
    linter.pop_scope();

    for action in linter.actions.values() {
        if !action.used {
            linter.warnings.push(Diagnostic::warning(
                "unused action",
                file_name,
                action.span,
                format!("Action '{}' is never called.", action.name),
                Some("Remove the action if it is not needed.".to_string()),
            ));
        }
    }

    linter
        .warnings
        .sort_by_key(|w| (w.span.line, w.span.col, w.kind.clone()));
    linter.warnings
}
