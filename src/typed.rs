//! Checked native IR. Integer arithmetic and numeric Cast nodes require runtime
//! overflow/range checks; calls are resolved by FunctionId, never source names.
use crate::{
    ast::Op,
    source::Span,
    types::{FunctionSig, Type},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);
#[derive(Debug, Clone)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: Type,
    pub span: Span,
}
#[derive(Debug, Clone)]
pub enum TypedExprKind {
    Int(i128),
    Float(f64),
    Bool(bool),
    Text(String),
    TextLength(Box<TypedExpr>),
    TextIndex(Box<TypedExpr>, Box<TypedExpr>),
    Variable(LocalId),
    Assign(LocalId, Box<TypedExpr>),
    Unary(Op, Box<TypedExpr>),
    Binary(Box<TypedExpr>, Op, Box<TypedExpr>),
    Call {
        callee: FunctionId,
        args: Vec<TypedExpr>,
    },
    Cast(Box<TypedExpr>),
    NullPointer,
    Address(LocalId),
    Load(Box<TypedExpr>),
    Store {
        pointer: Box<TypedExpr>,
        value: Box<TypedExpr>,
    },
}
#[derive(Debug, Clone)]
pub struct TypedStmt {
    pub kind: TypedStmtKind,
    pub span: Span,
}
#[derive(Debug, Clone)]
pub enum TypedStmtKind {
    Let {
        id: LocalId,
        name: String,
        ty: Type,
        value: TypedExpr,
    },
    Print(TypedExpr),
    Expr(TypedExpr),
    Block(Vec<TypedStmt>),
    If(TypedExpr, Vec<TypedStmt>, Option<Vec<TypedStmt>>),
    While(TypedExpr, Vec<TypedStmt>),
    Return(Option<TypedExpr>),
    Break,
    Continue,
}
#[derive(Debug, Clone)]
pub struct TypedFunction {
    pub id: FunctionId,
    pub name: String,
    pub symbol: String,
    pub signature: FunctionSig,
    pub params: Vec<LocalId>,
    pub body: Vec<TypedStmt>,
    pub span: Span,
    pub annotated: bool,
}
#[derive(Debug, Clone)]
pub struct ExternFunction {
    pub id: FunctionId,
    pub name: String,
    pub symbol: String,
    pub signature: FunctionSig,
    pub span: Span,
}
#[derive(Debug, Clone)]
pub struct ExportFunction {
    pub function_id: FunctionId,
    pub symbol: String,
    pub span: Span,
}
#[derive(Debug, Clone, Default)]
pub struct TypedProgram {
    pub functions: Vec<TypedFunction>,
    pub externs: Vec<ExternFunction>,
    pub exports: Vec<ExportFunction>,
    pub main: Vec<TypedStmt>,
}
