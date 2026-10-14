#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    Text(String),
    Bool(bool),
    Null,
    List(Vec<Expr>),
    Variable(String),
    Assign(String, Box<Expr>),
    AssignIndex(String, Box<Expr>, Box<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Unary(Op, Box<Expr>),
    Binary(Box<Expr>, Op, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Debug, Clone, Copy)]
pub enum Op {
    Add, Subtract, Multiply, Divide, Modulo,
    Equal, NotEqual, Less, LessEqual, Greater, GreaterEqual,
    And, Or, Not, Negate,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Import(String),
    Let(String, Expr),
    Print(Expr),
    Expr(Expr),
    Block(Vec<Stmt>),
    If(Expr, Vec<Stmt>, Option<Vec<Stmt>>),
    While(Expr, Vec<Stmt>),
    ForEach(String, Expr, Vec<Stmt>),
    Function(Function),
    Return(Option<Expr>),
}
