use nexa::compile_to_binary;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn run_native(name: &str, source: &str) -> Output {
    let dir = PathBuf::from(format!(
        "target/native-tests/{}-{}-{}",
        name,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("program");
    compile_to_binary(&format!("{}.nexa", name), source, bin.to_str().unwrap())
        .unwrap_or_else(|e| panic!("compilation failed for {}:\n{}", name, e));
    let output = Command::new(&bin).output().expect("execute native binary");
    let _ = fs::remove_dir_all(&dir);
    output
}

fn stdout(name: &str, source: &str) -> String {
    let out = run_native(name, source);
    assert!(
        out.status.success(),
        "{} failed: status={:?} stderr={}",
        name,
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn integer_arithmetic_and_comparisons() {
    let out = stdout(
        "int_ops",
        r#"
print(7 + 3 * 2)
print(10 - 4)
print(10 / 3)
print(10 % 3)
print(-5)
print(2 < 3)
print(3 <= 3)
print(4 > 5)
print(5 >= 5)
print(2 == 2)
print(2 != 2)
"#,
    );
    assert_eq!(
        out,
        "13\n6\n3\n1\n-5\ntrue\ntrue\nfalse\ntrue\ntrue\nfalse\n"
    );
}

#[test]
fn float_arithmetic_and_printing() {
    let out = stdout(
        "float_ops",
        r#"
let a = 1.5
let b = 2.25
print(a + b)
print(b - a)
print(a * b)
print(b / a)
print(5.5 % 2.0)
print(-a)
print(a < b)
print(a == 1.5)
print(a != b)
"#,
    );
    assert_eq!(out, "3.75\n0.75\n3.375\n1.5\n1.5\n-1.5\ntrue\ntrue\ntrue\n");
}

#[test]
fn string_concat_equality_and_escapes() {
    let out = stdout(
        "strings",
        r#"
let a = "Hello"
let b = "World"
print(a + ", " + b)
print(a == "Hello")
print(a != b)
print("line\nnext\ttab\\slash\"quote")
print("héllo 世界")
"#,
    );
    assert_eq!(
        out,
        "Hello, World\ntrue\ntrue\nline\nnext\ttab\\slash\"quote\nhéllo 世界\n"
    );
}

#[test]
fn boolean_short_circuit_avoids_division_by_zero() {
    let out = stdout(
        "short_circuit",
        r#"
print(false and (1 / 0 == 0))
print(true or (1 / 0 == 0))
print(true and not false)
"#,
    );
    assert_eq!(out, "false\ntrue\ntrue\n");
}

#[test]
fn sibling_block_scopes_have_distinct_slots() {
    let out = stdout(
        "scopes",
        r#"
if true {
    let x = 1
    print(x)
}
if true {
    let x = 2
    print(x)
}
var i = 0
while i < 2 {
    let x = i + 10
    print(x)
    i = i + 1
}
"#,
    );
    assert_eq!(out, "1\n2\n10\n11\n");
}

#[test]
fn repeat_counter_is_independent_of_body_mutation() {
    let out = stdout(
        "repeat_counter",
        r#"
var n = 3
repeat n {
    print(n)
    n = n - 1
}
print(n)
"#,
    );
    assert_eq!(out, "3\n2\n1\n0\n");
}

#[test]
fn actions_with_numeric_bool_and_string_parameters() {
    let out = stdout(
        "action_types",
        r#"
action add(a, b) {
    print(a + b)
}
action scale(x) {
    print(x * 2.0)
}
action flag(b) {
    print(not b)
}
action greet(name) {
    print("Hi " + name)
}
add(2, 3)
scale(1.25)
flag(false)
greet("NEXA")
"#,
    );
    assert_eq!(out, "5\n2.5\ntrue\nHi NEXA\n");
}

#[test]
fn actions_with_more_than_six_mixed_arguments() {
    let out = stdout(
        "many_args",
        r#"
action many(a, b, c, d, e, f, g, h) {
    print(a + b + c + d + e + f)
    print(g)
    print(h)
}
many(1, 2, 3, 4, 5, 6, 7.5, "tail")
"#,
    );
    assert_eq!(out, "21\n7.5\ntail\n");
}

#[test]
fn forward_calls_and_recursion() {
    let out = stdout(
        "recursion",
        r#"
countdown(3)
action countdown(n) {
    if n > 0 {
        print(n)
        countdown(n - 1)
    }
}
"#,
    );
    assert_eq!(out, "3\n2\n1\n");
}

#[test]
fn large_frame_and_deep_expression() {
    let mut source = String::new();
    for i in 0..120 {
        source.push_str(&format!("let v{} = {}\n", i, i));
    }
    source.push_str("print(v119)\nprint(");
    source.push_str(&"(".repeat(40));
    source.push('1');
    for _ in 0..40 {
        source.push_str(" + 1)");
    }
    source.push_str(")\n");
    let out = stdout("large_frame", &source);
    assert_eq!(out, "119\n41\n");
}

#[test]
fn division_by_zero_reports_runtime_error() {
    let out = run_native("div_zero", "print(1 / 0)\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("integer divide by zero"));
}

#[test]
fn modulo_by_zero_reports_runtime_error() {
    let out = run_native("mod_zero", "print(1 % 0)\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("integer divide by zero"));
}

#[test]
fn signed_division_overflow_reports_runtime_error() {
    let out = run_native(
        "div_overflow",
        "let min = -9223372036854775807 - 1\nprint(min / -1)\n",
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("integer division overflow"));
}
