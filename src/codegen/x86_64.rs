use crate::syntax::ast::*;
use std::collections::HashMap;

pub struct X86_64CodeGen {
    label_id: usize,
    string_literals: Vec<(String, String)>, // (label, content)
    var_offsets: HashMap<String, i32>,
    current_offset: i32,
}

impl X86_64CodeGen {
    pub fn new() -> Self {
        Self {
            
            label_id: 0,
            string_literals: Vec::new(),
            var_offsets: HashMap::new(),
            current_offset: 0,
        }
    }

    pub fn generate(mut self, program: &Program) -> String {
        let mut text_section = String::new();

        // main 関数のプロローグ (スタックフレーム構築)
        text_section.push_str(".globl main\n");
        text_section.push_str("main:\n");
        text_section.push_str("    push rbp\n");
        text_section.push_str("    mov rbp, rsp\n");
        text_section.push_str("    sub rsp, 256\n\n"); // ローカル変数用に256バイト確保

        // 各文のコード生成
        for stmt in &program.statements {
            self.gen_stmt(stmt, &mut text_section);
        }

        // main 関数のエピローグ (正常終了: exit code 0)
        text_section.push_str("\n    mov eax, 0\n");
        text_section.push_str("    mov rsp, rbp\n");
        text_section.push_str("    pop rbp\n");
        text_section.push_str("    ret\n");

        // .rodata セクション (文字列・フォーマット指定子)
        let mut rodata_section = String::new();
        rodata_section.push_str(".section .rodata\n");
        rodata_section.push_str(".LC_fmt_int:\n");
        rodata_section.push_str("    .string \"%ld\\n\"\n");
        rodata_section.push_str(".LC_fmt_str:\n");
        rodata_section.push_str("    .string \"%s\\n\"\n");

        for (lbl, content) in &self.string_literals {
            rodata_section.push_str(&format!("{}:\n", lbl));
            rodata_section.push_str(&format!("    .string \"{}\"\n", content));
        }

        format!(
            ".intel_syntax noprefix\n\n{}\n.text\n{}",
            rodata_section, text_section
        )
    }

    fn gen_stmt(&mut self, stmt: &Stmt, out: &mut String) {
        match stmt {
            Stmt::Let { name, value, .. }
            | Stmt::Var { name, value, .. }
            | Stmt::Const { name, value, .. } => {
                self.gen_expr(value, out);
                self.current_offset -= 8;
                self.var_offsets.insert(name.name.clone(), self.current_offset);
                out.push_str("    pop rax\n");
                out.push_str(&format!("    mov [rbp {}], rax\n\n", self.current_offset));
            }
            Stmt::Assign { target, value, .. } => {
                self.gen_expr(value, out);
                let offset = *self.var_offsets.get(&target.name).expect("Variable must exist");
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

                // カウンタ用にスタック領域を割り当て
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
                // 式文の結果は破棄
                out.push_str("    add rsp, 8\n\n");
            }
            Stmt::Action { .. } => {
                // 関数定義は次ステップで拡張
            }
        }
    }

    fn gen_expr(&mut self, expr: &Expr, out: &mut String) {
        match expr {
            Expr::Literal { lit, .. } => match lit {
                Literal::Int(val) => {
                    out.push_str(&format!("    mov rax, {}\n", val));
                    out.push_str("    push rax\n");
                }
                Literal::Bool(b) => {
                    let val = if *b { 1 } else { 0 };
                    out.push_str(&format!("    mov rax, {}\n", val));
                    out.push_str("    push rax\n");
                }
                Literal::Str(s) => {
                    let lbl = self.new_label("str");
                    self.string_literals.push((lbl.clone(), s.clone()));
                    out.push_str(&format!("    lea rax, [rip + {}]\n", lbl));
                    out.push_str("    push rax\n");
                }
                Literal::Float(_) => {
                    // 浮動小数点 (xmmレジスタ) は次ステップで拡張
                }
            },
            Expr::Variable { name } => {
                let offset = *self.var_offsets.get(&name.name).expect("Variable must exist");
                out.push_str(&format!("    mov rax, [rbp {}]\n", offset));
                out.push_str("    push rax\n");
            }
            Expr::Unary { op, expr, .. } => {
                self.gen_expr(expr, out);
                out.push_str("    pop rax\n");
                match op {
                    UnaryOp::Negate => out.push_str("    neg rax\n"),
                    UnaryOp::Not => {
                        out.push_str("    cmp rax, 0\n");
                        out.push_str("    sete al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                }
                out.push_str("    push rax\n");
            }
            Expr::Binary { left, op, right, .. } => {
                self.gen_expr(left, out);
                self.gen_expr(right, out);

                out.push_str("    pop rbx\n"); // 右辺
                out.push_str("    pop rax\n"); // 左辺

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
                    }
                    BinaryOp::NotEqual => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setne al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::Less => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setl al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::LessEq => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setle al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::Greater => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setg al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::GreaterEq => {
                        out.push_str("    cmp rax, rbx\n");
                        out.push_str("    setge al\n");
                        out.push_str("    movzx rax, al\n");
                    }
                    BinaryOp::And => out.push_str("    and rax, rbx\n"),
                    BinaryOp::Or => out.push_str("    or rax, rbx\n"),
                }
                out.push_str("    push rax\n");
            }
            Expr::Call { callee, args, .. } => {
                if callee.name == "print" {
                    let is_str = matches!(args.first(), Some(Expr::Literal { lit: Literal::Str(_), .. }));
                    self.gen_expr(&args[0], out);

                    out.push_str("    pop rsi\n");
                    if is_str {
                        out.push_str("    lea rdi, [rip + .LC_fmt_str]\n");
                    } else {
                        out.push_str("    lea rdi, [rip + .LC_fmt_int]\n");
                    }
                    out.push_str("    mov eax, 0\n");
                    out.push_str("    call printf\n");
                    out.push_str("    push 0\n"); // printのダミー返り値
                }
            }
        }
    }

    fn new_label(&mut self, prefix: &str) -> String {
        self.label_id += 1;
        format!(".L_{}_{}", prefix, self.label_id)
    }
}
