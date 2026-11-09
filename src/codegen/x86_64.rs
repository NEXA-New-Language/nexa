use crate::syntax::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NexaType {
    Int,
    Str,
    Bool,
}

pub struct X86_64CodeGen {
    label_id: usize,
    string_literals: Vec<(String, String)>,
    var_table: HashMap<String, (i32, NexaType)>,
    func_params: HashMap<String, Vec<(String, NexaType)>>,
    current_offset: i32,
}

impl X86_64CodeGen {
    pub fn new() -> Self {
        Self {
            label_id: 0,
            string_literals: Vec::new(),
            var_table: HashMap::new(),
            func_params: HashMap::new(),
            current_offset: 0,
        }
    }

    pub fn generate(mut self, program: &Program) -> String {
        let mut functions_code = String::new();
        let mut main_body = String::new();

        for stmt in &program.statements {
            if let Stmt::Action { name, params, .. } = stmt {
                let p_types = params.iter().map(|p| (p.name.clone(), NexaType::Str)).collect();
                self.func_params.insert(name.name.clone(), p_types);
            }
        }

        for stmt in &program.statements {
            if let Stmt::Action { name, params, body, .. } = stmt {
                self.gen_action(name, params, body, &mut functions_code);
            }
        }

        self.var_table.clear();
        self.current_offset = 0;

        for stmt in &program.statements {
            if !matches!(stmt, Stmt::Action { .. }) {
                self.gen_stmt(stmt, &mut main_body);
            }
        }

        let mut main_code = String::new();
        main_code.push_str(".globl main\n");
        main_code.push_str("main:\n");
        main_code.push_str("    push rbp\n");
        main_code.push_str("    mov rbp, rsp\n");
        main_code.push_str("    sub rsp, 512\n\n");
        main_code.push_str(&main_body);
        main_code.push_str("\n    mov eax, 0\n");
        main_code.push_str("    mov rsp, rbp\n");
        main_code.push_str("    pop rbp\n");
        main_code.push_str("    ret\n");

        let mut rodata_section = String::new();
        rodata_section.push_str(".section .rodata\n");
        rodata_section.push_str(".LC_fmt_int:\n    .string \"%ld\\n\"\n");
        rodata_section.push_str(".LC_fmt_str:\n    .string \"%s\\n\"\n");
        rodata_section.push_str(".LC_bool_true:\n    .string \"true\"\n");
        rodata_section.push_str(".LC_bool_false:\n    .string \"false\"\n");

        for (lbl, content) in &self.string_literals {
            rodata_section.push_str(&format!("{}:\n    .string \"{}\"\n", lbl, content));
        }

        let helper_code = r#"
.globl __nexa_str_concat
__nexa_str_concat:
    push rbp
    mov rbp, rsp
    push r12
    push r13
    push r14
    push r15
    sub rsp, 16

    mov r12, rdi
    mov r13, rsi

    mov rdi, r12
    call strlen
    mov r14, rax

    mov rdi, r13
    call strlen
    mov r15, rax

    lea rdi, [r14 + r15 + 1]
    call malloc
    mov rbx, rax

    mov rdi, rbx
    mov rsi, r12
    call strcpy

    mov rdi, rbx
    mov rsi, r13
    call strcat

    mov rax, rbx
    add rsp, 16
    pop r15
    pop r14
    pop r13
    pop r12
    mov rsp, rbp
    pop rbp
    ret
"#;

        format!(
            ".intel_syntax noprefix\n\n{}\n.text\n{}\n{}\n{}\n.section .note.GNU-stack,\"\",@progbits\n",
            rodata_section, helper_code, functions_code, main_code
        )
    }

    fn gen_action(&mut self, name: &Ident, params: &[Ident], body: &Block, out: &mut String) {
        self.var_table.clear();
        self.current_offset = 0;

        out.push_str(&format!(".globl {}\n", name.name));
        out.push_str(&format!("{}:\n", name.name));
        out.push_str("    push rbp\n");
        out.push_str("    mov rbp, rsp\n");
        out.push_str("    sub rsp, 256\n");

        let arg_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        for (i, param) in params.iter().enumerate() {
            if i < arg_regs.len() {
                self.current_offset -= 8;
                self.var_table.insert(param.name.clone(), (self.current_offset, NexaType::Str));
                out.push_str(&format!("    mov [rbp {}], {}\n", self.current_offset, arg_regs[i]));
            }
        }

        for stmt in &body.statements {
            self.gen_stmt(stmt, out);
        }

        out.push_str("    mov eax, 0\n");
        out.push_str("    mov rsp, rbp\n");
        out.push_str("    pop rbp\n");
        out.push_str("    ret\n\n");
    }

    fn gen_stmt(&mut self, stmt: &Stmt, out: &mut String) {
        match stmt {
            Stmt::Let { name, value, .. }
            | Stmt::Var { name, value, .. }
            | Stmt::Const { name, value, .. } => {
                let ty = self.gen_expr(value, out);
                self.current_offset -= 8;
                self.var_table.insert(name.name.clone(), (self.current_offset, ty));
                out.push_str("    pop rax\n");
                out.push_str(&format!("    mov [rbp {}], rax\n\n", self.current_offset));
            }
            Stmt::Assign { target, value, .. } => {
                self.gen_expr(value, out);
                let (offset, _) = *self.var_table.get(&target.name).expect("Variable must exist");
                out.push_str("    pop rax\n");
                out.push_str(&format!("    mov [rbp {}], rax\n\n", offset));
            }
            Stmt::If { condition, then_branch, else_branch, .. } => {
                let else_lbl = self.new_label("else");
                let end_lbl = self.new_label("endif");

                self.gen_expr(condition, out);
                out.push_str("    pop rax\n");
                out.push_str("    cmp rax, 0\n");
                if else_branch.is_some() {
                    out.push_str(&format!("    je {}\n", else_lbl));
                } else {
                    out.push_str(&format!("    je {}\n", end_lbl));
                }

                for s in &then_branch.statements {
                    self.gen_stmt(s, out);
                }

                if let Some(eb) = else_branch {
                    out.push_str(&format!("    jmp {}\n", end_lbl));
                    out.push_str(&format!("{}:\n", else_lbl));
                    for s in &eb.statements {
                        self.gen_stmt(s, out);
                    }
                }
                out.push_str(&format!("{}:\n", end_lbl));
            }
            Stmt::While { condition, body, .. } => {
                let loop_start = self.new_label("while_start");
                let loop_end = self.new_label("while_end");

                out.push_str(&format!("{}:\n", loop_start));
                self.gen_expr(condition, out);
                out.push_str("    pop rax\n");
                out.push_str("    cmp rax, 0\n");
                out.push_str(&format!("    je {}\n", loop_end));

                for s in &body.statements {
                    self.gen_stmt(s, out);
                }

                out.push_str(&format!("    jmp {}\n", loop_start));
                out.push_str(&format!("{}:\n", loop_end));
            }
            Stmt::Repeat { count, body, .. } => {
                let repeat_start = self.new_label("repeat_start");
                let repeat_end = self.new_label("repeat_end");

                self.current_offset -= 8;
                let counter_offset = self.current_offset;

                self.gen_expr(count, out);
                out.push_str("    pop rax\n");
                out.push_str(&format!("    mov [rbp {}], rax\n", counter_offset));

                out.push_str(&format!("{}:\n", repeat_start));
                out.push_str(&format!("    mov rax, [rbp {}]\n", counter_offset));
                out.push_str("    cmp rax, 0\n");
                out.push_str(&format!("    jle {}\n", repeat_end));

                for s in &body.statements {
                    self.gen_stmt(s, out);
                }

                out.push_str(&format!("    sub qword ptr [rbp {}], 1\n", counter_offset));
                out.push_str(&format!("    jmp {}\n", repeat_start));
                out.push_str(&format!("{}:\n", repeat_end));
            }
            Stmt::Expr(expr) => {
                self.gen_expr(expr, out);
                out.push_str("    add rsp, 8\n\n");
            }
            Stmt::Action { .. } => {}
        }
    }

    fn gen_expr(&mut self, expr: &Expr, out: &mut String) -> NexaType {
        match expr {
            Expr::Literal { lit, .. } => match lit {
                Literal::Int(val) => {
                    out.push_str(&format!("    mov rax, {}\n", val));
                    out.push_str("    push rax\n");
                    NexaType::Int
                }
                Literal::Bool(b) => {
                    let val = if *b { 1 } else { 0 };
                    out.push_str(&format!("    mov rax, {}\n", val));
                    out.push_str("    push rax\n");
                    NexaType::Bool
                }
                Literal::Str(s) => {
                    let lbl = self.new_label("str");
                    self.string_literals.push((lbl.clone(), s.clone()));
                    out.push_str(&format!("    lea rax, [rip + {}]\n", lbl));
                    out.push_str("    push rax\n");
                    NexaType::Str
                }
                Literal::Float(_) => NexaType::Int,
            },
            Expr::Variable { name } => {
                let (offset, ty) = *self.var_table.get(&name.name).expect("Variable must exist");
                out.push_str(&format!("    mov rax, [rbp {}]\n", offset));
                out.push_str("    push rax\n");
                ty
            }
            Expr::Unary { op, expr, .. } => {
                self.gen_expr(expr, out);
                out.push_str("    pop rax\n");
                match op {
                    UnaryOp::Negate => {
                        out.push_str("    neg rax\n");
                        out.push_str("    push rax\n");
                        NexaType::Int
                    }
                    UnaryOp::Not => {
                        out.push_str("    cmp rax, 0\n");
                        out.push_str("    sete al\n");
                        out.push_str("    movzx rax, al\n");
                        out.push_str("    push rax\n");
                        NexaType::Bool
                    }
                }
            }
            Expr::Binary { left, op, right, .. } => {
                let ty_left = self.gen_expr(left, out);
                let ty_right = self.gen_expr(right, out);

                out.push_str("    pop rbx\n");
                out.push_str("    pop rax\n");

                if *op == BinaryOp::Add && (ty_left == NexaType::Str || ty_right == NexaType::Str) {
                    out.push_str("    mov rdi, rax\n");
                    out.push_str("    mov rsi, rbx\n");
                    out.push_str("    call __nexa_str_concat\n");
                    out.push_str("    push rax\n");
                    return NexaType::Str;
                }

                match op {
                    BinaryOp::Add => out.push_str("    add rax, rbx\n"),
                    BinaryOp::Sub => out.push_str("    sub rax, rbx\n"),
                    BinaryOp::Mul => out.push_str("    imul rax, rbx\n"),
                    BinaryOp::Div => {
                        out.push_str("    cqo\n");
                        out.push_str("    idiv rbx\n");
                    }
                    BinaryOp::Mod => {
                        out.push_str("    cqo\n");
                        out.push_str("    idiv rbx\n");
                        out.push_str("    mov rax, rdx\n");
                    }
                    BinaryOp::Equal => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    sete al\n");
                        out.push_str("    movzx rax, al\n");
                        out.push_str("    push rax\n");
                        return NexaType::Bool;
                    }
                    BinaryOp::NotEqual => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setne al\n");
                        out.push_str("    movzx rax, al\n");
                        out.push_str("    push rax\n");
                        return NexaType::Bool;
                    }
                    BinaryOp::Less => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setl al\n");
                        out.push_str("    movzx rax, al\n");
                        out.push_str("    push rax\n");
                        return NexaType::Bool;
                    }
                    BinaryOp::LessEq => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setle al\n");
                        out.push_str("    movzx rax, al\n");
                        out.push_str("    push rax\n");
                        return NexaType::Bool;
                    }
                    BinaryOp::Greater => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setg al\n");
                        out.push_str("    movzx rax, al\n");
                        out.push_str("    push rax\n");
                        return NexaType::Bool;
                    }
                    BinaryOp::GreaterEq => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setge al\n");
                        out.push_str("    movzx rax, al\n");
                        out.push_str("    push rax\n");
                        return NexaType::Bool;
                    }
                    BinaryOp::And => {
                        out.push_str("    and rax, rbx\n");
                        out.push_str("    push rax\n");
                        return NexaType::Bool;
                    }
                    BinaryOp::Or => {
                        out.push_str("    or rax, rbx\n");
                        out.push_str("    push rax\n");
                        return NexaType::Bool;
                    }
                }
                out.push_str("    push rax\n");
                NexaType::Int
            }
            Expr::Call { callee, args, .. } => {
                if callee.name == "print" {
                    let arg_type = self.gen_expr(&args[0], out);
                    out.push_str("    pop rsi\n");
                    match arg_type {
                        NexaType::Str => out.push_str("    lea rdi, [rip + .LC_fmt_str]\n"),
                        NexaType::Int => out.push_str("    lea rdi, [rip + .LC_fmt_int]\n"),
                        NexaType::Bool => {
                            let true_lbl = self.new_label("btrue");
                            let end_lbl = self.new_label("bend");
                            out.push_str("    cmp rsi, 0\n");
                            out.push_str(&format!("    jne {}\n", true_lbl));
                            out.push_str("    lea rdi, [rip + .LC_bool_false]\n");
                            out.push_str(&format!("    jmp {}\n", end_lbl));
                            out.push_str(&format!("{}:\n", true_lbl));
                            out.push_str("    lea rdi, [rip + .LC_bool_true]\n");
                            out.push_str(&format!("{}:\n", end_lbl));
                            out.push_str("    call puts\n");
                            out.push_str("    push 0\n");
                            return NexaType::Int;
                        }
                    }
                    out.push_str("    mov eax, 0\n");
                    out.push_str("    call printf\n");
                    out.push_str("    push 0\n");
                    NexaType::Int
                } else {
                    let arg_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
                    for arg in args {
                        self.gen_expr(arg, out);
                    }
                    for i in (0..args.len()).rev() {
                        if i < arg_regs.len() {
                            out.push_str(&format!("    pop {}\n", arg_regs[i]));
                        }
                    }
                    out.push_str(&format!("    call {}\n", callee.name));
                    out.push_str("    push rax\n");
                    NexaType::Int
                }
            }
        }
    }

    fn new_label(&mut self, prefix: &str) -> String {
        self.label_id += 1;
        format!(".L_{}_{}", prefix, self.label_id)
    }
}
