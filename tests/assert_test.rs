//! Tests for the `assert(condition)` builtin: a passing assertion is a
//! no-op, a failing one prints `assertion failed` to stderr and exits 1,
//! and misuse is a compile-time error.

use nexa::compile_to_binary;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn build(name: &str, source: &str) -> Result<PathBuf, nexa::diagnostics::Diagnostic> {
    let dir = PathBuf::from(format!(
        "target/assert-tests/{}-{}-{}",
        name,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("program");
    compile_to_binary(&format!("{}.nexa", name), source, bin.to_str().unwrap())
        .map(|_| bin)
}

fn run(bin: &PathBuf) -> std::process::Output {
    Command::new(bin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("execute binary")
}

#[test]
fn passing_assertion_is_a_noop() {
    let bin = build("assert_pass", "assert(1 < 2)\nvar x = 5\nassert(x == 5)\nprint(42)\n")
        .unwrap();
    let out = run(&bin);
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
    let _ = fs::remove_dir_all(bin.parent().unwrap());
}

#[test]
fn failing_assertion_traps() {
    let bin = build("assert_fail", "assert(1 > 2)\nprint(42)\n").unwrap();
    let out = run(&bin);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("assertion failed"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "nothing runs after a failed assert");
    let _ = fs::remove_dir_all(bin.parent().unwrap());
}

#[test]
fn failing_assertion_after_state_change() {
    let bin = build(
        "assert_fail_state",
        "var x = 5\nx = x + 1\nassert(x == 5)\nprint(x)\n",
    )
    .unwrap();
    let out = run(&bin);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("assertion failed"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = fs::remove_dir_all(bin.parent().unwrap());
}

#[test]
fn assert_rejects_non_bool_and_wrong_arity() {
    assert!(build("assert_int", "assert(1)\n").is_err());
    assert!(build("assert_string", "assert(\"yes\")\n").is_err());
    assert!(build("assert_none", "assert()\n").is_err());
    assert!(build("assert_two", "assert(true, true)\n").is_err());
}

#[test]
fn assert_name_is_reserved() {
    assert!(build(
        "assert_action",
        "action assert(x) {\n    print(x)\n}\n",
    )
    .is_err());
}
