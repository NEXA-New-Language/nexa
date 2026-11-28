use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Equal,
    NotEqual,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal {
        lit: Literal,
        span: Span,
    },
    Variable {
        name: Ident,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Ident,
        args: Vec<Expr>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal { span, .. } => *span,
            Expr::Variable { name } => name.span,
            Expr::Unary { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub statements: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: Ident,
        value: Expr,
        span: Span,
    },
    Var {
        name: Ident,
        value: Expr,
        span: Span,
    },
    Const {
        name: Ident,
        value: Expr,
        span: Span,
    },
    Assign {
        target: Ident,
        value: Expr,
        span: Span,
    },
    Action {
        name: Ident,
        params: Vec<Ident>,
        body: Block,
        span: Span,
    },
    If {
        condition: Expr,
        then_branch: Block,
        else_branch: Option<Block>,
        span: Span,
    },
    While {
        condition: Expr,
        body: Block,
        span: Span,
    },
    Repeat {
        count: Expr,
        body: Block,
        span: Span,
    },
    /// A whole NEXA UI 1.0 application (`app("Name") { ... }`, spec section 7).
    /// It is the only top-level item of a UI program.
    UiApp { app: UiApp, span: Span },
    Expr(Expr),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. } => *span,
            Stmt::Var { span, .. } => *span,
            Stmt::Const { span, .. } => *span,
            Stmt::Assign { span, .. } => *span,
            Stmt::Action { span, .. } => *span,
            Stmt::If { span, .. } => *span,
            Stmt::While { span, .. } => *span,
            Stmt::Repeat { span, .. } => *span,
            Stmt::UiApp { span, .. } => *span,
            Stmt::Expr(expr) => expr.span(),
        }
    }
}

// ---------------------------------------------------------------------------
// NEXA UI 1.0 AST (NEXA UI Language Specification 1.0, section 7 EBNF).
//
//   ui_app          = "app", "(", string, ")", "{", { ui_item }, "}" ;
//   ui_item         = state_decl | window_decl ;
//   state_decl      = "state", identifier, "=", expression ;
//   window_decl     = "Window", "{", { window_item }, "}" ;
//   window_item     = property | component ;
//   component       = layout_component | text_component | button_component
//                   | input_component | checkbox_component
//                   | spacer_component | divider_component | image_component ;
//   layout_component = layout_name, "{", { component_item }, "}" ;
//   component_item  = property | component | event_handler | change_handler ;
// ---------------------------------------------------------------------------

/// The root of a UI program: `app("Name") { state ... Window { ... } }`.
#[derive(Debug, Clone, PartialEq)]
pub struct UiApp {
    pub name: String,
    pub name_span: Span,
    pub states: Vec<UiState>,
    pub window: Option<UiWindow>,
    pub span: Span,
}

/// `state count = 0` inside an `app` block. The type is inferred from the
/// initializer (Int, Float, String or Bool) and the state is mutable.
#[derive(Debug, Clone, PartialEq)]
pub struct UiState {
    pub name: Ident,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiWindow {
    pub items: Vec<UiItem>,
    pub span: Span,
}

/// One item inside a Window or layout block.
#[derive(Debug, Clone, PartialEq)]
pub enum UiItem {
    Property(UiProperty),
    Component(UiComponent),
    EventHandler(UiEventHandler),
    ChangeHandler(UiChangeHandler),
}

/// `title("Counter")`, `size(480, 320)`, `spacing(12)`, ...
#[derive(Debug, Clone, PartialEq)]
pub struct UiProperty {
    pub name: Ident,
    pub args: Vec<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    Column,
    Row,
    Container,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiComponent {
    Layout {
        kind: LayoutKind,
        items: Vec<UiItem>,
        span: Span,
    },
    Text {
        value: Expr,
        span: Span,
    },
    Button {
        label: Expr,
        handler: Option<UiEventHandler>,
        span: Span,
    },
    TextField {
        bound: Expr,
        handler: Option<UiChangeHandler>,
        span: Span,
    },
    Checkbox {
        bound: Expr,
        handler: Option<UiChangeHandler>,
        span: Span,
    },
    Spacer {
        span: Span,
    },
    Divider {
        span: Span,
    },
    Image {
        source: Expr,
        span: Span,
    },
}

impl UiComponent {
    pub fn span(&self) -> Span {
        match self {
            UiComponent::Layout { span, .. }
            | UiComponent::Text { span, .. }
            | UiComponent::Button { span, .. }
            | UiComponent::TextField { span, .. }
            | UiComponent::Checkbox { span, .. }
            | UiComponent::Spacer { span, .. }
            | UiComponent::Divider { span, .. }
            | UiComponent::Image { span, .. } => *span,
        }
    }
}

/// `on_click { ... }` on a Button.
#[derive(Debug, Clone, PartialEq)]
pub struct UiEventHandler {
    pub body: Block,
    pub span: Span,
}

/// `on_change(value) { ... }` on a TextField (String) or Checkbox (Bool).
#[derive(Debug, Clone, PartialEq)]
pub struct UiChangeHandler {
    pub param: Ident,
    pub body: Block,
    pub span: Span,
}
