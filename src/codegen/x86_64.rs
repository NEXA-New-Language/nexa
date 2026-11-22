use crate::semantics::{AnalyzedProgram, SemanticAnalyzer};
use crate::syntax::ast::*;
use crate::types::Type;

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

    fn gen_stmt(&mut self, stmt: &Stmt, depth: usize, out: &mut String) {
        match stmt {
            Stmt::Let { name, value, .. }
            | Stmt::Var { name, value, .. }
            | Stmt::Const { name, value, .. } => {
                let ty = self.gen_expr(value, depth, out);
                let off = self.offset(self.binding(name.span));
                store_rax(out, &ty, off);
            }
            Stmt::Assign { target, value, .. } => {
                let ty = self.gen_expr(value, depth, out);
                let off = self.offset(self.binding(target.span));
                store_rax(out, &ty, off);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let else_lbl = self.label("else");
                let end_lbl = self.label("endif");
                self.gen_expr(condition, depth, out);
                out.push_str("    test rax, rax\n");
                out.push_str(&format!(
                    "    jz {}\n",
                    if else_branch.is_some() {
                        &else_lbl
                    } else {
                        &end_lbl
                    }
                ));
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
                let start = self.label("while_start");
                let end = self.label("while_end");
                out.push_str(&format!("{}:\n", start));
                self.gen_expr(condition, depth, out);
                out.push_str("    test rax, rax\n");
                out.push_str(&format!("    jz {}\n", end));
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
                let off = self.offset(self.binding(name.span));
                load_rax(out, &result_ty, off);
            }
            Expr::Unary { op, expr, .. } => {
                let ty = self.gen_expr(expr, depth, out);
                match op {
                    UnaryOp::Negate if ty == Type::Float => {
                        out.push_str(
                            "    movsd xmm1, [rip + .LC_sign_mask]\n    xorpd xmm0, xmm1\n",
                        );
                    }
                    UnaryOp::Negate => out.push_str("    neg rax\n"),
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
                    BinaryOp::Add => out.push_str("    add rax, rbx\n"),
                    BinaryOp::Sub => out.push_str("    sub rax, rbx\n"),
                    BinaryOp::Mul => out.push_str("    imul rax, rbx\n"),
                    BinaryOp::Div => {
                        out.push_str("    mov rdi, rax\n    mov rsi, rbx\n    call __nexa_div\n")
                    }
                    BinaryOp::Mod => {
                        out.push_str("    mov rdi, rax\n    mov rsi, rbx\n    call __nexa_mod\n")
                    }
                    _ => {
                        out.push_str("    cmp rax, rbx\n");
                        let set = match op {
                            BinaryOp::Equal => "sete",
                            BinaryOp::NotEqual => "setne",
                            BinaryOp::Less => "setl",
                            BinaryOp::LessEq => "setle",
                            BinaryOp::Greater => "setg",
                            BinaryOp::GreaterEq => "setge",
                            _ => unreachable!(),
                        };
                        out.push_str(&format!("    {} al\n    movzx rax, al\n", set));
                    }
                }
            }
            Expr::Call { callee, args, .. } => {
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
.Lerr_str_size:
    .ascii "string concatenation overflow\n"
.Lerr_str_size_len = . - .Lerr_str_size
.Lerr_oom:
    .ascii "out of memory\n"
.Lerr_oom_len = . - .Lerr_oom
.text
"#
}
