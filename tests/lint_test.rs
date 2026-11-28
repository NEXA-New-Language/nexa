//! Tests for `nexa lint`: unused declarations, unused actions and
//! parameters, shadowing, and UI state usage.

use nexa::diagnostics::Severity;
use nexa::lint_source;

fn lint(source: &str) -> Vec<nexa::diagnostics::Diagnostic> {
    lint_source("test.nexa", source).expect("valid source should parse")
}

fn kinds(source: &str) -> Vec<String> {
    lint(source).iter().map(|w| w.kind.clone()).collect()
}

#[test]
fn unused_let_warns() {
    let warnings = lint("let x = 5\nprint(1)\n");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, "unused variable");
    assert_eq!(warnings[0].severity, Severity::Warning);
    assert!(warnings[0].message.contains("'x'"));
}

#[test]
fn write_only_var_warns() {
    // An assignment is a write, not a read.
    assert_eq!(kinds("var x = 0\nx = 5\nprint(1)\n"), vec!["unused variable"]);
}

#[test]
fn read_var_is_not_flagged() {
    assert!(lint("var x = 0\nx = x + 1\nprint(x)\n").is_empty());
}

#[test]
fn shadowing_warns() {
    let warnings = lint("var x = 1\nif true {\n    var x = 2\n    print(x)\n}\nprint(x)\n");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, "shadowing");
    assert!(warnings[0].message.contains("'x'"));
}

#[test]
fn unused_action_warns_but_called_action_does_not() {
    assert_eq!(
        kinds("action unused() {\n    print(1)\n}\nprint(2)\n"),
        vec!["unused action"]
    );
    assert!(lint("action greet(name) {\n    print(name)\n}\ngreet(\"NEXA\")\n").is_empty());
}

#[test]
fn unused_parameter_warns() {
    let warnings = lint("action f(a, b) {\n    print(a)\n}\nf(1, 2)\n");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, "unused parameter");
    assert!(warnings[0].message.contains("'b'"));
}

#[test]
fn counter_example_is_clean() {
    let source = include_str!("../examples/ui_counter.nexa");
    assert!(lint(source).is_empty());
}

#[test]
fn unused_ui_state_warns() {
    let source = "app(\"Test\") {\n    state count = 0\n    state hidden = 1\n    Window {\n        Column {\n            Text(count)\n        }\n    }\n}\n";
    let warnings = lint(source);
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].kind, "unused state");
    assert!(warnings[0].message.contains("'hidden'"));
}

#[test]
fn warning_renders_with_location() {
    let source = "let x = 5\nprint(1)\n";
    let rendered = lint(source)[0].render(source);
    assert!(rendered.contains("Warning: unused variable"));
    assert!(rendered.contains("test.nexa:1:"));
    assert!(rendered.contains("let x = 5"));
}

#[test]
fn parse_error_is_an_error_not_a_warning() {
    assert!(lint_source("test.nexa", "let = 5\n").is_err());
}
