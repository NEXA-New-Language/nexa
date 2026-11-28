use crate::semantics::{AnalyzedProgram, SemanticAnalyzer};
use crate::syntax::ast::*;
use crate::types::Type;
use std::collections::HashMap;

/// Native x86-64 System V backend. Programs are lowered directly to GNU as
/// Intel-syntax assembly and linked with the platform C runtime; there is no
/// interpreter or bytecode VM in the compilation/execution path.
pub struct X86_64CodeGen {
    label_id: usize,
    string_literals: Vec<(String, String)>,
    float_literals: Vec<(String, u64)>,
}

impl Default for X86_64CodeGen {
    fn default() -> Self {
        Self::new()
    }
}

impl X86_64CodeGen {
    pub fn new() -> Self {
        Self {
            label_id: 0,
            string_literals: Vec::new(),
            float_literals: Vec::new(),
        }
    }

    /// Compatibility wrapper for callers that only have an untyped AST.
    pub fn generate(self, program: &Program) -> String {
        let mut analyzer = SemanticAnalyzer::new("<codegen>");
        let analyzed = analyzer
            .analyze_program(program)
            .expect("codegen requires a semantically valid program");
        self.generate_analyzed(&analyzed)
    }

    pub fn generate_analyzed(mut self, analyzed: &AnalyzedProgram) -> String {
        if let Some(app) = analyzed
            .program
            .statements
            .iter()
            .find_map(|stmt| match stmt {
                Stmt::UiApp { app, .. } => Some(app),
                _ => None,
            })
        {
            return self.generate_ui_analyzed(analyzed, app);
        }

        let mut ctx = GenCtx {
            analyzed,
            labels: &mut self.label_id,
            strings: &mut self.string_literals,
            floats: &mut self.float_literals,
            frame_size: 0,
        };

        let max_depth = max_expr_depth_program(&analyzed.program);
        let max_stack_args = max_call_stack_args(&analyzed.program);
        let outgoing = max_stack_args.saturating_sub(6);
        let words = analyzed.slot_types.len() + max_depth + outgoing;
        ctx.frame_size = (words * 8).div_ceil(16) * 16;

        let mut functions_code = String::new();
        let mut main_body = String::new();
        for stmt in &analyzed.program.statements {
            if let Stmt::Action {
                name, params, body, ..
            } = stmt
            {
                ctx.gen_action(name, params, body, &mut functions_code);
            } else {
                ctx.gen_stmt(stmt, 0, &mut main_body);
            }
        }

        let mut main_code = String::new();
        main_code.push_str(".globl main\nmain:\n");
        main_code.push_str("    push rbp\n    mov rbp, rsp\n");
        main_code.push_str(&format!("    sub rsp, {}\n", ctx.frame_size));
        main_code.push_str(&main_body);
        main_code.push_str("    xor eax, eax\n    leave\n    ret\n");

        let mut rodata = String::from(".section .rodata\n");
        rodata.push_str(".LC_fmt_int:\n    .asciz \"%ld\\n\"\n");
        rodata.push_str(".LC_fmt_float:\n    .asciz \"%.17g\\n\"\n");
        rodata.push_str(".LC_bool_true:\n    .asciz \"true\"\n");
        rodata.push_str(".LC_bool_false:\n    .asciz \"false\"\n");
        rodata.push_str(".LC_sign_mask:\n    .quad 0x8000000000000000, 0\n");
        for (label, content) in &self.string_literals {
            rodata.push_str(&format!("{}:\n", label));
            emit_bytes(&mut rodata, content.as_bytes());
            rodata.push_str("    .byte 0\n");
        }
        for (label, bits) in &self.float_literals {
            rodata.push_str(&format!("{}:\n    .quad {}\n", label, bits));
        }

        format!(
            ".intel_syntax noprefix\n{}\n.text\n{}{}{}\n.section .note.GNU-stack,\"\",@progbits\n",
            rodata,
            runtime_asm(),
            functions_code,
            main_code
        )
    }
    /// UI 1.0 backend (spec section 8): state lives in a global array
    /// shared by `main`, the render function and every event handler;
    /// `main` hands the render function to the native UI runtime
    /// (`nexa_ui_init` / `nexa_ui_run`, see src/ui/runtime.c), which owns
    /// the window, the event loop and re-rendering after each event.
    fn generate_ui_analyzed(mut self, analyzed: &AnalyzedProgram, app: &UiApp) -> String {
        let handlers = collect_ui_handlers(app);
        let handler_indices: HashMap<Span, usize> = handlers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.span, i))
            .collect();

        let mut ctx = GenCtx {
            analyzed,
            labels: &mut self.label_id,
            strings: &mut self.string_literals,
            floats: &mut self.float_literals,
            frame_size: 0,
        };

        let max_depth = max_expr_depth_program(&analyzed.program);
        let max_stack_args = max_call_stack_args(&analyzed.program);
        let outgoing = max_stack_args.saturating_sub(6);
        let words = analyzed.slot_types.len() + max_depth + outgoing;
        ctx.frame_size = (words * 8).div_ceil(16) * 16;

        let mut functions_code = String::new();
        for (i, handler) in handlers.iter().enumerate() {
            ctx.gen_ui_handler(handler, i, &mut functions_code);
        }
        let mut render_code = String::new();
        ctx.gen_ui_render(app, &handler_indices, &mut render_code);

        let mut main_body = String::new();
        for state in &app.states {
            let ty = ctx.gen_expr(&state.value, 0, &mut main_body);
            let slot = ctx.binding(state.name.span);
            ctx.store_slot(&mut main_body, &ty, slot);
        }
        let app_name_label = ctx.intern_string(&app.name);
        main_body.push_str("    lea rdi, [rip + __nexa_ui_render]\n");
        main_body.push_str(&format!("    lea rsi, [rip + {}]\n", app_name_label));
        main_body.push_str("    call nexa_ui_init\n");
        main_body.push_str("    call nexa_ui_run\n");

        let mut main_code = String::new();
        main_code.push_str(".globl main\nmain:\n");
        main_code.push_str("    push rbp\n    mov rbp, rsp\n");
        main_code.push_str(&format!("    sub rsp, {}\n", ctx.frame_size));
        main_code.push_str(&main_body);
        main_code.push_str("    xor eax, eax\n    leave\n    ret\n");

        let mut rodata = String::from(".section .rodata\n");
        rodata.push_str(".LC_fmt_int:\n    .asciz \"%ld\\n\"\n");
        rodata.push_str(".LC_fmt_float:\n    .asciz \"%.17g\\n\"\n");
        rodata.push_str(".LC_bool_true:\n    .asciz \"true\"\n");
        rodata.push_str(".LC_bool_false:\n    .asciz \"false\"\n");
        rodata.push_str(".LC_sign_mask:\n    .quad 0x8000000000000000, 0\n");
        for (label, content) in &self.string_literals {
            rodata.push_str(&format!("{}:\n", label));
            emit_bytes(&mut rodata, content.as_bytes());
            rodata.push_str("    .byte 0\n");
        }
        for (label, bits) in &self.float_literals {
            rodata.push_str(&format!("{}:\n    .quad {}\n", label, bits));
        }

        let state_bytes = analyzed.slot_types.len().max(1) * 8;
        let bss = format!(
            ".section .bss\n    .balign 8\n    .globl __nexa_ui_state\n__nexa_ui_state:\n    .zero {}\n",
            state_bytes
        );

        format!(
            ".intel_syntax noprefix\n{}\n.text\n{}{}{}{}\n{}\n.section .note.GNU-stack,\"\",@progbits\n",
            rodata, runtime_asm(), functions_code, render_code, main_code, bss
        )
    }
}

/// One compiled event handler: an `on_click` body (no parameter) or an
/// `on_change(value)` body (one String/Bool parameter).
struct UiHandlerInfo<'a> {
    body: &'a Block,
    param: Option<(Ident, Type)>,
    span: Span,
}

fn collect_ui_handlers<'a>(app: &'a UiApp) -> Vec<UiHandlerInfo<'a>> {
    let mut out = Vec::new();
    if let Some(window) = &app.window {
        for item in &window.items {
            collect_ui_item_handlers(item, &mut out);
        }
    }
    out
}

fn collect_ui_item_handlers<'a>(item: &'a UiItem, out: &mut Vec<UiHandlerInfo<'a>>) {
    match item {
        UiItem::Component(comp) => collect_ui_component_handlers(comp, out),
        UiItem::EventHandler(handler) => out.push(UiHandlerInfo {
            body: &handler.body,
            param: None,
            span: handler.span,
        }),
        UiItem::ChangeHandler(handler) => out.push(UiHandlerInfo {
            body: &handler.body,
            param: Some((handler.param.clone(), Type::String)),
            span: handler.span,
        }),
        UiItem::Property(_) => {}
    }
}

fn collect_ui_component_handlers<'a>(comp: &'a UiComponent, out: &mut Vec<UiHandlerInfo<'a>>) {
    match comp {
        UiComponent::Layout { items, .. } => {
            for item in items {
                collect_ui_item_handlers(item, out);
            }
        }
        UiComponent::Button { handler, .. } => {
            if let Some(h) = handler {
                out.push(UiHandlerInfo {
                    body: &h.body,
                    param: None,
                    span: h.span,
                });
            }
        }
        UiComponent::TextField { handler, .. } => {
            if let Some(h) = handler {
                out.push(UiHandlerInfo {
                    body: &h.body,
                    param: Some((h.param.clone(), Type::String)),
                    span: h.span,
                });
            }
        }
        UiComponent::Checkbox { handler, .. } => {
            if let Some(h) = handler {
                out.push(UiHandlerInfo {
                    body: &h.body,
                    param: Some((h.param.clone(), Type::Bool)),
                    span: h.span,
                });
            }
        }
        _ => {}
    }
}

struct GenCtx<'a> {
    analyzed: &'a AnalyzedProgram,
    labels: &'a mut usize,
    strings: &'a mut Vec<(String, String)>,
    floats: &'a mut Vec<(String, u64)>,
    frame_size: usize,
}

impl<'a> GenCtx<'a> {
    fn label(&mut self, prefix: &str) -> String {
        *self.labels += 1;
        format!(".L_{}_{}", prefix, self.labels)
    }

    fn offset(&self, slot: usize) -> usize {
        (slot + 1) * 8
    }

    fn temp_offset(&self, depth: usize) -> usize {
        (self.analyzed.slot_types.len() + depth + 1) * 8
    }

    fn binding(&self, span: Span) -> usize {
        *self.analyzed.bindings.get(&span).expect("analyzed binding")
    }

    fn expr_type(&self, expr: &Expr) -> Type {
        self.analyzed
            .expr_types
            .get(&expr.span())
            .cloned()
            .expect("analyzed expression type")
    }

    fn action_label(name: &str) -> String {
        format!("__nexa_action_{}", name)
    }

    fn gen_action(&mut self, name: &Ident, params: &[Ident], body: &Block, out: &mut String) {
        out.push_str(&format!("{}:\n", Self::action_label(&name.name)));
        out.push_str("    push rbp\n    mov rbp, rsp\n");
        out.push_str(&format!("    sub rsp, {}\n", self.frame_size));
        let int_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        let float_regs = ["xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5"];
        let sig = &self.analyzed.actions[&name.name];
        let mut int_i = 0;
        let mut float_i = 0;
        let mut stack_i = 0;
        for (param, ty) in params.iter().zip(sig.params.iter()) {
            let slot = self.binding(param.span);
            let off = self.offset(slot);
            match ty {
                Type::Float => {
                    if float_i < 6 {
                        out.push_str(&format!(
                            "    movsd [rbp - {}], {}\n",
                            off, float_regs[float_i]
                        ));
                        float_i += 1;
                    } else {
                        out.push_str(&format!("    mov rax, [rbp + {}]\n", 16 + stack_i * 8));
                        out.push_str(&format!("    mov [rbp - {}], rax\n", off));
                        stack_i += 1;
                    }
                }
                _ => {
                    if int_i < 6 {
                        out.push_str(&format!("    mov [rbp - {}], {}\n", off, int_regs[int_i]));
                        int_i += 1;
                    } else {
                        out.push_str(&format!("    mov rax, [rbp + {}]\n", 16 + stack_i * 8));
                        out.push_str(&format!("    mov [rbp - {}], rax\n", off));
                        stack_i += 1;
                    }
                }
            }
        }
        for stmt in &body.statements {
            self.gen_stmt(stmt, 0, out);
        }
        out.push_str("    xor eax, eax\n    leave\n    ret\n");
    }

    // --------------------------------------------------------------
    // NEXA UI 1.0 code generation (spec section 8).
    // --------------------------------------------------------------

    fn is_state_slot(&self, slot: usize) -> bool {
        self.analyzed.state_slots.contains(&slot)
    }

    /// Store rax/xmm0 into a binding: `state` variables live in the global
    /// `__nexa_ui_state` array, ordinary locals live in the stack frame.
    fn store_slot(&self, out: &mut String, ty: &Type, slot: usize) {
        if self.is_state_slot(slot) {
            if *ty == Type::Float {
                out.push_str(&format!(
                    "    movsd [rip + __nexa_ui_state + {}], xmm0\n",
                    slot * 8
                ));
            } else {
                out.push_str(&format!(
                    "    mov [rip + __nexa_ui_state + {}], rax\n",
                    slot * 8
                ));
            }
        } else {
            store_rax(out, ty, self.offset(slot));
        }
    }

    /// Emit a compile-time constant value (used by constant folding).
    fn emit_const(&mut self, value: &ConstVal, out: &mut String) {
        match value {
            ConstVal::Int(v) => out.push_str(&format!("    mov rax, {}\n", v)),
            ConstVal::Bool(v) => {
                out.push_str(&format!("    mov rax, {}\n", if *v { 1 } else { 0 }))
            }
            ConstVal::Float(v) => {
                let label = self.label("float");
                self.floats.push((label.clone(), v.to_bits()));
                out.push_str(&format!("    movsd xmm0, [rip + {}]\n", label));
            }
            ConstVal::Str(v) => {
                let label = self.label("str");
                self.strings.push((label.clone(), v.clone()));
                out.push_str(&format!("    lea rax, [rip + {}]\n", label));
            }
        }
    }

    /// Memory operand for a binding: a stack slot, or the global state
    /// array for a `state` variable.
    fn slot_location(&self, slot: usize) -> String {
        if self.is_state_slot(slot) {
            format!("[rip + __nexa_ui_state + {}]", slot * 8)
        } else {
            format!("[rbp - {}]", self.offset(slot))
        }
    }

    /// Peephole: `x = x +/- <expr>` on an integer variable updates the
    /// variable in place instead of loading, computing and storing it.
    /// Returns true when the assignment was emitted (or is a no-op).
    fn try_gen_self_assign(&mut self, slot: usize, value: &Expr, out: &mut String) -> bool {
        if self.analyzed.slot_types.get(slot) != Some(&Type::Int) {
            return false;
        }
        let Expr::Binary { left, op, right, .. } = value else {
            return false;
        };
        if !matches!(op, BinaryOp::Add | BinaryOp::Sub) {
            return false;
        }
        let is_add = *op == BinaryOp::Add;
        let is_self = |e: &Expr, slot: usize, this: &GenCtx| -> bool {
            matches!(e, Expr::Variable { name } if this.binding(name.span) == slot)
        };
        // `x = x +/- e` or (for Add) `x = e + x`.
        let other = if is_self(left, slot, self) {
            right
        } else if is_add && is_self(right, slot, self) {
            left
        } else {
            return false;
        };
        let loc = self.slot_location(slot);
        match const_eval(other) {
            Some(ConstVal::Int(c)) => {
                // `x = x + 0` is a no-op; anything else is one instruction.
                if c != 0 {
                    let instr = if is_add { "add" } else { "sub" };
                    out.push_str(&format!(
                        "    {} qword ptr {}, {}\n    jo __nexa_int_overflow\n",
                        instr, loc, c
                    ));
                }
                true
            }
            _ => {
                self.gen_expr(other, 0, out);
                let instr = if is_add { "add" } else { "sub" };
                out.push_str(&format!(
                    "    {} qword ptr {}, rax\n    jo __nexa_int_overflow\n",
                    instr, loc
                ));
                true
            }
        }
    }

    /// Emit `cmp` for an integer/boolean comparison in its cheapest form
    /// (memory vs. immediate when one side is a variable and the other a
    /// constant) and return the effective operator after any operand flip.
    fn gen_int_cmp(
        &mut self,
        left: &Expr,
        op: BinaryOp,
        right: &Expr,
        depth: usize,
        out: &mut String,
    ) -> BinaryOp {
        if let Expr::Variable { name } = left {
            if let Some(ConstVal::Int(c)) = const_eval(right) {
                let loc = self.slot_location(self.binding(name.span));
                out.push_str(&format!("    cmp qword ptr {}, {}\n", loc, c));
                return op;
            }
        }
        if let Expr::Variable { name } = right {
            if let Some(ConstVal::Int(c)) = const_eval(left) {
                let loc = self.slot_location(self.binding(name.span));
                out.push_str(&format!("    cmp qword ptr {}, {}\n", loc, c));
                return flip_cmp_op(op);
            }
        }
        if let Expr::Variable { name } = left {
            let loc = self.slot_location(self.binding(name.span));
            self.gen_expr(right, depth, out);
            out.push_str("    mov rbx, rax\n");
            out.push_str(&format!("    cmp qword ptr {}, rbx\n", loc));
            return op;
        }
        if let Expr::Variable { name } = right {
            let loc = self.slot_location(self.binding(name.span));
            self.gen_expr(left, depth, out);
            out.push_str(&format!("    cmp rax, qword ptr {}\n", loc));
            return op;
        }
        let left_ty = self.gen_expr(left, depth, out);
        let temp = self.temp_offset(depth);
        store_temp(out, &left_ty, temp);
        self.gen_expr(right, depth + 1, out);
        out.push_str("    mov rbx, rax\n");
        out.push_str(&format!("    mov rax, [rbp - {}]\n", temp));
        out.push_str("    cmp rax, rbx\n");
        op
    }

    /// Emit a conditional jump to `label` when `expr` evaluates to
    /// `jump_when`. Integer comparisons jump directly off the flags
    /// instead of materializing a 0/1 in rax first.
    fn gen_branch(
        &mut self,
        expr: &Expr,
        label: &str,
        jump_when: bool,
        depth: usize,
        out: &mut String,
    ) {
        if let Some(ConstVal::Bool(b)) = const_eval(expr) {
            if b == jump_when {
                out.push_str(&format!("    jmp {}\n", label));
            }
            return;
        }
        match expr {
            Expr::Unary {
                op: UnaryOp::Not,
                expr: inner,
                ..
            } => {
                self.gen_branch(inner, label, !jump_when, depth, out);
            }
            Expr::Binary { left, op, right, .. }
                if matches!(
                    op,
                    BinaryOp::Equal
                        | BinaryOp::NotEqual
                        | BinaryOp::Less
                        | BinaryOp::LessEq
                        | BinaryOp::Greater
                        | BinaryOp::GreaterEq
                ) && (self.expr_type(left) == Type::Int
                    || self.expr_type(left) == Type::Bool) =>
            {
                let eff = self.gen_int_cmp(left, *op, right, depth, out);
                let jmp = match (eff, jump_when) {
                    (BinaryOp::Equal, true) => "je",
                    (BinaryOp::Equal, false) => "jne",
                    (BinaryOp::NotEqual, true) => "jne",
                    (BinaryOp::NotEqual, false) => "je",
                    (BinaryOp::Less, true) => "jl",
                    (BinaryOp::Less, false) => "jge",
                    (BinaryOp::LessEq, true) => "jle",
                    (BinaryOp::LessEq, false) => "jg",
                    (BinaryOp::Greater, true) => "jg",
                    (BinaryOp::Greater, false) => "jle",
                    (BinaryOp::GreaterEq, true) => "jge",
                    (BinaryOp::GreaterEq, false) => "jl",
                    _ => unreachable!(),
                };
                out.push_str(&format!("    {} {}\n", jmp, label));
            }
            Expr::Binary {
                left,
                op: BinaryOp::And,
                right,
                ..
            } => {
                if jump_when {
                    let next = self.label("and_next");
                    self.gen_branch(left, &next, false, depth, out);
                    self.gen_branch(right, label, true, depth, out);
                    out.push_str(&format!("{}:\n", next));
                } else {
                    self.gen_branch(left, label, false, depth, out);
                    self.gen_branch(right, label, false, depth, out);
                }
            }
            Expr::Binary {
                left,
                op: BinaryOp::Or,
                right,
                ..
            } => {
                if jump_when {
                    self.gen_branch(left, label, true, depth, out);
                    self.gen_branch(right, label, true, depth, out);
                } else {
                    let next = self.label("or_next");
                    self.gen_branch(left, &next, true, depth, out);
                    self.gen_branch(right, label, false, depth, out);
                    out.push_str(&format!("{}:\n", next));
                }
            }
            _ => {
                self.gen_expr(expr, depth, out);
                out.push_str("    test rax, rax\n");
                out.push_str(&format!(
                    "    {} {}\n",
                    if jump_when { "jnz" } else { "jz" },
                    label
                ));
            }
        }
    }

    fn intern_string(&mut self, value: &str) -> String {
        let label = self.label("str");
        self.strings.push((label.clone(), value.to_string()));
        label
    }

    fn print_static(&mut self, value: &str, out: &mut String) {
        let label = self.intern_string(value);
        out.push_str(&format!(
            "    lea rax, [rip + {}]\n    mov rdi, rax\n    call nexa_ui_print_str\n",
            label
        ));
    }

    /// Evaluate an expression and print its value with the runtime helper
    /// matching its type (a no-op in the windowed backend, which prints
    /// nothing to stdout).
    fn print_expr_value(&mut self, expr: &Expr, out: &mut String) {
        let ty = self.gen_expr(expr, 0, out);
        match ty {
            Type::Float => out.push_str("    call nexa_ui_print_float\n"),
            Type::String => {
                out.push_str("    mov rdi, rax\n    call nexa_ui_print_str\n")
            }
            Type::Bool => {
                out.push_str("    mov rdi, rax\n    call nexa_ui_print_bool\n")
            }
            _ => out.push_str("    mov rdi, rax\n    call nexa_ui_print_int\n"),
        }
    }

    fn print_indent(&mut self, depth: usize, out: &mut String) {
        out.push_str(&format!(
            "    mov edi, {}\n    call nexa_ui_indent\n",
            depth
        ));
    }

    /// Evaluate an expression into the stack temp at `depth`.
    fn eval_to_temp(&mut self, expr: &Expr, depth: usize, out: &mut String) -> Type {
        let ty = self.gen_expr(expr, depth, out);
        let temp = self.temp_offset(depth);
        store_temp(out, &ty, temp);
        ty
    }

    /// Emit a handler function pointer into `reg`, or zero when there is
    /// no handler.
    fn emit_handler_ptr(
        &self,
        handler_indices: &HashMap<Span, usize>,
        span: Option<Span>,
        reg: &str,
        out: &mut String,
    ) {
        match span.and_then(|span| handler_indices.get(&span)) {
            Some(index) => out.push_str(&format!(
                "    lea {}, [rip + __nexa_ui_handler_{}]\n",
                reg, index
            )),
            None => out.push_str(&format!("    xor {}, {}\n", reg, reg)),
        }
    }

    fn gen_ui_handler(&mut self, info: &UiHandlerInfo, index: usize, out: &mut String) {
        out.push_str(&format!("__nexa_ui_handler_{}:\n", index));
        out.push_str("    push rbp\n    mov rbp, rsp\n");
        out.push_str(&format!("    sub rsp, {}\n", self.frame_size));
        if let Some((param, _)) = &info.param {
            // The runtime passes the event value in rdi (string pointer or
            // 0/1 for a checkbox).
            let slot = self.binding(param.span);
            let off = self.offset(slot);
            out.push_str(&format!("    mov [rbp - {}], rdi\n", off));
        }
        for stmt in &info.body.statements {
            self.gen_stmt(stmt, 0, out);
        }
        out.push_str("    xor eax, eax\n    leave\n    ret\n");
    }

    /// The render function: prints the snapshot (headless backend) and
    /// creates/updates the native widgets (windowed backend) in one pass.
    fn gen_ui_render(
        &mut self,
        app: &UiApp,
        handler_indices: &HashMap<Span, usize>,
        out: &mut String,
    ) {
        out.push_str("__nexa_ui_render:\n");
        out.push_str("    push rbp\n    mov rbp, rsp\n");
        out.push_str(&format!("    sub rsp, {}\n", self.frame_size));
        out.push_str("    call nexa_ui_begin_render\n");
        if let Some(window) = &app.window {
            self.gen_ui_window(app, window, handler_indices, out);
        }
        out.push_str("    xor eax, eax\n    leave\n    ret\n");
    }

    fn gen_ui_window(
        &mut self,
        app: &UiApp,
        window: &UiWindow,
        handler_indices: &HashMap<Span, usize>,
        out: &mut String,
    ) {
        let title_prop = window.items.iter().find_map(|item| match item {
            UiItem::Property(prop) if prop.name.name == "title" => Some(prop),
            _ => None,
        });
        let size_prop = window.items.iter().find_map(|item| match item {
            UiItem::Property(prop) if prop.name.name == "size" => Some(prop),
            _ => None,
        });

        self.print_indent(0, out);
        self.print_static("Window ", out);
        match title_prop {
            Some(prop) => self.print_expr_value(&prop.args[0], out),
            None => self.print_static(&app.name, out),
        }
        self.print_static(" size=", out);
        match size_prop {
            Some(prop) => {
                self.print_expr_value(&prop.args[0], out);
                self.print_static("x", out);
                self.print_expr_value(&prop.args[1], out);
            }
            None => {
                out.push_str("    mov rdi, 800\n    call nexa_ui_print_int\n");
                self.print_static("x", out);
                out.push_str("    mov rdi, 600\n    call nexa_ui_print_int\n");
            }
        }
        for item in &window.items {
            if let UiItem::Property(prop) = item {
                if prop.name.name == "title" || prop.name.name == "size" {
                    continue;
                }
                self.print_static(" ", out);
                self.print_static(&prop.name.name, out);
                self.print_static("=", out);
                self.print_expr_value(&prop.args[0], out);
            }
        }
        out.push_str("    call nexa_ui_print_nl\n");

        match title_prop {
            Some(prop) => {
                self.eval_to_temp(&prop.args[0], 0, out);
            }
            None => {
                let label = self.intern_string(&app.name);
                let temp = self.temp_offset(0);
                out.push_str(&format!("    lea rax, [rip + {}]\n", label));
                out.push_str(&format!("    mov [rbp - {}], rax\n", temp));
            }
        }
        match size_prop {
            Some(prop) => {
                self.eval_to_temp(&prop.args[0], 1, out);
                self.eval_to_temp(&prop.args[1], 2, out);
            }
            None => {
                for (depth, value) in [(1usize, 800i64), (2, 600)] {
                    let temp = self.temp_offset(depth);
                    out.push_str(&format!(
                        "    mov rax, {}\n    mov [rbp - {}], rax\n",
                        value, temp
                    ));
                }
            }
        }
        out.push_str(&format!(
            "    mov rdi, [rbp - {}]\n",
            self.temp_offset(0)
        ));
        out.push_str(&format!(
            "    mov rsi, [rbp - {}]\n",
            self.temp_offset(1)
        ));
        out.push_str(&format!(
            "    mov rdx, [rbp - {}]\n",
            self.temp_offset(2)
        ));
        out.push_str("    call nexa_ui_window\n");

        for item in &window.items {
            if let UiItem::Property(prop) = item {
                if prop.name.name == "title" || prop.name.name == "size" {
                    continue;
                }
                self.gen_ui_prop_call(prop, out);
            }
        }
        for item in &window.items {
            if let UiItem::Component(comp) = item {
                self.gen_ui_component(comp, 1, handler_indices, out);
            }
        }
    }

    fn gen_ui_prop_call(&mut self, prop: &UiProperty, out: &mut String) {
        if prop.args.is_empty() {
            return;
        }
        let name_label = self.intern_string(&prop.name.name);
        let ty = self.eval_to_temp(&prop.args[0], 0, out);
        out.push_str(&format!("    lea rdi, [rip + {}]\n", name_label));
        let temp = self.temp_offset(0);
        match ty {
            Type::Float => {
                out.push_str(&format!("    movsd xmm0, [rbp - {}]\n", temp));
                out.push_str("    call nexa_ui_prop_float\n");
            }
            Type::String => {
                out.push_str(&format!("    mov rsi, [rbp - {}]\n", temp));
                out.push_str("    call nexa_ui_prop_str\n");
            }
            Type::Bool => {
                out.push_str(&format!("    mov rsi, [rbp - {}]\n", temp));
                out.push_str("    call nexa_ui_prop_bool\n");
            }
            _ => {
                out.push_str(&format!("    mov rsi, [rbp - {}]\n", temp));
                out.push_str("    call nexa_ui_prop_int\n");
            }
        }
    }

    fn gen_ui_component(
        &mut self,
        comp: &UiComponent,
        depth: usize,
        handler_indices: &HashMap<Span, usize>,
        out: &mut String,
    ) {
        match comp {
            UiComponent::Layout { kind, items, .. } => {
                let (kind_name, kind_id) = match kind {
                    LayoutKind::Column => ("Column", 0),
                    LayoutKind::Row => ("Row", 1),
                    LayoutKind::Container => ("Container", 2),
                };
                self.print_indent(depth, out);
                self.print_static(kind_name, out);
                for item in items {
                    if let UiItem::Property(prop) = item {
                        self.print_static(" ", out);
                        self.print_static(&prop.name.name, out);
                        self.print_static("=", out);
                        self.print_expr_value(&prop.args[0], out);
                    }
                }
                out.push_str("    call nexa_ui_print_nl\n");
                out.push_str(&format!(
                    "    mov edi, {}\n    call nexa_ui_begin_layout\n",
                    kind_id
                ));
                for item in items {
                    if let UiItem::Property(prop) = item {
                        self.gen_ui_prop_call(prop, out);
                    }
                }
                for item in items {
                    if let UiItem::Component(child) = item {
                        self.gen_ui_component(child, depth + 1, handler_indices, out);
                    }
                }
                out.push_str("    call nexa_ui_end_layout\n");
            }
            UiComponent::Text { value, .. } => {
                self.print_indent(depth, out);
                self.print_static("Text ", out);
                self.print_expr_value(value, out);
                out.push_str("    call nexa_ui_print_nl\n");
                match self.expr_type(value) {
                    Type::Float => {
                        self.gen_expr(value, 0, out);
                        out.push_str("    call nexa_ui_text_float\n");
                    }
                    Type::String => {
                        self.gen_expr(value, 0, out);
                        out.push_str("    mov rdi, rax\n    call nexa_ui_text_str\n");
                    }
                    Type::Bool => {
                        self.gen_expr(value, 0, out);
                        out.push_str("    mov rdi, rax\n    call nexa_ui_text_bool\n");
                    }
                    _ => {
                        self.gen_expr(value, 0, out);
                        out.push_str("    mov rdi, rax\n    call nexa_ui_text_int\n");
                    }
                }
            }
            UiComponent::Button { label, handler, .. } => {
                self.print_indent(depth, out);
                self.print_static("Button ", out);
                self.print_expr_value(label, out);
                out.push_str("    call nexa_ui_print_nl\n");
                self.gen_expr(label, 0, out);
                out.push_str("    mov rdi, rax\n");
                let span = handler.as_ref().map(|h| h.span);
                self.emit_handler_ptr(handler_indices, span, "rsi", out);
                out.push_str("    call nexa_ui_button\n");
            }
            UiComponent::TextField { bound, handler, .. } => {
                self.print_indent(depth, out);
                self.print_static("TextField ", out);
                self.print_expr_value(bound, out);
                out.push_str("    call nexa_ui_print_nl\n");
                let key = match bound {
                    Expr::Variable { name } => name.name.clone(),
                    _ => String::new(),
                };
                let key_label = self.intern_string(&key);
                self.eval_to_temp(bound, 0, out);
                out.push_str(&format!("    lea rdi, [rip + {}]\n", key_label));
                out.push_str(&format!(
                    "    mov rsi, [rbp - {}]\n",
                    self.temp_offset(0)
                ));
                let span = handler.as_ref().map(|h| h.span);
                self.emit_handler_ptr(handler_indices, span, "rdx", out);
                out.push_str("    call nexa_ui_textfield\n");
            }
            UiComponent::Checkbox { bound, handler, .. } => {
                self.print_indent(depth, out);
                self.print_static("Checkbox ", out);
                self.print_expr_value(bound, out);
                out.push_str("    call nexa_ui_print_nl\n");
                let (key, slot) = match bound {
                    Expr::Variable { name } => {
                        (name.name.clone(), self.binding(name.span))
                    }
                    _ => (String::new(), 0),
                };
                let key_label = self.intern_string(&key);
                self.eval_to_temp(bound, 0, out);
                out.push_str(&format!("    lea rdi, [rip + {}]\n", key_label));
                out.push_str(&format!(
                    "    mov rsi, [rbp - {}]\n",
                    self.temp_offset(0)
                ));
                let span = handler.as_ref().map(|h| h.span);
                self.emit_handler_ptr(handler_indices, span, "rdx", out);
                out.push_str(&format!("    mov rcx, {}\n", slot));
                out.push_str("    call nexa_ui_checkbox\n");
            }
            UiComponent::Spacer { .. } => {
                self.print_indent(depth, out);
                self.print_static("Spacer", out);
                out.push_str("    call nexa_ui_print_nl\n");
                out.push_str("    call nexa_ui_spacer\n");
            }
            UiComponent::Divider { .. } => {
                self.print_indent(depth, out);
                self.print_static("Divider", out);
                out.push_str("    call nexa_ui_print_nl\n");
                out.push_str("    call nexa_ui_divider\n");
            }
            UiComponent::Image { source, .. } => {
                self.print_indent(depth, out);
                self.print_static("Image ", out);
                self.print_expr_value(source, out);
                out.push_str("    call nexa_ui_print_nl\n");
                self.gen_expr(source, 0, out);
                out.push_str("    mov rdi, rax\n    call nexa_ui_image\n");
            }
        }
    }

    fn gen_stmt(&mut self, stmt: &Stmt, depth: usize, out: &mut String) {
        match stmt {
            Stmt::Let { name, value, .. }
            | Stmt::Var { name, value, .. }
            | Stmt::Const { name, value, .. } => {
                let ty = self.gen_expr(value, depth, out);
                let slot = self.binding(name.span);
                self.store_slot(out, &ty, slot);
            }
            Stmt::Assign { target, value, .. } => {
                let slot = self.binding(target.span);
                if !self.try_gen_self_assign(slot, value, out) {
                    let ty = self.gen_expr(value, depth, out);
                    self.store_slot(out, &ty, slot);
                }
            }
            Stmt::UiApp { .. } => {
                // A UI application is compiled by generate_ui_analyzed, not
                // as a statement inside a function body.
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                // A constant condition keeps only the taken branch.
                if let Some(ConstVal::Bool(b)) = const_eval(condition) {
                    let branch = if b {
                        Some(then_branch)
                    } else {
                        else_branch.as_ref()
                    };
                    if let Some(block) = branch {
                        for s in &block.statements {
                            self.gen_stmt(s, depth, out);
                        }
                    }
                    return;
                }
                let else_lbl = self.label("else");
                let end_lbl = self.label("endif");
                self.gen_branch(
                    condition,
                    if else_branch.is_some() {
                        &else_lbl
                    } else {
                        &end_lbl
                    },
                    false,
                    depth,
                    out,
                );
                for s in &then_branch.statements {
                    self.gen_stmt(s, depth, out);
                }
                if let Some(eb) = else_branch {
                    out.push_str(&format!("    jmp {}\n{}:\n", end_lbl, else_lbl));
                    for s in &eb.statements {
                        self.gen_stmt(s, depth, out);
                    }
                }
                out.push_str(&format!("{}:\n", end_lbl));
            }
            Stmt::While {
                condition, body, ..
            } => {
                // `while false` never runs; anything else keeps looping.
                if const_eval(condition) == Some(ConstVal::Bool(false)) {
                    return;
                }
                let start = self.label("while_start");
                let end = self.label("while_end");
                out.push_str(&format!("{}:\n", start));
                self.gen_branch(condition, &end, false, depth, out);
                for s in &body.statements {
                    self.gen_stmt(s, depth, out);
                }
                out.push_str(&format!("    jmp {}\n{}:\n", start, end));
            }
            Stmt::Repeat { count, body, span } => {
                let start = self.label("repeat_start");
                let end = self.label("repeat_end");
                let slot = self.analyzed.repeat_counters[span];
                let off = self.offset(slot);
                self.gen_expr(count, depth, out);
                out.push_str(&format!("    mov [rbp - {}], rax\n", off));
                out.push_str(&format!("{}:\n", start));
                out.push_str(&format!("    mov rax, [rbp - {}]\n", off));
                out.push_str("    test rax, rax\n");
                out.push_str(&format!("    jle {}\n", end));
                for s in &body.statements {
                    self.gen_stmt(s, depth, out);
                }
                out.push_str(&format!("    sub qword ptr [rbp - {}], 1\n", off));
                out.push_str(&format!("    jmp {}\n{}:\n", start, end));
            }
            Stmt::Expr(expr) => {
                self.gen_expr(expr, depth, out);
            }
            Stmt::Action { .. } => {}
        }
    }

    fn gen_expr(&mut self, expr: &Expr, depth: usize, out: &mut String) -> Type {
        let result_ty = self.expr_type(expr);
        // Constant folding: a wholly constant expression is evaluated at
        // compile time, so no code is emitted for it at all.
        if let Some(folded) = const_eval(expr) {
            self.emit_const(&folded, out);
            return result_ty;
        }
        match expr {
            Expr::Literal { lit, .. } => match lit {
                Literal::Int(v) => out.push_str(&format!("    mov rax, {}\n", v)),
                Literal::Bool(v) => {
                    out.push_str(&format!("    mov rax, {}\n", if *v { 1 } else { 0 }))
                }
                Literal::Str(s) => {
                    let label = self.label("str");
                    self.strings.push((label.clone(), s.clone()));
                    out.push_str(&format!("    lea rax, [rip + {}]\n", label));
                }
                Literal::Float(v) => {
                    let label = self.label("float");
                    self.floats.push((label.clone(), v.to_bits()));
                    out.push_str(&format!("    movsd xmm0, [rip + {}]\n", label));
                }
            },
            Expr::Variable { name } => {
                let slot = self.binding(name.span);
                if self.is_state_slot(slot) {
                    if result_ty == Type::Float {
                        out.push_str(&format!(
                            "    movsd xmm0, [rip + __nexa_ui_state + {}]\n",
                            slot * 8
                        ));
                    } else {
                        out.push_str(&format!(
                            "    mov rax, [rip + __nexa_ui_state + {}]\n",
                            slot * 8
                        ));
                    }
                } else {
                    let off = self.offset(slot);
                    load_rax(out, &result_ty, off);
                }
            }
            Expr::Unary { op, expr, .. } => {
                let ty = self.gen_expr(expr, depth, out);
                match op {
                    UnaryOp::Negate if ty == Type::Float => {
                        out.push_str(
                            "    movsd xmm1, [rip + .LC_sign_mask]\n    xorpd xmm0, xmm1\n",
                        );
                    }
                    UnaryOp::Negate => {
                        out.push_str("    neg rax\n    jo __nexa_int_overflow\n")
                    }
                    UnaryOp::Not => {
                        out.push_str("    test rax, rax\n    sete al\n    movzx rax, al\n")
                    }
                }
            }
            Expr::Binary {
                left, op, right, ..
            } => {
                if matches!(op, BinaryOp::And | BinaryOp::Or) {
                    let short = self.label("logic_short");
                    let end = self.label("logic_end");
                    self.gen_expr(left, depth, out);
                    out.push_str("    test rax, rax\n");
                    out.push_str(&format!(
                        "    {} {}\n",
                        if *op == BinaryOp::And { "jz" } else { "jnz" },
                        short
                    ));
                    self.gen_expr(right, depth, out);
                    out.push_str(&format!(
                        "    jmp {}\n{}:\n    mov rax, {}\n{}:\n",
                        end,
                        short,
                        if *op == BinaryOp::And { 0 } else { 1 },
                        end
                    ));
                    return Type::Bool;
                }

                // Integer/boolean comparisons use the peephole path:
                // no operand is materialized when a cheaper `cmp` exists.
                let left_ty_static = self.expr_type(left);
                if (left_ty_static == Type::Int || left_ty_static == Type::Bool)
                    && matches!(
                        op,
                        BinaryOp::Equal
                            | BinaryOp::NotEqual
                            | BinaryOp::Less
                            | BinaryOp::LessEq
                            | BinaryOp::Greater
                            | BinaryOp::GreaterEq
                    )
                {
                    let eff = self.gen_int_cmp(left, *op, right, depth, out);
                    let set = match eff {
                        BinaryOp::Equal => "sete",
                        BinaryOp::NotEqual => "setne",
                        BinaryOp::Less => "setl",
                        BinaryOp::LessEq => "setle",
                        BinaryOp::Greater => "setg",
                        BinaryOp::GreaterEq => "setge",
                        _ => unreachable!(),
                    };
                    out.push_str(&format!("    {} al\n    movzx rax, al\n", set));
                    return result_ty;
                }

                let left_ty = self.gen_expr(left, depth, out);
                let temp = self.temp_offset(depth);
                store_temp(out, &left_ty, temp);
                self.gen_expr(right, depth + 1, out);
                if left_ty == Type::Float {
                    out.push_str("    movsd xmm1, xmm0\n");
                    out.push_str(&format!("    movsd xmm0, [rbp - {}]\n", temp));
                } else {
                    out.push_str("    mov rbx, rax\n");
                    out.push_str(&format!("    mov rax, [rbp - {}]\n", temp));
                }

                if left_ty == Type::String {
                    match op {
                        BinaryOp::Add => {
                            out.push_str(
                                "    mov rdi, rax\n    mov rsi, rbx\n    call __nexa_concat\n",
                            );
                        }
                        BinaryOp::Equal | BinaryOp::NotEqual => {
                            out.push_str(
                                "    mov rdi, rax\n    mov rsi, rbx\n    call __nexa_str_eq\n",
                            );
                            if *op == BinaryOp::NotEqual {
                                out.push_str("    xor eax, 1\n");
                            }
                        }
                        _ => unreachable!("string ordering rejected by semantics"),
                    }
                    return result_ty;
                }

                if left_ty == Type::Float {
                    match op {
                        BinaryOp::Add => out.push_str("    addsd xmm0, xmm1\n"),
                        BinaryOp::Sub => out.push_str("    subsd xmm0, xmm1\n"),
                        BinaryOp::Mul => out.push_str("    mulsd xmm0, xmm1\n"),
                        BinaryOp::Div => out.push_str("    divsd xmm0, xmm1\n"),
                        BinaryOp::Mod => {
                            // fmod is provided by libm; keep the call ABI aligned.
                            out.push_str("    call fmod\n");
                        }
                        _ => {
                            let false_lbl = self.label("cmp_false");
                            let end = self.label("cmp_end");
                            out.push_str("    ucomisd xmm0, xmm1\n");
                            out.push_str(&format!("    jp {}\n", false_lbl));
                            let set = match op {
                                BinaryOp::Equal => "sete",
                                BinaryOp::NotEqual => "setne",
                                BinaryOp::Less => "setb",
                                BinaryOp::LessEq => "setbe",
                                BinaryOp::Greater => "seta",
                                BinaryOp::GreaterEq => "setae",
                                _ => unreachable!(),
                            };
                            out.push_str(&format!("    {} al\n    movzx rax, al\n    jmp {}\n{}:\n    xor eax, eax\n{}:\n", set, end, false_lbl, end));
                        }
                    }
                    return result_ty;
                }

                match op {
                    BinaryOp::Add => out.push_str(
                        "    add rax, rbx\n    jo __nexa_int_overflow\n",
                    ),
                    BinaryOp::Sub => out.push_str(
                        "    sub rax, rbx\n    jo __nexa_int_overflow\n",
                    ),
                    BinaryOp::Mul => out.push_str(
                        "    imul rax, rbx\n    jo __nexa_int_overflow\n",
                    ),
                    BinaryOp::Div => {
                        out.push_str("    mov rdi, rax\n    mov rsi, rbx\n    call __nexa_div\n")
                    }
                    BinaryOp::Mod => {
                        out.push_str("    mov rdi, rax\n    mov rsi, rbx\n    call __nexa_mod\n")
                    }
                    _ => unreachable!("integer comparisons use the peephole path"),
                }
            }
            Expr::Call { callee, args, .. } => {
                if callee.name == "assert" {
                    let ok = self.label("assert_ok");
                    self.gen_expr(&args[0], depth, out);
                    out.push_str("    test rax, rax\n");
                    out.push_str(&format!("    jnz {}\n", ok));
                    out.push_str(
                        "    sub rsp, 8\n    lea rdi, [rip + .Lerr_assert]\n    mov rsi, 17\n    call __nexa_runtime_error\n",
                    );
                    out.push_str(&format!("{}:\n", ok));
                    out.push_str("    xor eax, eax\n");
                    return Type::Void;
                }

                if callee.name == "print" {
                    let ty = self.gen_expr(&args[0], depth, out);
                    match ty {
                        Type::Float => out.push_str("    call __nexa_print_float\n"),
                        Type::String => {
                            out.push_str("    mov rdi, rax\n    call __nexa_print_string\n")
                        }
                        Type::Bool => {
                            out.push_str("    mov rdi, rax\n    call __nexa_print_bool\n")
                        }
                        _ => out.push_str("    mov rdi, rax\n    call __nexa_print_int\n"),
                    }
                    out.push_str("    xor eax, eax\n");
                    return Type::Void;
                }

                let sig = &self.analyzed.actions[&callee.name];
                for (i, arg) in args.iter().enumerate() {
                    let ty = self.gen_expr(arg, depth + i, out);
                    let temp = self.temp_offset(depth + i);
                    store_temp(out, &ty, temp);
                }
                let int_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
                let float_regs = ["xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5"];
                let mut int_i = 0;
                let mut float_i = 0;
                let mut stack_i = 0;
                for (i, ty) in sig.params.iter().enumerate() {
                    let temp = self.temp_offset(depth + i);
                    match ty {
                        Type::Float => {
                            if float_i < 6 {
                                out.push_str(&format!(
                                    "    movsd {}, [rbp - {}]\n",
                                    float_regs[float_i], temp
                                ));
                                float_i += 1;
                            } else {
                                out.push_str(&format!("    mov rax, [rbp - {}]\n", temp));
                                out.push_str(&format!("    mov [rsp + {}], rax\n", stack_i * 8));
                                stack_i += 1;
                            }
                        }
                        _ => {
                            if int_i < 6 {
                                out.push_str(&format!(
                                    "    mov {}, [rbp - {}]\n",
                                    int_regs[int_i], temp
                                ));
                                int_i += 1;
                            } else {
                                out.push_str(&format!("    mov rax, [rbp - {}]\n", temp));
                                out.push_str(&format!("    mov [rsp + {}], rax\n", stack_i * 8));
                                stack_i += 1;
                            }
                        }
                    }
                }
                out.push_str(&format!(
                    "    call {}\n    xor eax, eax\n",
                    Self::action_label(&callee.name)
                ));
                return Type::Void;
            }
        }
        result_ty
    }
}

use crate::span::Span;

fn store_rax(out: &mut String, ty: &Type, off: usize) {
    if *ty == Type::Float {
        out.push_str(&format!("    movsd [rbp - {}], xmm0\n", off));
    } else {
        out.push_str(&format!("    mov [rbp - {}], rax\n", off));
    }
}

fn load_rax(out: &mut String, ty: &Type, off: usize) {
    if *ty == Type::Float {
        out.push_str(&format!("    movsd xmm0, [rbp - {}]\n", off));
    } else {
        out.push_str(&format!("    mov rax, [rbp - {}]\n", off));
    }
}

fn store_temp(out: &mut String, ty: &Type, off: usize) {
    if *ty == Type::Float {
        out.push_str(&format!("    movsd [rbp - {}], xmm0\n", off));
    } else {
        out.push_str(&format!("    mov [rbp - {}], rax\n", off));
    }
}

fn emit_bytes(out: &mut String, bytes: &[u8]) {
    for chunk in bytes.chunks(16) {
        out.push_str("    .byte ");
        for (i, b) in chunk.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&b.to_string());
        }
        out.push('\n');
    }
}

fn max_expr_depth_program(program: &Program) -> usize {
    let mut max = 0;
    for stmt in &program.statements {
        max = max.max(max_stmt_depth(stmt));
    }
    max
}

fn max_stmt_depth(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Let { value, .. }
        | Stmt::Var { value, .. }
        | Stmt::Const { value, .. }
        | Stmt::Assign { value, .. }
        | Stmt::Expr(value) => max_expr_depth(value),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let mut m = max_expr_depth(condition);
            for s in &then_branch.statements {
                m = m.max(max_stmt_depth(s));
            }
            if let Some(b) = else_branch {
                for s in &b.statements {
                    m = m.max(max_stmt_depth(s));
                }
            }
            m
        }
        Stmt::While {
            condition, body, ..
        } => {
            let mut m = max_expr_depth(condition);
            for s in &body.statements {
                m = m.max(max_stmt_depth(s));
            }
            m
        }
        Stmt::Repeat { count, body, .. } => {
            let mut m = max_expr_depth(count);
            for s in &body.statements {
                m = m.max(max_stmt_depth(s));
            }
            m
        }
        Stmt::Action { body, .. } => {
            let mut m = 0;
            for s in &body.statements {
                m = m.max(max_stmt_depth(s));
            }
            m
        }
        Stmt::UiApp { app, .. } => max_ui_app_depth(app),
    }
}

fn max_ui_app_depth(app: &UiApp) -> usize {
    let mut m = 0;
    for state in &app.states {
        m = m.max(max_expr_depth(&state.value));
    }
    if let Some(window) = &app.window {
        for item in &window.items {
            m = m.max(max_ui_item_depth(item));
        }
    }
    m
}

fn max_ui_item_depth(item: &UiItem) -> usize {
    match item {
        UiItem::Property(prop) => prop
            .args
            .iter()
            .map(max_expr_depth)
            .max()
            .unwrap_or(0),
        UiItem::Component(comp) => max_ui_component_depth(comp),
        UiItem::EventHandler(handler) => handler
            .body
            .statements
            .iter()
            .map(max_stmt_depth)
            .max()
            .unwrap_or(0),
        UiItem::ChangeHandler(handler) => handler
            .body
            .statements
            .iter()
            .map(max_stmt_depth)
            .max()
            .unwrap_or(0),
    }
}

fn max_ui_component_depth(comp: &UiComponent) -> usize {
    match comp {
        UiComponent::Layout { items, .. } => items
            .iter()
            .map(max_ui_item_depth)
            .max()
            .unwrap_or(0),
        UiComponent::Text { value, .. } | UiComponent::Image { source: value, .. } => {
            max_expr_depth(value)
        }
        UiComponent::Button {
            label, handler, ..
        } => {
            let mut m = max_expr_depth(label);
            if let Some(h) = handler {
                m = m.max(
                    h.body
                        .statements
                        .iter()
                        .map(max_stmt_depth)
                        .max()
                        .unwrap_or(0),
                );
            }
            m
        }
        UiComponent::TextField {
            bound, handler, ..
        }
        | UiComponent::Checkbox {
            bound, handler, ..
        } => {
            let mut m = max_expr_depth(bound);
            if let Some(h) = handler {
                m = m.max(
                    h.body
                        .statements
                        .iter()
                        .map(max_stmt_depth)
                        .max()
                        .unwrap_or(0),
                );
            }
            m
        }
        UiComponent::Spacer { .. } | UiComponent::Divider { .. } => 0,
    }
}

fn max_expr_depth(expr: &Expr) -> usize {
    match expr {
        Expr::Literal { .. } | Expr::Variable { .. } => 0,
        Expr::Unary { expr, .. } => max_expr_depth(expr),
        Expr::Binary { left, right, .. } => 1 + max_expr_depth(left).max(max_expr_depth(right)),
        Expr::Call { args, .. } => {
            let mut m = args.len();
            for (i, a) in args.iter().enumerate() {
                m = m.max(i + max_expr_depth(a));
            }
            m
        }
    }
}

fn max_call_stack_args(program: &Program) -> usize {
    let mut max = 0;
    for stmt in &program.statements {
        max = max.max(max_call_stmt(stmt));
    }
    max
}

fn max_call_stmt(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Let { value, .. }
        | Stmt::Var { value, .. }
        | Stmt::Const { value, .. }
        | Stmt::Assign { value, .. }
        | Stmt::Expr(value) => max_call_expr(value),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let mut m = max_call_expr(condition);
            for s in &then_branch.statements {
                m = m.max(max_call_stmt(s));
            }
            if let Some(b) = else_branch {
                for s in &b.statements {
                    m = m.max(max_call_stmt(s));
                }
            }
            m
        }
        Stmt::While {
            condition, body, ..
        } => {
            let mut m = max_call_expr(condition);
            for s in &body.statements {
                m = m.max(max_call_stmt(s));
            }
            m
        }
        Stmt::Repeat { count, body, .. } => {
            let mut m = max_call_expr(count);
            for s in &body.statements {
                m = m.max(max_call_stmt(s));
            }
            m
        }
        Stmt::Action { body, .. } => {
            let mut m = 0;
            for s in &body.statements {
                m = m.max(max_call_stmt(s));
            }
            m
        }
        Stmt::UiApp { app, .. } => max_call_ui_app(app),
    }
}

fn max_call_ui_app(app: &UiApp) -> usize {
    let mut m = 0;
    for state in &app.states {
        m = m.max(max_call_expr(&state.value));
    }
    if let Some(window) = &app.window {
        for item in &window.items {
            m = m.max(max_call_ui_item(item));
        }
    }
    m
}

fn max_call_ui_item(item: &UiItem) -> usize {
    match item {
        UiItem::Property(prop) => prop
            .args
            .iter()
            .map(max_call_expr)
            .max()
            .unwrap_or(0),
        UiItem::Component(comp) => max_call_ui_component(comp),
        UiItem::EventHandler(handler) => handler
            .body
            .statements
            .iter()
            .map(max_call_stmt)
            .max()
            .unwrap_or(0),
        UiItem::ChangeHandler(handler) => handler
            .body
            .statements
            .iter()
            .map(max_call_stmt)
            .max()
            .unwrap_or(0),
    }
}

fn max_call_ui_component(comp: &UiComponent) -> usize {
    match comp {
        UiComponent::Layout { items, .. } => items
            .iter()
            .map(max_call_ui_item)
            .max()
            .unwrap_or(0),
        UiComponent::Text { value, .. } | UiComponent::Image { source: value, .. } => {
            max_call_expr(value)
        }
        UiComponent::Button {
            label, handler, ..
        } => {
            let mut m = max_call_expr(label);
            if let Some(h) = handler {
                m = m.max(
                    h.body
                        .statements
                        .iter()
                        .map(max_call_stmt)
                        .max()
                        .unwrap_or(0),
                );
            }
            m
        }
        UiComponent::TextField {
            bound, handler, ..
        }
        | UiComponent::Checkbox {
            bound, handler, ..
        } => {
            let mut m = max_call_expr(bound);
            if let Some(h) = handler {
                m = m.max(
                    h.body
                        .statements
                        .iter()
                        .map(max_call_stmt)
                        .max()
                        .unwrap_or(0),
                );
            }
            m
        }
        UiComponent::Spacer { .. } | UiComponent::Divider { .. } => 0,
    }
}

fn max_call_expr(expr: &Expr) -> usize {
    match expr {
        Expr::Call { args, .. } => {
            let mut m = args.len();
            for a in args {
                m = m.max(max_call_expr(a));
            }
            m
        }
        Expr::Unary { expr, .. } => max_call_expr(expr),
        Expr::Binary { left, right, .. } => max_call_expr(left).max(max_call_expr(right)),
        _ => 0,
    }
}

fn runtime_asm() -> &'static str {
    r#"
.globl __nexa_runtime_error
__nexa_runtime_error:
    sub rsp, 8
    mov rdx, rsi
    mov rsi, rdi
    mov edi, 2
    call write
    mov edi, 1
    call exit

.globl __nexa_div
__nexa_div:
    test rsi, rsi
    jz .Ldiv_zero
    mov rax, -1
    cmp rsi, rax
    jne .Ldiv_ok
    mov rax, 0x8000000000000000
    cmp rdi, rax
    je .Ldiv_overflow
.Ldiv_ok:
    mov rax, rdi
    cqo
    idiv rsi
    ret
.Ldiv_zero:
    sub rsp, 8
    lea rdi, [rip + .Lerr_div_zero]
    mov rsi, 23
    call __nexa_runtime_error
.Ldiv_overflow:
    sub rsp, 8
    lea rdi, [rip + .Lerr_div_overflow]
    mov rsi, 26
    call __nexa_runtime_error

.globl __nexa_mod
__nexa_mod:
    test rsi, rsi
    jz .Lmod_zero
    mov rax, -1
    cmp rsi, rax
    jne .Lmod_ok
    mov rax, 0x8000000000000000
    cmp rdi, rax
    je .Lmod_overflow
.Lmod_ok:
    mov rax, rdi
    cqo
    idiv rsi
    mov rax, rdx
    ret
.Lmod_zero:
    sub rsp, 8
    lea rdi, [rip + .Lerr_div_zero]
    mov rsi, 23
    call __nexa_runtime_error
.Lmod_overflow:
    sub rsp, 8
    lea rdi, [rip + .Lerr_div_overflow]
    mov rsi, 26
    call __nexa_runtime_error

.globl __nexa_int_overflow
__nexa_int_overflow:
    sub rsp, 8
    lea rdi, [rip + .Lerr_int_overflow]
    mov rsi, 17
    call __nexa_runtime_error

.globl __nexa_str_eq
__nexa_str_eq:
    sub rsp, 8
    call strcmp
    add rsp, 8
    test eax, eax
    sete al
    movzx eax, al
    ret

.globl __nexa_concat
__nexa_concat:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    push r15
    sub rsp, 8
    mov r12, rdi
    mov r13, rsi
    mov rdi, r12
    call strlen
    mov r14, rax
    mov rdi, r13
    call strlen
    mov r15, rax
    lea rdi, [r14 + r15 + 1]
    jc .Lconcat_overflow
    call malloc
    test rax, rax
    jz .Lconcat_oom
    mov rbx, rax
    mov rdi, rbx
    mov rsi, r12
    mov rdx, r14
    call memcpy
    lea rdi, [rbx + r14]
    mov rsi, r13
    mov rdx, r15
    call memcpy
    lea rax, [r14 + r15]
    mov byte ptr [rbx + rax], 0
    mov rax, rbx
    add rsp, 8
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    leave
    ret
.Lconcat_overflow:
    lea rdi, [rip + .Lerr_str_size]
    mov rsi, 30
    call __nexa_runtime_error
.Lconcat_oom:
    lea rdi, [rip + .Lerr_oom]
    mov rsi, 14
    call __nexa_runtime_error

__nexa_print_int:
    sub rsp, 8
    mov rsi, rdi
    lea rdi, [rip + .LC_fmt_int]
    xor eax, eax
    call printf
    add rsp, 8
    ret

__nexa_print_float:
    sub rsp, 8
    lea rdi, [rip + .LC_fmt_float]
    mov eax, 1
    call printf
    add rsp, 8
    ret

__nexa_print_string:
    sub rsp, 8
    call puts
    add rsp, 8
    ret

__nexa_print_bool:
    sub rsp, 8
    test rdi, rdi
    jz .Lprint_false
    lea rdi, [rip + .LC_bool_true]
    jmp .Lprint_bool
.Lprint_false:
    lea rdi, [rip + .LC_bool_false]
.Lprint_bool:
    call puts
    add rsp, 8
    ret

.section .rodata
.Lerr_div_zero:
    .ascii "integer divide by zero\n"
.Lerr_div_zero_len = . - .Lerr_div_zero
.Lerr_div_overflow:
    .ascii "integer division overflow\n"
.Lerr_div_overflow_len = . - .Lerr_div_overflow
.Lerr_assert:
    .ascii "assertion failed\n"
.Lerr_assert_len = . - .Lerr_assert
.Lerr_int_overflow:
    .ascii "integer overflow\n"
.Lerr_int_overflow_len = . - .Lerr_int_overflow
.Lerr_str_size:
    .ascii "string concatenation overflow\n"
.Lerr_str_size_len = . - .Lerr_str_size
.Lerr_oom:
    .ascii "out of memory\n"
.Lerr_oom_len = . - .Lerr_oom
.text
"#
}

/// A compile-time constant value, for constant folding.
#[derive(Debug, Clone, PartialEq)]
enum ConstVal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

/// Evaluate a pure expression at compile time. Returns `None` for anything
/// that is not a constant (variables, calls) or whose evaluation could trap
/// (integer division/modulo by zero or overflow, which the runtime reports
/// as an error and must keep trapping).
fn const_eval(expr: &Expr) -> Option<ConstVal> {
    match expr {
        Expr::Literal { lit, .. } => Some(match lit {
            Literal::Int(v) => ConstVal::Int(*v),
            Literal::Float(v) => ConstVal::Float(*v),
            Literal::Str(v) => ConstVal::Str(v.clone()),
            Literal::Bool(v) => ConstVal::Bool(*v),
        }),
        Expr::Unary { op, expr, .. } => {
            let v = const_eval(expr)?;
            match (op, v) {
                (UnaryOp::Negate, ConstVal::Int(v)) => Some(ConstVal::Int(v.checked_neg()?)),
                (UnaryOp::Negate, ConstVal::Float(v)) => Some(ConstVal::Float(-v)),
                (UnaryOp::Not, ConstVal::Bool(v)) => Some(ConstVal::Bool(!v)),
                _ => None,
            }
        }
        Expr::Binary { left, op, right, .. } => {
            if *op == BinaryOp::And {
                return match (const_eval(left)?, const_eval(right)?) {
                    (ConstVal::Bool(a), ConstVal::Bool(b)) => Some(ConstVal::Bool(a && b)),
                    _ => None,
                };
            }
            if *op == BinaryOp::Or {
                return match (const_eval(left)?, const_eval(right)?) {
                    (ConstVal::Bool(a), ConstVal::Bool(b)) => Some(ConstVal::Bool(a || b)),
                    _ => None,
                };
            }
            let l = const_eval(left)?;
            let r = const_eval(right)?;
            match (l, r) {
                (ConstVal::Int(a), ConstVal::Int(b)) => match op {
                    BinaryOp::Add => Some(ConstVal::Int(a.checked_add(b)?)),
                    BinaryOp::Sub => Some(ConstVal::Int(a.checked_sub(b)?)),
                    BinaryOp::Mul => Some(ConstVal::Int(a.checked_mul(b)?)),
                    BinaryOp::Div => Some(ConstVal::Int(a.checked_div(b)?)),
                    BinaryOp::Mod => Some(ConstVal::Int(a.checked_rem(b)?)),
                    BinaryOp::Equal => Some(ConstVal::Bool(a == b)),
                    BinaryOp::NotEqual => Some(ConstVal::Bool(a != b)),
                    BinaryOp::Less => Some(ConstVal::Bool(a < b)),
                    BinaryOp::LessEq => Some(ConstVal::Bool(a <= b)),
                    BinaryOp::Greater => Some(ConstVal::Bool(a > b)),
                    BinaryOp::GreaterEq => Some(ConstVal::Bool(a >= b)),
                    _ => None,
                },
                (ConstVal::Float(a), ConstVal::Float(b)) => match op {
                    BinaryOp::Add => Some(ConstVal::Float(a + b)),
                    BinaryOp::Sub => Some(ConstVal::Float(a - b)),
                    BinaryOp::Mul => Some(ConstVal::Float(a * b)),
                    BinaryOp::Div => Some(ConstVal::Float(a / b)),
                    BinaryOp::Mod => Some(ConstVal::Float(a % b)),
                    BinaryOp::Equal => Some(ConstVal::Bool(a == b)),
                    BinaryOp::NotEqual => Some(ConstVal::Bool(a != b)),
                    BinaryOp::Less => Some(ConstVal::Bool(a < b)),
                    BinaryOp::LessEq => Some(ConstVal::Bool(a <= b)),
                    BinaryOp::Greater => Some(ConstVal::Bool(a > b)),
                    BinaryOp::GreaterEq => Some(ConstVal::Bool(a >= b)),
                    _ => None,
                },
                (ConstVal::Bool(a), ConstVal::Bool(b)) => match op {
                    BinaryOp::Equal => Some(ConstVal::Bool(a == b)),
                    BinaryOp::NotEqual => Some(ConstVal::Bool(a != b)),
                    _ => None,
                },
                (ConstVal::Str(a), ConstVal::Str(b)) => match op {
                    BinaryOp::Add => Some(ConstVal::Str(format!("{}{}", a, b))),
                    BinaryOp::Equal => Some(ConstVal::Bool(a == b)),
                    BinaryOp::NotEqual => Some(ConstVal::Bool(a != b)),
                    _ => None,
                },
                _ => None,
            }
        }
        _ => None,
    }
}

/// Swap a comparison's operands (used when the constant is on the left).
fn flip_cmp_op(op: BinaryOp) -> BinaryOp {
    match op {
        BinaryOp::Less => BinaryOp::Greater,
        BinaryOp::LessEq => BinaryOp::GreaterEq,
        BinaryOp::Greater => BinaryOp::Less,
        BinaryOp::GreaterEq => BinaryOp::LessEq,
        other => other,
    }
}
