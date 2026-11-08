#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    String,
    Bool,
    Void,
    Function { params: Vec<Type>, ret: Box<Type> },
}
