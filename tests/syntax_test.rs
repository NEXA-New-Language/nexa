use nexa::parse_source;
use nexa::syntax::ast::*;

#[test]
fn test_required_specification_code() {
    let code = r#"let name = "NEXA"
var score = 100

action greet(target) {
    print("Hello " + target)
}

if score >= 100 {
    greet(name)
}
"#;

    let program = parse_source("main.nexa", code).expect("Parsing must succeed");
    assert_eq!(program.statements.len(), 4);

    match &program.statements[0] {
        Stmt::Let { name, value, .. } => {
            assert_eq!(name.name, "name");
            assert_eq!(value, &Expr::Literal {
                lit: Literal::Str("NEXA".to_string()),
                span: value.span(),
            });
        }
        other => panic!("Expected Stmt::Let, got {:?}", other),
    }

    match &program.statements[1] {
        Stmt::Var { name, value, .. } => {
            assert_eq!(name.name, "score");
            assert_eq!(value, &Expr::Literal {
                lit: Literal::Int(100),
                span: value.span(),
            });
        }
        other => panic!("Expected Stmt::Var, got {:?}", other),
    }

    match &program.statements[2] {
        Stmt::Action { name, params, body, .. } => {
            assert_eq!(name.name, "greet");
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].name, "target");
            assert_eq!(body.statements.len(), 1);
        }
        other => panic!("Expected Stmt::Action, got {:?}", other),
    }

    match &program.statements[3] {
        Stmt::If { condition, then_branch, else_branch, .. } => {
            match condition {
                Expr::Binary { op, .. } => assert_eq!(*op, BinaryOp::GreaterEq),
                other => panic!("Expected GreaterEq condition, got {:?}", other),
            }
            assert_eq!(then_branch.statements.len(), 1);
            assert!(else_branch.is_none());
        }
        other => panic!("Expected Stmt::If, got {:?}", other),
    }
}

#[test]
fn test_semicolon_forbidden_error() {
    let code = "let score = 100;";
    let result = parse_source("forbidden.nexa", code);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind, "Forbidden syntax");
}
