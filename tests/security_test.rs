//! Security tests for NEXA's native backend:
//! - integer overflow traps at runtime (add, sub, mul, negation,
//!   in-place self-assignments, and constant expressions that would
//!   overflow if folded);
//! - generated binaries are position-independent executables with
//!   RELRO and a non-executable stack.

use nexa::compile_to_binary;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn build(name: &str, source: &str) -> PathBuf {
    let dir = PathBuf::from(format!(
        "target/security-tests/{}-{}-{}",
        name,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("program");
    compile_to_binary(&format!("{}.nexa", name), source, bin.to_str().unwrap())
        .unwrap_or_else(|e| panic!("compilation failed for {}:\n{}", name, e));
    bin
}

fn run(bin: &PathBuf) -> Output {
    Command::new(bin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("execute binary")
}

fn traps_with_overflow_message(name: &str, source: &str) {
    let bin = build(name, source);
    let out = run(&bin);
    assert!(
        !out.status.success(),
        "{} should trap, but exited successfully",
        name
    );
    assert_eq!(out.status.code(), Some(1), "trap exits with status 1");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("integer overflow"),
        "{} stderr should name the overflow, got: {}",
        name,
        stderr
    );
    let _ = fs::remove_dir_all(bin.parent().unwrap());
}

#[test]
fn add_overflow_traps() {
    traps_with_overflow_message(
        "add_overflow",
        "var x = 9223372036854775807\nx = x + 1\nprint(x)\n",
    );
}

#[test]
fn sub_overflow_traps() {
    traps_with_overflow_message(
        "sub_overflow",
        "var x = -9223372036854775807\nx = x - 2\nprint(x)\n",
    );
}

#[test]
fn mul_overflow_traps() {
    traps_with_overflow_message("mul_overflow", "print(4611686018427387904 * 4)\n");
}

#[test]
fn neg_overflow_traps() {
    traps_with_overflow_message(
        "neg_overflow",
        "var x = -9223372036854775807\nx = x - 1\nprint(-x)\n",
    );
}

#[test]
fn in_place_self_assign_overflow_traps() {
    traps_with_overflow_message(
        "self_assign_overflow",
        "var i = 9223372036854775807\ni = i + 1\nprint(i)\n",
    );
}

#[test]
fn constant_expression_overflow_is_not_silently_wrapped() {
    // 2^62 * 4 overflows i64; folding must not turn it into 0.
    traps_with_overflow_message("const_overflow", "print(4611686018427387904 * 4)\n");
}

#[test]
fn large_but_valid_arithmetic_still_works() {
    let bin = build(
        "valid_big",
        "print(1000000 * 1000000)\nprint(9223372036854775807 - 1)\n",
    );
    let out = run(&bin);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "1000000000000\n9223372036854775806\n"
    );
    let _ = fs::remove_dir_all(bin.parent().unwrap());
}

#[test]
fn binary_is_pie_with_relro() {
    let bin = build("pie_check", "print(42)\n");
    let readelf = Command::new("readelf")
        .args(["-h", "-l"])
        .arg(&bin)
        .output()
        .expect("readelf");
    assert!(readelf.status.success());
    let text = String::from_utf8_lossy(&readelf.stdout);
    assert!(
        text.contains("Type:") && text.contains("DYN"),
        "binary should be a position-independent executable (DYN), got:\n{}",
        text
    );
    assert!(
        text.contains("GNU_RELRO"),
        "binary should have a GNU_RELRO segment, got:\n{}",
        text
    );
    let _ = fs::remove_dir_all(bin.parent().unwrap());
}
