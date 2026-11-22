use nexa::analyze_source;

fn accepts(source: &str) {
    let result = analyze_source("test.nexa", source);
    assert!(
        result.is_ok(),
        "Expected valid source, got: {:?}",
        result.err()
    );
}

fn rejects(source: &str) {
    let result = analyze_source("test.nexa", source);
    assert!(
        result.is_err(),
        "Expected semantic error, but source was accepted:\n{}",
        source
    );

    let error = result.unwrap_err();
    assert_eq!(error.kind, "Semantic Error");
}

#[test]
fn accepts_basic_variables_and_expressions() {
    accepts(
        r#"
let name = "NEXA"
var score = 100
const enabled = true
let ratio = 1.5
let total = score + 20
let message = name + " Language"
"#,
    );
}

#[test]
fn accepts_mutable_variable_assignment() {
    accepts(
        r#"
var count = 1
count = count + 1
"#,
    );
}

#[test]
fn accepts_conditions_and_loops() {
    accepts(
        r#"
var count = 0
if true {
    print("enabled")
}
while count < 3 {
    count = count + 1
}
repeat 2 {
    print("NEXA")
}
"#,
    );
}

#[test]
fn accepts_actions_and_calls() {
    accepts(
        r#"
action greet(target) {
    print("Hello " + target)
}
greet("NEXA")
"#,
    );
}

#[test]
fn rejects_undefined_variable() {
    rejects("print(missing)");
}

#[test]
fn rejects_assignment_to_undefined_variable() {
    rejects("missing = 10");
}

#[test]
fn rejects_assignment_to_immutable_variable() {
    rejects(
        r#"
let score = 10
score = 20
"#,
    );
}

#[test]
fn rejects_duplicate_variable_declaration() {
    rejects(
        r#"
let score = 10
let score = 20
"#,
    );
}

#[test]
fn rejects_mismatched_arithmetic_types() {
    rejects(
        r#"
let result = 10 + "text"
"#,
    );
}

#[test]
fn rejects_non_boolean_if_condition() {
    rejects(
        r#"
if 10 {
    print("invalid")
}
"#,
    );
}

#[test]
fn rejects_non_boolean_while_condition() {
    rejects(
        r#"
while "text" {
    print("invalid")
}
"#,
    );
}

#[test]
fn rejects_non_integer_repeat_count() {
    rejects(
        r#"
repeat 1.5 {
    print("invalid")
}
"#,
    );
}

#[test]
fn rejects_undefined_action() {
    rejects("missing()");
}

#[test]
fn rejects_action_argument_count_mismatch() {
    rejects(
        r#"
action greet(target) {
    print(target)
}
greet("NEXA", "extra")
"#,
    );
}

#[test]
fn rejects_print_without_argument() {
    rejects("print()");
}

#[test]
fn rejects_assignment_to_const() {
    rejects(
        r#"
const limit = 100
limit = 200
"#,
    );
}

#[test]
fn rejects_invalid_unary_not() {
    rejects("let result = not 10");
}

#[test]
fn rejects_invalid_unary_negation() {
    rejects(r#"let result = -"text""#);
}

#[test]
fn rejects_mixed_numeric_types() {
    rejects("let result = 10 + 1.5");
}

#[test]
fn rejects_invalid_logical_operands() {
    rejects("let result = true and 10");
}

#[test]
fn accepts_boolean_logic() {
    accepts(
        r#"
let first = true
let second = false
let result = first and not second
"#,
    );
}

#[test]
fn accepts_numeric_comparisons() {
    accepts(
        r#"
let first = 10 < 20
let second = 3.5 >= 1.5
"#,
    );
}

#[test]
fn rejects_action_argument_type_mismatch() {
    rejects(
        r#"
action greet(target) {
    print("Hello " + target)
}
greet(123)
"#,
    );
}
