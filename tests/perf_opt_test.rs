//! Performance optimization tests: the float self-assign peephole,
//! constant-divisor strength reduction, and capacity-based strings.
//! Assembly assertions pin the emitted instructions; native runs pin
//! the behavior.

use nexa::{compile_to_assembly, compile_to_binary};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn asm(source: &str) -> String {
    compile_to_assembly("test.nexa", source).expect("compiles")
}

fn run_native(name: &str, source: &str) -> Output {
    let dir = PathBuf::from(format!(
        "target/perf-tests/{}-{}-{}",
        name,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("program");
    compile_to_binary(&format!("{}.nexa", name), source, bin.to_str().unwrap())
        .unwrap_or_else(|e| panic!("compilation failed for {}:\n{}", name, e));
    let output = Command::new(&bin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("execute binary");
    let _ = fs::remove_dir_all(&dir);
    output
}

fn stdout(name: &str, source: &str) -> String {
    let out = run_native(name, source);
    assert!(out.status.success(), "program failed: {:?}", out);
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn float_self_assign_add_const_uses_peephole() {
    let out = asm("var s = 0.0\ns = s + 0.5\nprint(s)\n");
    assert!(
        out.contains("addsd xmm0, [rip +"),
        "const float add should use the peephole:\n{}",
        out
    );
}

#[test]
fn float_self_assign_sub_const_uses_peephole() {
    let out = asm("var s = 1.0\ns = s - 0.25\nprint(s)\n");
    assert!(
        out.contains("subsd xmm0, [rip +"),
        "const float sub should use the peephole:\n{}",
        out
    );
}

#[test]
fn float_self_assign_var_uses_peephole() {
    let out = asm("var s = 1.0\nvar t = 2.0\ns = s + t\nprint(s)\n");
    assert!(
        out.contains("addsd xmm0, [rbp - "),
        "float add of a variable should add in place:\n{}",
        out
    );
    let out = asm("var s = 1.0\nvar t = 2.0\ns = s - t\nprint(s)\n");
    assert!(
        out.contains("subsd xmm0, xmm1"),
        "float sub of a variable should subtract in place:\n{}",
        out
    );
}

#[test]
fn float_self_assign_add_zero_keeps_sign() {
    // -0.0 + 0.0 == +0.0, so the peephole must not skip the addition.
    assert_eq!(
        stdout("float_neg_zero", "var s = -0.0\ns = s + 0.0\nprint(s)\n"),
        "0\n"
    );
}

#[test]
fn float_self_assign_loop_sums_correctly() {
    let src = "var sum = 0.0\nvar i = 0\nwhile i < 1000000 {\n    sum = sum + 0.5\n    i = i + 1\n}\nprint(sum)\n";
    assert_eq!(stdout("float_loop", src), "500000\n");
}

#[test]
fn const_div_by_magic_uses_multiply_not_idiv() {
    let out = asm("var x = 100\nprint(x / 7)\n");
    assert!(
        out.contains("mul rbx"),
        "division by 7 should use the magic multiply:\n{}",
        out
    );
    // The __nexa_div helper is always emitted; only the call matters.
    assert!(
        !out.contains("call __nexa_div"),
        "division by 7 must not call __nexa_div:\n{}",
        out
    );
}

#[test]
fn const_div_by_power_of_two_uses_shift() {
    let out = asm("var x = 100\nprint(x / 8)\n");
    assert!(
        out.contains("sar rax, 3"),
        "division by 8 should shift:\n{}",
        out
    );
    assert!(
        !out.contains("call __nexa_div"),
        "division by 8 must not call __nexa_div:\n{}",
        out
    );
}

#[test]
fn const_mod_uses_magic_and_remainder() {
    let out = asm("var x = 100\nprint(x % 7)\n");
    assert!(
        out.contains("mul rbx"),
        "modulo by 7 should use the magic multiply:\n{}",
        out
    );
    assert!(
        out.contains("imul rax, rdx"),
        "modulo should derive the remainder from the quotient:\n{}",
        out
    );
    assert!(
        !out.contains("call __nexa_mod"),
        "modulo by 7 must not call __nexa_mod:\n{}",
        out
    );
}

#[test]
fn const_div_mod_values_are_correct() {
    let src = "var x = 100\nvar s = 0\ns = s + x / 7\ns = s + x % 7\ns = s + -x / 7\ns = s + -x % 7\ns = s + x / -7\ns = s + x % -7\ns = s + x / 8\ns = s + x % 8\ns = s + x / 1\ns = s + x % 1\ns = s + x / -1\ns = s + x % -1\ns = s + 7 / 7\ns = s + 0 / 7\nprint(s)\n";
    assert_eq!(stdout("const_div_values", src), "5\n");
}

#[test]
fn string_self_append_uses_append_helper() {
    let out = asm("var s = \"\"\ns = s + \"x\"\nprint(s)\n");
    let main_code = out.split("main:\n").nth(1).unwrap();
    assert!(
        main_code.contains("call __nexa_str_append"),
        "self-append should call __nexa_str_append:\n{}",
        main_code
    );
    assert!(
        !main_code.contains("call __nexa_concat"),
        "self-append must not call __nexa_concat:\n{}",
        main_code
    );
}

#[test]
fn string_concat_uses_concat_helper() {
    let out = asm("var s = \"a\"\nvar t = s + \"b\"\nprint(t)\n");
    let main_code = out.split("main:\n").nth(1).unwrap();
    assert!(
        main_code.contains("call __nexa_concat"),
        "a fresh concat should call __nexa_concat:\n{}",
        main_code
    );
    assert!(
        !main_code.contains("call __nexa_str_append"),
        "a fresh concat must not call __nexa_str_append:\n{}",
        main_code
    );
}

#[test]
fn string_self_append_loop_builds_correctly() {
    let src = "var s = \"\"\nvar i = 0\nwhile i < 20000 {\n    s = s + \"x\"\n    i = i + 1\n}\nprint(s)\n";
    assert_eq!(stdout("str_loop", src), format!("{}\n", "x".repeat(20000)));
}

#[test]
fn string_aliasing_stays_correct() {
    // `t` aliases `s`'s buffer, so appending to `s` must allocate a
    // fresh string and leave `t` untouched.
    let src = "var s = \"ab\"\nvar t = s\ns = s + \"cd\"\nprint(t)\nprint(s)\n";
    assert_eq!(stdout("str_alias", src), "ab\nabcd\n");
}

#[test]
fn string_literal_is_not_mutated() {
    let src = "var a = \"ab\"\nvar b = \"ab\"\na = a + \"x\"\nprint(b)\nprint(a)\n";
    assert_eq!(stdout("str_literal", src), "ab\nabx\n");
}

#[test]
fn string_action_arg_is_not_mutated() {
    // The argument aliases the caller's buffer; appending inside the
    // action must not change the caller's string.
    let src = "action shout(name) {\n    name = name + \"!\"\n    print(name)\n}\nvar s = \"hi\"\nshout(s)\nprint(s)\n";
    assert_eq!(stdout("str_action_arg", src), "hi!\nhi\n");
}

#[test]
fn string_concat_still_correct() {
    let src = "var s = \"foo\"\nvar t = \"bar\"\nprint(s + t)\nprint(s)\nprint(t)\n";
    assert_eq!(stdout("str_concat", src), "foobar\nfoo\nbar\n");
}
