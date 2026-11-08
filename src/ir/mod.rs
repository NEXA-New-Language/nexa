#[derive(Debug, Clone)]
pub enum IrOp {
    LoadConst(usize),
    LoadVar(String),
    StoreVar(String),
    Call(String, usize),
    BranchIfFalse(usize),
    Jump(usize),
    Return,
}
