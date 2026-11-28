//! NEXA UI 1.0 tests: parsing, semantic analysis and native execution
//! (headless backend) of UI programs from the specification.

use nexa::{analyze_source, compile_to_binary, parse_source};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const COUNTER: &str = include_str!("../examples/ui_counter.nexa");
const FORM: &str = include_str!("../examples/ui_form.nexa");
const CALCULATOR: &str = include_str!("../examples/ui_calculator.nexa");

fn accepts(source: &str) {
    let result = analyze_source("test.nexa", source);
    assert!(
        result.is_ok(),
        "Expected valid UI source, got: {:?}",
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
    assert_eq!(result.unwrap_err().kind, "Semantic Error");
}

fn parse_rejects(source: &str) {
    assert!(
        parse_source("test.nexa", source).is_err(),
        "Expected parse error, but source was accepted:\n{}",
        source
    );
}

fn run_headless(name: &str, source: &str, stdin_input: &str) -> Output {
    let dir = PathBuf::from(format!(
        "target/ui-tests/{}-{}-{}",
        name,
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("program");
    compile_to_binary(&format!("{}.nexa", name), source, bin.to_str().unwrap())
        .unwrap_or_else(|e| panic!("UI compilation failed for {}:\n{}", name, e));
    let mut child = Command::new(&bin)
        .env("NEXA_UI_HEADLESS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("execute UI binary");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin_input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait for UI binary");
    let _ = fs::remove_dir_all(&dir);
    output
}

#[test]
fn parses_spec_counter_example() {
    accepts(COUNTER);
    let program = parse_source("counter.nexa", COUNTER).unwrap();
    assert_eq!(program.statements.len(), 1);
    match &program.statements[0] {
        nexa::syntax::ast::Stmt::UiApp { app, .. } => {
            assert_eq!(app.name, "Counter");
            assert_eq!(app.states.len(), 1);
            assert_eq!(app.states[0].name.name, "count");
            assert!(app.window.is_some());
        }
        other => panic!("Expected Stmt::UiApp, got {:?}", other),
    }
}

#[test]
fn accepts_full_component_surface() {
    accepts(FORM);
}

#[test]
fn accepts_all_layouts_and_properties() {
    accepts(
        r##"
app("Layouts") {
    state label = "Hi"
    state on = true

    Window {
        title("Layouts")
        size(800, 600)
        width(800)
        height(600)
        padding(4)
        margin(2)
        background("#FFFFFF")
        align("center")
        visible(true)

        Row {
            spacing(6)
            padding(10)
            Container {
                Text(label)
                Text(1)
                Text(2.5)
                Text(on)
                Spacer
                Divider
                Image("a.png")
            }
        }
    }
}
"##,
    );
}

#[test]
fn accepts_button_without_handler_and_textfield_without_handler() {
    accepts(
        r#"
app("Plain") {
    state name = "x"
    state ok = false

    Window {
        Column {
            Button("No-op") {
            }
            TextField(name) {
            }
            Checkbox(ok) {
            }
        }
    }
}
"#,
    );
}

#[test]
fn rejects_missing_window() {
    rejects(
        r#"
app("NoWindow") {
    state count = 0
}
"#,
    );
}

#[test]
fn rejects_two_windows() {
    parse_rejects(
        r#"
app("Two") {
    Window {
    }
    Window {
    }
}
"#,
    );
}

#[test]
fn rejects_app_mixed_with_plain_statements() {
    rejects(
        r#"
let x = 1
app("Mixed") {
    Window {
    }
}
"#,
    );
}

#[test]
fn rejects_unknown_component() {
    parse_rejects(
        r#"
app("Bad") {
    Window {
        Column {
            Slider(50)
        }
    }
}
"#,
    );
}

#[test]
fn rejects_unknown_property() {
    parse_rejects(
        r##"
app("Bad") {
    Window {
        Column {
            colour("#FFFFFF")
            Text("x")
        }
    }
}
"##,
    );
}

#[test]
fn rejects_duplicate_property() {
    rejects(
        r#"
app("Bad") {
    Window {
        Column {
            spacing(4)
            spacing(8)
            Text("x")
        }
    }
}
"#,
    );
}

#[test]
fn rejects_window_only_property_on_layout() {
    rejects(
        r#"
app("Bad") {
    Window {
        Column {
            title("Nope")
            Text("x")
        }
    }
}
"#,
    );
}

#[test]
fn rejects_bad_background_color() {
    rejects(
        r#"
app("Bad") {
    Window {
        Column {
            background("red")
            Text("x")
        }
    }
}
"#,
    );
}

#[test]
fn rejects_size_with_wrong_arity_or_type() {
    rejects(
        r#"
app("Bad") {
    Window {
        size(480)
    }
}
"#,
    );
    rejects(
        r#"
app("Bad") {
    Window {
        size(480.5, 320)
    }
}
"#,
    );
}

#[test]
fn rejects_textfield_bound_to_non_state() {
    rejects(
        r#"
app("Bad") {
    state name = "x"

    Window {
        Column {
            TextField("literal") {
                on_change(value) {
                    name = value
                }
            }
        }
    }
}
"#,
    );
}

#[test]
fn rejects_textfield_bound_to_wrong_type() {
    rejects(
        r#"
app("Bad") {
    state count = 0

    Window {
        Column {
            TextField(count) {
                on_change(value) {
                    print(value)
                }
            }
        }
    }
}
"#,
    );
}

#[test]
fn rejects_checkbox_bound_to_wrong_type() {
    rejects(
        r#"
app("Bad") {
    state name = "x"

    Window {
        Column {
            Checkbox(name) {
                on_change(value) {
                    print(value)
                }
            }
        }
    }
}
"#,
    );
}

#[test]
fn rejects_on_click_on_textfield() {
    parse_rejects(
        r#"
app("Bad") {
    state name = "x"

    Window {
        Column {
            TextField(name) {
                on_click {
                    print("nope")
                }
            }
        }
    }
}
"#,
    );
}

#[test]
fn rejects_on_change_on_button() {
    parse_rejects(
        r#"
app("Bad") {
    Window {
        Column {
            Button("B") {
                on_change(value) {
                    print(value)
                }
            }
        }
    }
}
"#,
    );
}

#[test]
fn rejects_state_type_mismatch_on_assign() {
    rejects(
        r#"
app("Bad") {
    state count = 0

    Window {
        Column {
            Button("B") {
                on_click {
                    count = "text"
                }
            }
        }
    }
}
"#,
    );
}

#[test]
fn rejects_duplicate_state() {
    rejects(
        r#"
app("Bad") {
    state count = 0
    state count = 1

    Window {
    }
}
"#,
    );
}

#[test]
fn rejects_text_of_void_value() {
    rejects(
        r#"
app("Bad") {
    Window {
        Column {
            Text(print("x"))
        }
    }
}
"#,
    );
}

#[test]
fn counter_clicks_update_state_and_rerender() {
    let out = run_headless(
        "counter",
        COUNTER,
        "click Increment\nclick Increment\nclick Reset\nquit\n",
    );
    assert!(
        out.status.success(),
        "counter exited with {:?}, stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    // Initial snapshot, then one snapshot per click.
    let snapshots: Vec<&str> = stdout.split("Window Counter size=480x320").collect();
    assert_eq!(snapshots.len(), 5, "expected 4 snapshots, got:\n{}", stdout);
    assert!(snapshots[1].contains("Text 0"), "initial:\n{}", stdout);
    assert!(snapshots[2].contains("Text 1"), "after 1st click:\n{}", stdout);
    assert!(snapshots[3].contains("Text 2"), "after 2nd click:\n{}", stdout);
    assert!(snapshots[4].contains("Text 0"), "after reset:\n{}", stdout);
    assert!(stdout.contains("Text Hello, NEXA!"));
}

#[test]
fn form_typing_and_toggle_update_state() {
    let out = run_headless(
        "form",
        FORM,
        "type name Alice\ntoggle checked\nquit\n",
    );
    assert!(
        out.status.success(),
        "form exited with {:?}, stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let snapshots: Vec<&str> = stdout.split("Window Form size=400x300").collect();
    assert_eq!(snapshots.len(), 4, "expected 3 snapshots, got:\n{}", stdout);
    assert!(snapshots[1].contains("TextField NEXA"));
    assert!(snapshots[2].contains("TextField Alice"), "after typing:\n{}", stdout);
    assert!(snapshots[2].contains("Text Alice"), "after typing:\n{}", stdout);
    assert!(snapshots[3].contains("Checkbox true"), "after toggle:\n{}", stdout);
    assert!(snapshots[3].contains("Text true"), "after toggle:\n{}", stdout);
}

#[test]
fn headless_eof_exits_cleanly() {
    let out = run_headless("counter_eof", COUNTER, "");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Window Counter size=480x320"));
    assert!(stdout.contains("Button Increment"));
}

#[test]
fn accepts_calculator_example() {
    accepts(CALCULATOR);
}

#[test]
fn calculator_computes_and_guards_division_by_zero() {
    let out = run_headless(
        "calculator",
        CALCULATOR,
        "click 7\nclick +\nclick 8\nclick =\nclick C\nclick 9\nclick *\nclick 6\nclick =\nclick 8\nclick /\nclick 2\nclick =\nclick 5\nclick /\nclick 0\nclick =\nquit\n",
    );
    assert!(
        out.status.success(),
        "calculator exited with {:?}, stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let snapshots: Vec<&str> = stdout.split("Window Calculator size=320x420").collect();
    assert_eq!(snapshots.len(), 19, "expected 18 snapshots, got:\n{}", stdout);
    assert!(snapshots[5].contains("Text 15"), "7 + 8:\n{}", stdout);
    assert!(snapshots[10].contains("Text 54"), "9 * 6:\n{}", stdout);
    assert!(snapshots[14].contains("Text 4"), "8 / 2:\n{}", stdout);
    assert!(snapshots[18].contains("Text 0"), "5 / 0 guard:\n{}", stdout);
}

#[test]
fn calculator_chains_operations() {
    let out = run_headless(
        "calculator_chain",
        CALCULATOR,
        "click 2\nclick +\nclick 3\nclick +\nclick 4\nclick =\nquit\n",
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let snapshots: Vec<&str> = stdout.split("Window Calculator size=320x420").collect();
    assert_eq!(snapshots.len(), 8, "expected 7 snapshots, got:\n{}", stdout);
    assert!(snapshots[7].contains("Text 9"), "2 + 3 + 4:\n{}", stdout);
}
