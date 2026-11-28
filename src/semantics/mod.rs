use crate::diagnostics::Diagnostic;
use crate::span::Span;
use crate::syntax::ast::*;
use crate::types::Type;
use std::collections::{HashMap, HashSet};

/// Type information produced by semantic analysis and consumed by the native
/// backend. Spans identify AST nodes; binding ids are stable across scopes.
#[derive(Debug, Clone)]
pub struct AnalyzedProgram {
    pub program: Program,
    pub expr_types: HashMap<Span, Type>,
    pub bindings: HashMap<Span, usize>,
    pub slot_types: Vec<Type>,
    pub repeat_counters: HashMap<Span, usize>,
    pub actions: HashMap<String, ActionSignature>,
    /// Slots of `state` variables declared inside an `app` block. The
    /// native backend stores these in a global state array instead of the
    /// stack frame, so handlers and the render function can share them.
    pub state_slots: HashSet<usize>,
}

#[derive(Debug, Clone)]
pub struct ActionSignature {
    pub params: Vec<Type>,
    pub ret: Type,
}

#[derive(Clone)]
struct Variable {
    ty: Type,
    mutable: bool,
    slot: usize,
}

pub struct SemanticAnalyzer {
    file_name: String,
    scopes: Vec<HashMap<String, Variable>>,
    actions: HashMap<String, ActionSignature>,
    analyzed: Option<AnalyzedProgram>,
    state_slots: HashSet<usize>,
}

impl SemanticAnalyzer {
    pub fn new(file_name: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            scopes: vec![HashMap::new()],
            actions: HashMap::new(),
            analyzed: None,
            state_slots: HashSet::new(),
        }
    }

    /// Compatibility entry point: validate without retaining the typed result.
    pub fn analyze(&mut self, program: &Program) -> Result<(), Diagnostic> {
        self.analyze_program(program).map(|_| ())
    }

    pub fn analyze_program(&mut self, program: &Program) -> Result<AnalyzedProgram, Diagnostic> {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
        self.actions.clear();
        self.state_slots.clear();

        // A UI application is the whole program (spec section 7): it cannot
        // be mixed with ordinary top-level statements or actions.
        if program.statements.iter().any(|s| matches!(s, Stmt::UiApp { .. }))
            && program.statements.len() != 1
        {
            let span = program
                .statements
                .iter()
                .find(|s| matches!(s, Stmt::UiApp { .. }))
                .map(|s| s.span())
                .unwrap_or(program.span);
            return Err(self.error(
                span,
                "app(...) must be the only top-level item of a UI program.",
                Some("Move ordinary statements inside an event handler, or write a separate non-UI program.".into()),
            ));
        }

        let mut declarations = Vec::new();
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
                declarations.push((name, params.as_slice(), stmt));
                self.actions.insert(
                    name.name.clone(),
                    ActionSignature {
                        params: vec![Type::String; params.len()],
                        ret: Type::Void,
                    },
                );
            }
        }

        let signatures = self.infer_actions(&declarations, program)?;
        self.actions = signatures;

        let analyzed = AnalyzedProgram {
            program: program.clone(),
            expr_types: HashMap::new(),
            bindings: HashMap::new(),
            slot_types: Vec::new(),
            repeat_counters: HashMap::new(),
            actions: self.actions.clone(),
            state_slots: HashSet::new(),
        };
        self.analyzed = Some(analyzed);

        for stmt in &program.statements {
            self.analyze_stmt(stmt)?;
        }

        let mut analyzed = self.analyzed.take().unwrap();
        analyzed.state_slots = self.state_slots.clone();
        Ok(analyzed)
    }

    fn infer_actions(
        &self,
        declarations: &[(&Ident, &[Ident], &Stmt)],
        program: &Program,
    ) -> Result<HashMap<String, ActionSignature>, Diagnostic> {
        let mut solver = Inference::default();
        for (name, params, _) in declarations {
            for (index, _) in params.iter().enumerate() {
                solver.add_param(&name.name, index);
            }
        }

        for (name, params, stmt) in declarations {
            if let Stmt::Action { body, .. } = stmt {
                let mut ctx = InferCtx {
                    action: &name.name,
                    params,
                    solver: &mut solver,
                };
                for statement in &body.statements {
                    ctx.walk_stmt(statement);
                }
            }
        }

        for stmt in &program.statements {
            if !matches!(stmt, Stmt::Action { .. }) {
                let mut ctx = InferCtx {
                    action: "__top",
                    params: &[],
                    solver: &mut solver,
                };
                ctx.walk_stmt(stmt);
            }
        }

        solver.solve()?;

        let mut result = HashMap::new();
        for (name, params, _) in declarations {
            let types = params
                .iter()
                .enumerate()
                .map(|(i, _)| solver.param_type(&name.name, i))
                .collect();
            result.insert(
                name.name.clone(),
                ActionSignature {
                    params: types,
                    ret: Type::Void,
                },
            );
        }
        Ok(result)
    }

    fn analyze_stmt(&mut self, stmt: &Stmt) -> Result<(), Diagnostic> {
        match stmt {
            Stmt::Let { name, value, span } => {
                let ty = self.analyze_expr(value)?;
                self.declare(name, ty, false, *span)?;
            }
            Stmt::Var { name, value, span } => {
                let ty = self.analyze_expr(value)?;
                self.declare(name, ty, true, *span)?;
            }
            Stmt::Const { name, value, span } => {
                let ty = self.analyze_expr(value)?;
                self.declare(name, ty, false, *span)?;
            }
            Stmt::Assign { target, value, .. } => {
                let value_type = self.analyze_expr(value)?;
                let variable = self.lookup_variable(&target.name).ok_or_else(|| {
                    self.error(
                        target.span,
                        format!("Undefined variable '{}'.", target.name),
                        Some("Declare the variable before assigning to it.".into()),
                    )
                })?;
                if !variable.mutable {
                    return Err(self.error(
                        target.span,
                        format!("Cannot assign to immutable variable '{}'.", target.name),
                        Some("Use 'var' when the variable needs reassignment.".into()),
                    ));
                }
                if variable.ty != value_type {
                    return Err(self.type_error(target.span, &variable.ty, &value_type));
                }
                self.record_binding(target.span, variable.slot);
            }
            Stmt::Action {
                name, params, body, ..
            } => {
                if self.scopes.len() != 1 {
                    return Err(self.error(
                        name.span,
                        "Nested action declarations are not supported.",
                        None,
                    ));
                }
                let signature = self.actions.get(&name.name).cloned().unwrap();
                self.push_scope();
                for (param, ty) in params.iter().zip(signature.params.iter()) {
                    self.declare(param, ty.clone(), true, param.span)?;
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
            Stmt::While {
                condition, body, ..
            } => {
                let condition_type = self.analyze_expr(condition)?;
                self.require_type(&condition_type, &Type::Bool, condition.span())?;
                self.analyze_block(body)?;
            }
            Stmt::Repeat { count, body, span } => {
                let count_type = self.analyze_expr(count)?;
                self.require_type(&count_type, &Type::Int, count.span())?;
                let slot = self.new_slot(Type::Int);
                self.analyzed
                    .as_mut()
                    .unwrap()
                    .repeat_counters
                    .insert(*span, slot);
                self.analyze_block(body)?;
            }
            Stmt::UiApp { app, .. } => {
                self.analyze_ui_app(app)?;
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

    // ------------------------------------------------------------------
    // NEXA UI 1.0 semantic analysis (spec sections 3-7).
    // ------------------------------------------------------------------

    fn analyze_ui_app(&mut self, app: &UiApp) -> Result<(), Diagnostic> {
        if self.scopes.len() != 1 {
            return Err(self.error(
                app.span,
                "Nested app(...) blocks are not supported.",
                None,
            ));
        }
        // State declarations are registered before the Window so that
        // every component and handler can refer to them.
        for state in &app.states {
            let ty = self.analyze_expr(&state.value)?;
            match ty {
                Type::Int | Type::Float | Type::String | Type::Bool => {}
                _ => {
                    return Err(self.error(
                        state.value.span(),
                        format!(
                            "state '{}' must start as Int, Float, String or Bool, found {:?}.",
                            state.name.name, ty
                        ),
                        None,
                    ));
                }
            }
            self.declare(&state.name, ty, true, state.name.span)?;
            let slot = self
                .analyzed
                .as_ref()
                .unwrap()
                .bindings
                .get(&state.name.span)
                .copied()
                .expect("state binding");
            self.state_slots.insert(slot);
        }
        let window = app.window.as_ref().ok_or_else(|| {
            self.error(
                app.span,
                "A UI application requires exactly one Window.",
                Some("Add a Window { ... } block inside app(...).".into()),
            )
        })?;
        self.analyze_ui_items(&window.items, true)
    }

    fn analyze_ui_items(&mut self, items: &[UiItem], is_window: bool) -> Result<(), Diagnostic> {
        let mut seen: HashSet<String> = HashSet::new();
        for item in items {
            match item {
                UiItem::Property(prop) => {
                    self.analyze_ui_property(prop, is_window, &mut seen)?;
                }
                UiItem::Component(comp) => {
                    self.analyze_ui_component(comp)?;
                }
                UiItem::EventHandler(handler) => {
                    return Err(self.error(
                        handler.span,
                        "on_click is only allowed directly on a Button.",
                        Some("Move the handler inside Button(...) { on_click { ... } }.".into()),
                    ));
                }
                UiItem::ChangeHandler(handler) => {
                    return Err(self.error(
                        handler.span,
                        "on_change is only allowed directly on a TextField or Checkbox.",
                        None,
                    ));
                }
            }
        }
        Ok(())
    }

    fn analyze_ui_property(
        &mut self,
        prop: &UiProperty,
        is_window: bool,
        seen: &mut HashSet<String>,
    ) -> Result<(), Diagnostic> {
        if !seen.insert(prop.name.name.clone()) {
            return Err(self.error(
                prop.name.span,
                format!("Duplicate property '{}'.", prop.name.name),
                None,
            ));
        }
        let expect_args = |n: usize, prop: &UiProperty, this: &Self| -> Result<(), Diagnostic> {
            if prop.args.len() != n {
                return Err(this.error(
                    prop.span,
                    format!(
                        "Property '{}' expects {} argument(s), but received {}.",
                        prop.name.name,
                        n,
                        prop.args.len()
                    ),
                    None,
                ));
            }
            Ok(())
        };
        match prop.name.name.as_str() {
            "title" => {
                if !is_window {
                    return Err(self.error(
                        prop.name.span,
                        "Property 'title' is only allowed on Window.",
                        None,
                    ));
                }
                expect_args(1, prop, self)?;
                let ty = self.analyze_expr(&prop.args[0])?;
                self.require_type(&ty, &Type::String, prop.args[0].span())?;
            }
            "size" => {
                if !is_window {
                    return Err(self.error(
                        prop.name.span,
                        "Property 'size' is only allowed on Window.",
                        None,
                    ));
                }
                expect_args(2, prop, self)?;
                for arg in &prop.args {
                    let ty = self.analyze_expr(arg)?;
                    self.require_type(&ty, &Type::Int, arg.span())?;
                }
            }
            "width" | "height" | "padding" | "margin" | "spacing" => {
                expect_args(1, prop, self)?;
                let ty = self.analyze_expr(&prop.args[0])?;
                if ty != Type::Int && ty != Type::Float {
                    return Err(self.error(
                        prop.args[0].span(),
                        format!(
                            "Property '{}' expects a numeric value, found {:?}.",
                            prop.name.name, ty
                        ),
                        None,
                    ));
                }
            }
            "background" => {
                expect_args(1, prop, self)?;
                let ty = self.analyze_expr(&prop.args[0])?;
                self.require_type(&ty, &Type::String, prop.args[0].span())?;
                if let Expr::Literal {
                    lit: Literal::Str(value),
                    ..
                } = &prop.args[0]
                {
                    if !is_rrggbb(value) {
                        return Err(self.error(
                            prop.args[0].span(),
                            format!(
                                "Background color '{}' is not a valid #RRGGBB color.",
                                value
                            ),
                            Some("Use the form \"#RRGGBB\", for example \"#1E90FF\".".into()),
                        ));
                    }
                }
            }
            "align" => {
                expect_args(1, prop, self)?;
                let ty = self.analyze_expr(&prop.args[0])?;
                self.require_type(&ty, &Type::String, prop.args[0].span())?;
            }
            "visible" => {
                expect_args(1, prop, self)?;
                let ty = self.analyze_expr(&prop.args[0])?;
                self.require_type(&ty, &Type::Bool, prop.args[0].span())?;
            }
            other => {
                return Err(self.error(
                    prop.name.span,
                    format!("Unknown property '{}'.", other),
                    Some(
                        "Valid properties: width, height, padding, margin, spacing, background, align, visible, title (Window only), size (Window only)."
                            .into(),
                    ),
                ));
            }
        }
        Ok(())
    }

    fn analyze_ui_component(&mut self, comp: &UiComponent) -> Result<(), Diagnostic> {
        match comp {
            UiComponent::Layout { items, .. } => self.analyze_ui_items(items, false),
            UiComponent::Text { value, .. } => {
                let ty = self.analyze_expr(value)?;
                match ty {
                    Type::Int | Type::Float | Type::String | Type::Bool => Ok(()),
                    _ => Err(self.error(
                        value.span(),
                        format!("Text() expects Int, Float, String or Bool, found {:?}.", ty),
                        None,
                    )),
                }
            }
            UiComponent::Button { label, handler, .. } => {
                let ty = self.analyze_expr(label)?;
                self.require_type(&ty, &Type::String, label.span())?;
                if let Some(h) = handler {
                    self.analyze_ui_handler_body(&h.body, None)?;
                }
                Ok(())
            }
            UiComponent::TextField { bound, handler, .. } => {
                self.check_ui_bound(bound, &Type::String, "TextField")?;
                if let Some(h) = handler {
                    self.analyze_ui_handler_body(&h.body, Some((&h.param, Type::String)))?;
                }
                Ok(())
            }
            UiComponent::Checkbox { bound, handler, .. } => {
                self.check_ui_bound(bound, &Type::Bool, "Checkbox")?;
                if let Some(h) = handler {
                    self.analyze_ui_handler_body(&h.body, Some((&h.param, Type::Bool)))?;
                }
                Ok(())
            }
            UiComponent::Spacer { .. } | UiComponent::Divider { .. } => Ok(()),
            UiComponent::Image { source, .. } => {
                let ty = self.analyze_expr(source)?;
                self.require_type(&ty, &Type::String, source.span())?;
                Ok(())
            }
        }
    }

    /// TextField/Checkbox must be bound to a `state` variable of the right
    /// type, e.g. `TextField(name)` / `Checkbox(checked)`.
    fn check_ui_bound(&mut self, bound: &Expr, expected: &Type, comp_name: &str) -> Result<(), Diagnostic> {
        let name = match bound {
            Expr::Variable { name } => name,
            _ => {
                return Err(self.error(
                    bound.span(),
                    format!(
                        "{} expects a state variable, for example {}(name).",
                        comp_name, comp_name
                    ),
                    Some("Declare the value with 'state' inside app(...).".into()),
                ));
            }
        };
        // Analyze the bound expression so the backend has its type and
        // binding recorded, like every other expression.
        let ty = self.analyze_expr(bound)?;
        let variable = self.lookup_variable(&name.name).ok_or_else(|| {
            self.error(
                name.span,
                format!("Undefined variable '{}'.", name.name),
                Some("Declare it with 'state' inside app(...).".into()),
            )
        })?;
        if !self.state_slots.contains(&variable.slot) {
            return Err(self.error(
                name.span,
                format!(
                    "{} must be bound to a 'state' variable, but '{}' is not state.",
                    comp_name, name.name
                ),
                None,
            ));
        }
        if &ty != expected {
            return Err(self.type_error(name.span, expected, &ty));
        }
        self.record_binding(name.span, variable.slot);
        Ok(())
    }

    fn analyze_ui_handler_body(
        &mut self,
        body: &Block,
        param: Option<(&Ident, Type)>,
    ) -> Result<(), Diagnostic> {
        self.push_scope();
        if let Some((name, ty)) = param {
            self.declare(name, ty, true, name.span)?;
        }
        for stmt in &body.statements {
            self.analyze_stmt(stmt)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn analyze_expr(&mut self, expr: &Expr) -> Result<Type, Diagnostic> {
        let ty = match expr {
            Expr::Literal { lit, .. } => match lit {
                Literal::Int(_) => Type::Int,
                Literal::Float(_) => Type::Float,
                Literal::Str(_) => Type::String,
                Literal::Bool(_) => Type::Bool,
            },
            Expr::Variable { name } => {
                let variable = self.lookup_variable(&name.name).ok_or_else(|| {
                    self.error(
                        name.span,
                        format!("Undefined variable '{}'.", name.name),
                        Some("Declare the variable before using it.".into()),
                    )
                })?;
                self.record_binding(name.span, variable.slot);
                variable.ty
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
                        operand
                    }
                    UnaryOp::Not => {
                        self.require_type(&operand, &Type::Bool, *span)?;
                        Type::Bool
                    }
                }
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                let left_type = self.analyze_expr(left)?;
                let right_type = self.analyze_expr(right)?;
                match op {
                    BinaryOp::Add => {
                        if left_type == Type::String && right_type == Type::String {
                            Type::String
                        } else if left_type == right_type
                            && (left_type == Type::Int || left_type == Type::Float)
                        {
                            left_type
                        } else {
                            return Err(self.error(
                                *span,
                                "Addition requires two values of the same numeric type or two strings.",
                                None,
                            ));
                        }
                    }
                    BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                        if left_type == right_type
                            && (left_type == Type::Int || left_type == Type::Float)
                        {
                            left_type
                        } else {
                            return Err(self.error(
                                *span,
                                "Arithmetic operation requires matching numeric types.",
                                None,
                            ));
                        }
                    }
                    BinaryOp::Equal | BinaryOp::NotEqual => {
                        if left_type == right_type && left_type != Type::Void {
                            Type::Bool
                        } else {
                            return Err(self.error(
                                *span,
                                "Equality comparison requires matching non-void types.",
                                None,
                            ));
                        }
                    }
                    BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq => {
                        if left_type == right_type
                            && (left_type == Type::Int || left_type == Type::Float)
                        {
                            Type::Bool
                        } else {
                            return Err(self.error(
                                *span,
                                "Comparison requires matching numeric types.",
                                None,
                            ));
                        }
                    }
                    BinaryOp::And | BinaryOp::Or => {
                        self.require_type(&left_type, &Type::Bool, left.span())?;
                        self.require_type(&right_type, &Type::Bool, right.span())?;
                        Type::Bool
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
                        return Err(self.error(args[0].span(), "Cannot print a void value.", None));
                    }
                    Type::Void
                } else {
                    let signature = self.actions.get(&callee.name).cloned().ok_or_else(|| {
                        self.error(
                            callee.span,
                            format!("Undefined action '{}'.", callee.name),
                            None,
                        )
                    })?;
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
                    Type::Void
                }
            }
        };
        self.analyzed
            .as_mut()
            .unwrap()
            .expr_types
            .insert(expr.span(), ty.clone());
        Ok(ty)
    }

    fn declare(
        &mut self,
        name: &Ident,
        ty: Type,
        mutable: bool,
        _span: Span,
    ) -> Result<(), Diagnostic> {
        if self
            .scopes
            .iter()
            .any(|scope| scope.contains_key(&name.name))
        {
            return Err(self.error(
                name.span,
                format!("Variable '{}' is already declared.", name.name),
                None,
            ));
        }
        let slot = self.new_slot(ty.clone());
        self.record_binding(name.span, slot);
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.name.clone(), Variable { ty, mutable, slot });
        Ok(())
    }

    fn new_slot(&mut self, ty: Type) -> usize {
        let analyzed = self.analyzed.as_mut().unwrap();
        let slot = analyzed.slot_types.len();
        analyzed.slot_types.push(ty);
        slot
    }

    fn record_binding(&mut self, span: Span, slot: usize) {
        self.analyzed.as_mut().unwrap().bindings.insert(span, slot);
    }

    fn lookup_variable(&self, name: &str) -> Option<Variable> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }

    fn require_type(&self, actual: &Type, expected: &Type, span: Span) -> Result<(), Diagnostic> {
        if actual != expected {
            return Err(self.type_error(span, expected, actual));
        }
        Ok(())
    }

    fn type_error(&self, span: Span, expected: &Type, actual: &Type) -> Diagnostic {
        self.error(
            span,
            format!(
                "Type mismatch: expected {:?}, found {:?}.",
                expected, actual
            ),
            None,
        )
    }

    fn error(&self, span: Span, message: impl Into<String>, hint: Option<String>) -> Diagnostic {
        Diagnostic::error("Semantic Error", &self.file_name, span, message, hint)
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

// ---------------------------------------------------------------------------
// Monomorphic parameter inference for the existing untyped action syntax.
// Parameters default to String unless numeric/boolean use or a call constrains
// them. This preserves the historical `greet(target)` string behavior while
// allowing numeric actions without introducing type-annotation syntax.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Inference {
    parent: Vec<usize>,
    ty: Vec<Option<Type>>,
    numeric: Vec<bool>,
    allow_call_constraints: Vec<bool>,
    ids: HashMap<(String, usize), usize>,
    constraints: Vec<(usize, Type, Span)>,
    conflicts: Vec<(Type, Type, Span)>,
}

impl Inference {
    fn add_param(&mut self, action: &str, index: usize) {
        let id = self.parent.len();
        self.parent.push(id);
        self.ty.push(None);
        self.numeric.push(false);
        self.allow_call_constraints.push(false);
        self.ids.insert((action.to_string(), index), id);
    }

    fn find(&mut self, id: usize) -> usize {
        if self.parent[id] != id {
            let root = self.find(self.parent[id]);
            self.parent[id] = root;
        }
        self.parent[id]
    }

    fn link(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        if let (Some(a), Some(b)) = (&self.ty[ra], &self.ty[rb]) {
            if a != b {
                self.conflicts
                    .push((a.clone(), b.clone(), Span::new(0, 0, 1, 1)));
            }
        }
        self.parent[rb] = ra;
        self.numeric[ra] |= self.numeric[rb];
        self.allow_call_constraints[ra] |= self.allow_call_constraints[rb];
        if self.ty[ra].is_none() {
            self.ty[ra] = self.ty[rb].clone();
        }
    }

    fn constrain(&mut self, id: usize, ty: Type, span: Span) {
        let root = self.find(id);
        self.allow_call_constraints[root] = true;
        self.constraints.push((id, ty, span));
    }

    fn constrain_call(&mut self, id: usize, ty: Type, span: Span) {
        self.constraints.push((id, ty, span));
    }

    fn allow_call(&mut self, id: usize) {
        let root = self.find(id);
        self.allow_call_constraints[root] = true;
    }

    fn mark_numeric(&mut self, id: usize) {
        let root = self.find(id);
        self.numeric[root] = true;
    }

    fn solve(&mut self) -> Result<(), Diagnostic> {
        if let Some((a, b, span)) = self.conflicts.first() {
            return Err(Diagnostic::error(
                "Semantic Error",
                "<inference>",
                *span,
                format!(
                    "Action parameter is used with incompatible types: {:?} and {:?}.",
                    a, b
                ),
                Some("Use separate actions for different parameter types.".into()),
            ));
        }
        for (id, ty, span) in std::mem::take(&mut self.constraints) {
            let root = self.find(id);
            if let Some(existing) = &self.ty[root] {
                if existing != &ty {
                    return Err(Diagnostic::error(
                        "Semantic Error",
                        "<inference>",
                        span,
                        format!(
                            "Action parameter is used with incompatible types: {:?} and {:?}.",
                            existing, ty
                        ),
                        Some("Use separate actions for different parameter types.".into()),
                    ));
                }
            } else {
                self.ty[root] = Some(ty);
            }
        }
        for i in 0..self.parent.len() {
            let root = self.find(i);
            if self.numeric[root] {
                match &self.ty[root] {
                    None => self.ty[root] = Some(Type::Int),
                    Some(Type::Int) | Some(Type::Float) => {}
                    Some(other) => {
                        return Err(Diagnostic::error(
                            "Semantic Error",
                            "<inference>",
                            Span::new(0, 0, 1, 1),
                            format!(
                                "Action parameter of type {:?} cannot be used numerically.",
                                other
                            ),
                            None,
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn param_type(&self, action: &str, index: usize) -> Type {
        let id = self.ids[&(action.to_string(), index)];
        let mut root = id;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        self.ty[root].clone().unwrap_or(Type::String)
    }
}

struct InferCtx<'a> {
    action: &'a str,
    params: &'a [Ident],
    solver: &'a mut Inference,
}

impl<'a> InferCtx<'a> {
    fn param_id(&self, name: &str) -> Option<usize> {
        self.params
            .iter()
            .position(|p| p.name == name)
            .and_then(|i| self.solver.ids.get(&(self.action.to_string(), i)).copied())
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { value, .. } | Stmt::Var { value, .. } | Stmt::Const { value, .. } => {
                self.walk_expr(value);
            }
            Stmt::Assign { target, value, .. } => {
                self.walk_expr(value);
                if let Some(id) = self.param_id(&target.name) {
                    if let Some(t) = self.simple_type(value) {
                        self.solver.constrain(id, t, target.span);
                    }
                }
            }
            Stmt::Expr(expr) => self.walk_expr(expr),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.walk_expr(condition);
                if let Some(id) = variable_param(condition).and_then(|n| self.param_id(n)) {
                    self.solver.constrain(id, Type::Bool, condition.span());
                }
                for s in &then_branch.statements {
                    self.walk_stmt(s);
                }
                if let Some(block) = else_branch {
                    for s in &block.statements {
                        self.walk_stmt(s);
                    }
                }
            }
            Stmt::While {
                condition, body, ..
            } => {
                self.walk_expr(condition);
                if let Some(id) = variable_param(condition).and_then(|n| self.param_id(n)) {
                    self.solver.constrain(id, Type::Bool, condition.span());
                }
                for s in &body.statements {
                    self.walk_stmt(s);
                }
            }
            Stmt::Repeat { count, body, .. } => {
                self.walk_expr(count);
                if let Some(id) = variable_param(count).and_then(|n| self.param_id(n)) {
                    self.solver.constrain(id, Type::Int, count.span());
                }
                for s in &body.statements {
                    self.walk_stmt(s);
                }
            }
            Stmt::Action { .. } => {}
            Stmt::UiApp { app, .. } => {
                for state in &app.states {
                    self.walk_expr(&state.value);
                }
                if let Some(window) = &app.window {
                    for item in &window.items {
                        self.walk_ui_item(item);
                    }
                }
            }
        }
    }

    fn walk_ui_item(&mut self, item: &UiItem) {
        match item {
            UiItem::Property(prop) => {
                for arg in &prop.args {
                    self.walk_expr(arg);
                }
            }
            UiItem::Component(comp) => self.walk_ui_component(comp),
            UiItem::EventHandler(handler) => {
                for s in &handler.body.statements {
                    self.walk_stmt(s);
                }
            }
            UiItem::ChangeHandler(handler) => {
                for s in &handler.body.statements {
                    self.walk_stmt(s);
                }
            }
        }
    }

    fn walk_ui_component(&mut self, comp: &UiComponent) {
        match comp {
            UiComponent::Layout { items, .. } => {
                for item in items {
                    self.walk_ui_item(item);
                }
            }
            UiComponent::Text { value, .. } | UiComponent::Image { source: value, .. } => {
                self.walk_expr(value);
            }
            UiComponent::Button { label, handler, .. } => {
                self.walk_expr(label);
                if let Some(h) = handler {
                    for s in &h.body.statements {
                        self.walk_stmt(s);
                    }
                }
            }
            UiComponent::TextField { bound, handler, .. }
            | UiComponent::Checkbox { bound, handler, .. } => {
                self.walk_expr(bound);
                if let Some(h) = handler {
                    for s in &h.body.statements {
                        self.walk_stmt(s);
                    }
                }
            }
            UiComponent::Spacer { .. } | UiComponent::Divider { .. } => {}
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Unary { op, expr, .. } => {
                self.walk_expr(expr);
                if let Expr::Variable { name } = expr.as_ref() {
                    if let Some(id) = self.param_id(&name.name) {
                        match op {
                            UnaryOp::Negate => self.solver.mark_numeric(id),
                            UnaryOp::Not => self.solver.constrain(id, Type::Bool, name.span),
                        }
                    }
                }
            }
            Expr::Binary {
                left, op, right, ..
            } => {
                self.walk_expr(left);
                self.walk_expr(right);
                let numeric = matches!(
                    op,
                    BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Mod
                        | BinaryOp::Less
                        | BinaryOp::LessEq
                        | BinaryOp::Greater
                        | BinaryOp::GreaterEq
                );
                let lid = variable_param(left).and_then(|n| self.param_id(n));
                let rid = variable_param(right).and_then(|n| self.param_id(n));
                if numeric {
                    if let Some(id) = lid {
                        self.solver.mark_numeric(id);
                    }
                    if let Some(id) = rid {
                        self.solver.mark_numeric(id);
                    }
                    if let Some(t) = self.simple_type(left) {
                        if matches!(t, Type::Int | Type::Float) {
                            if let Some(id) = rid {
                                self.solver.constrain(id, t, left.span());
                            }
                        }
                    }
                    if let Some(t) = self.simple_type(right) {
                        if matches!(t, Type::Int | Type::Float) {
                            if let Some(id) = lid {
                                self.solver.constrain(id, t, right.span());
                            }
                        }
                    }
                }
                if matches!(op, BinaryOp::Equal | BinaryOp::NotEqual) {
                    if let Some(t) = self.simple_type(left) {
                        if let Some(id) = rid {
                            self.solver.constrain(id, t, left.span());
                        }
                    }
                    if let Some(t) = self.simple_type(right) {
                        if let Some(id) = lid {
                            self.solver.constrain(id, t, right.span());
                        }
                    }
                }
                if *op == BinaryOp::Add {
                    if let Some(t) = self.simple_type(left) {
                        if let Some(id) = rid {
                            self.solver.constrain(id, t, right.span());
                        }
                    }
                    if let Some(t) = self.simple_type(right) {
                        if let Some(id) = lid {
                            self.solver.constrain(id, t, left.span());
                        }
                    }
                }
                if *op == BinaryOp::Add {
                    for id in self
                        .add_param_ids(left)
                        .into_iter()
                        .chain(self.add_param_ids(right))
                    {
                        self.solver.allow_call(id);
                    }
                }
                if let (Some(a), Some(b)) = (lid, rid) {
                    self.solver.link(a, b);
                }
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    if let Some(id) = lid {
                        self.solver.constrain(id, Type::Bool, left.span());
                    }
                    if let Some(id) = rid {
                        self.solver.constrain(id, Type::Bool, right.span());
                    }
                }
            }
            Expr::Call { callee, args, .. } => {
                for arg in args {
                    self.walk_expr(arg);
                }
                if callee.name == "print" {
                    return;
                }
                for (index, arg) in args.iter().enumerate() {
                    let target = self.solver.ids.get(&(callee.name.clone(), index)).copied();
                    if let Some(target) = target {
                        if let Some(t) = self.simple_type(arg) {
                            self.solver.constrain_call(target, t, arg.span());
                        }
                        if let Some(name) = variable_param(arg) {
                            if let Some(source) = self.param_id(name) {
                                self.solver.link(source, target);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn add_param_ids(&self, expr: &Expr) -> Vec<usize> {
        let mut ids = Vec::new();
        match expr {
            Expr::Variable { name } => {
                if let Some(id) = self.param_id(&name.name) {
                    ids.push(id);
                }
            }
            Expr::Binary { left, right, .. } => {
                ids.extend(self.add_param_ids(left));
                ids.extend(self.add_param_ids(right));
            }
            Expr::Unary { expr, .. } => ids.extend(self.add_param_ids(expr)),
            _ => {}
        }
        ids
    }

    fn simple_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Literal { lit, .. } => Some(match lit {
                Literal::Int(_) => Type::Int,
                Literal::Float(_) => Type::Float,
                Literal::Str(_) => Type::String,
                Literal::Bool(_) => Type::Bool,
            }),
            Expr::Unary { op, expr, .. } => self.simple_type(expr).map(|t| match op {
                UnaryOp::Not => Type::Bool,
                UnaryOp::Negate => t,
            }),
            Expr::Binary {
                left, op, right, ..
            } => {
                let l = self.simple_type(left)?;
                let r = self.simple_type(right)?;
                if l != r {
                    return None;
                }
                match op {
                    BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod => Some(l),
                    _ => Some(Type::Bool),
                }
            }
            _ => None,
        }
    }
}

fn variable_param(expr: &Expr) -> Option<&str> {
    if let Expr::Variable { name } = expr {
        Some(&name.name)
    } else {
        None
    }
}

fn is_rrggbb(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].chars().all(|c| c.is_ascii_hexdigit())
}
