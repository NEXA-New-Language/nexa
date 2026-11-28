//! Code generation optimization tests: constant folding, the self-assign
//! peephole, comparison peepholes and branch fusion. Assembly assertions
//! pin the emitted instructions; native runs pin the behavior.

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
        "target/opt-tests/{}-{}-{}",
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

#[test]
fn folds_constant_arithmetic_and_strings() {
    let out = asm(
        r#"
let x = 2 + 3 * 4
let y = (10 - 4) / 2
let s = "ab" + "cd"
let b = 5 > 3 and not false
print(x)
print(y)
print(s)
print(b)
"#,
    );
    assert!(out.contains("mov rax, 14"), "2 + 3 * 4 folds to 14");
    assert!(out.contains("mov rax, 3"), "(10 - 4) / 2 folds to 3");
    assert!(
        !out.contains("imul rax, rbx"),
        "no multiply is emitted for constant arithmetic"
    );
    assert!(
        !out.contains("call __nexa_concat"),
        "no concat call is emitted for constant strings"
    );
}

#[test]
fn does_not_fold_trapping_division() {
    // Division by zero must still trap at runtime, so it is not folded.
    let out = asm("let x = 1 / 0\nprint(x)\n");
    assert!(out.contains("call __nexa_div"), "1 / 0 keeps the runtime check");
}

#[test]
fn self_assign_uses_in_place_add_and_sub() {
    let out = asm(
        r#"
var i = 0
var j = 10
i = i + 1
j = j - 2
i = i + j
print(i)
print(j)
"#,
    );
    assert!(
        out.contains("add qword ptr [rbp - "),
        "x = x + <expr> updates in place:\n{}",
        out
    );
    assert!(
        out.contains("sub qword ptr [rbp - "),
        "x = x - <const> updates in place:\n{}",
        out
    );
}

#[test]
fn loop_condition_compares_in_place() {
    let out = asm(
        r#"
var sum = 0
var i = 0
while i < 10000000 {
    sum = sum + i
    i = i + 1
}
print(sum)
"#,
    );
    assert!(
        out.contains("cmp qword ptr [rbp - "),
        "while condition compares the slot directly:\n{}",
        out
    );
    assert!(
        out.contains("jge "),
        "while jumps off the comparison flags:\n{}",
        out
    );
    // The fused branch must not materialize the condition as 0/1.
    assert!(
        !out.contains("setl al"),
        "no setcc for the fused loop condition:\n{}",
        out
    );
}

#[test]
fn constant_if_keeps_only_the_taken_branch() {
    let out = run_native(
        "const_if",
        "if true { print(42) } else { print(99) }\nif false { print(99) } else { print(7) }\n",
    );
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n7\n");
}

#[test]
fn while_false_never_runs() {
    let out = run_native("while_false", "while false { print(9) }\nprint(1)\n");
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "1\n");
}

#[test]
fn optimized_loop_still_computes_the_right_sum() {
    let out = run_native(
        "sum_loop",
        r#"
var sum = 0
var i = 0
while i < 10000000 {
    sum = sum + i
    i = i + 1
}
print(sum)
"#,
    );
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "49999995000000\n"
    );
}

#[test]
fn chained_self_assign_and_repeat_still_work() {
    let out = run_native(
        "chain",
        r#"
var x = 2
repeat 5 {
    x = x + 3
}
print(x)
var y = 100
y = y - 1
y = y - 1
print(y)
"#,
    );
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "17\n98\n");
}
