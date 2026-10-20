use crate::{ast, diagnostic::Diagnostic, source::Span, types::Type};
#[derive(Debug, Clone)]
pub struct Node<T> {
    pub kind: T,
    pub span: Span,
}
pub type Expr = Node<ExprKind>;
pub type Stmt = Node<StmtKind>;
pub type TypeSyntax = Node<Type>;
#[derive(Debug, Clone, Default)]
pub struct Program {
    pub statements: Vec<Stmt>,
}
#[derive(Debug, Clone)]
pub struct NumericLiteral {
    pub text: String,
}
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Option<TypeSyntax>,
    pub span: Span,
}
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub result: Option<TypeSyntax>,
    pub body: Vec<Stmt>,
}
#[derive(Debug, Clone)]
pub struct ExternFunction {
    pub name: String,
    pub symbol: String,
    pub params: Vec<Param>,
    pub result: TypeSyntax,
}
#[derive(Debug, Clone)]
pub enum ExprKind {
    Number(NumericLiteral),
    Text(String),
    Bool(bool),
    Null,
    List(Vec<Expr>),
    Map(Vec<(String, Expr)>),
    Variable(String),
    Assign(String, Box<Expr>),
    AssignIndex(String, Box<Expr>, Box<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Unary(ast::Op, Box<Expr>),
    Binary(Box<Expr>, ast::Op, Box<Expr>),
    Call(String, Vec<Expr>),
    Cast(TypeSyntax, Box<Expr>),
    NullPointer(TypeSyntax),
    Address(Box<Expr>),
    Load(Box<Expr>),
    Store(Box<Expr>, Box<Expr>),
}
#[derive(Debug, Clone)]
pub enum StmtKind {
    Import(String),
    Let {
        name: String,
        ty: Option<TypeSyntax>,
        value: Expr,
    },
    Print(Expr),
    Expr(Expr),
    Block(Vec<Stmt>),
    If(Expr, Vec<Stmt>, Option<Vec<Stmt>>),
    While(Expr, Vec<Stmt>),
    ForEach(String, Expr, Vec<Stmt>),
    Function(Function),
    ExternFunction(ExternFunction),
    ExportFunction {
        function: Function,
        symbol: String,
    },
    Return(Option<Expr>),
    Break,
    Continue,
    Unsafe(Vec<Stmt>),
}
fn unsupported(span: Span) -> Diagnostic {
    Diagnostic::new(
        "E_UNSUPPORTED",
        span,
        "هذا التركيب يتطلب المترجم الأصلي ولا يدعمه المفسر",
    )
}
pub fn to_legacy(p: &Program) -> Result<Vec<ast::Stmt>, Diagnostic> {
    p.statements.iter().map(stmt_to_legacy).collect()
}
fn expr_to_legacy(e: &Expr) -> Result<ast::Expr, Diagnostic> {
    use ExprKind::*;
    Ok(match &e.kind {
        Number(n) => ast::Expr::Number(
            n.text
                .parse()
                .map_err(|_| Diagnostic::new("E_PARSE", e.span, "عدد غير صحيح"))?,
        ),
        Text(s) => ast::Expr::Text(s.clone()),
        Bool(b) => ast::Expr::Bool(*b),
        Null => ast::Expr::Null,
        Variable(s) => ast::Expr::Variable(s.clone()),
        List(v) => ast::Expr::List(v.iter().map(expr_to_legacy).collect::<Result<_, _>>()?),
        Map(v) => ast::Expr::Map(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), expr_to_legacy(v)?)))
                .collect::<Result<_, Diagnostic>>()?,
        ),
        Assign(n, v) => ast::Expr::Assign(n.clone(), Box::new(expr_to_legacy(v)?)),
        AssignIndex(n, i, v) => ast::Expr::AssignIndex(
            n.clone(),
            Box::new(expr_to_legacy(i)?),
            Box::new(expr_to_legacy(v)?),
        ),
        Index(a, b) => ast::Expr::Index(Box::new(expr_to_legacy(a)?), Box::new(expr_to_legacy(b)?)),
        Unary(o, a) => ast::Expr::Unary(*o, Box::new(expr_to_legacy(a)?)),
        Binary(a, o, b) => ast::Expr::Binary(
            Box::new(expr_to_legacy(a)?),
            *o,
            Box::new(expr_to_legacy(b)?),
        ),
        Call(n, v) => ast::Expr::Call(
            n.clone(),
            v.iter().map(expr_to_legacy).collect::<Result<_, _>>()?,
        ),
        _ => return Err(unsupported(e.span)),
    })
}
fn stmts(v: &[Stmt]) -> Result<Vec<ast::Stmt>, Diagnostic> {
    v.iter().map(stmt_to_legacy).collect()
}
fn stmt_to_legacy(s: &Stmt) -> Result<ast::Stmt, Diagnostic> {
    use StmtKind::*;
    Ok(match &s.kind {
        Import(p) => ast::Stmt::Import(p.clone()),
        Let {
            name,
            ty: None,
            value,
        } => ast::Stmt::Let(name.clone(), expr_to_legacy(value)?),
        Print(e) => ast::Stmt::Print(expr_to_legacy(e)?),
        Expr(e) => ast::Stmt::Expr(expr_to_legacy(e)?),
        Block(v) => ast::Stmt::Block(stmts(v)?),
        If(c, a, b) => ast::Stmt::If(
            expr_to_legacy(c)?,
            stmts(a)?,
            b.as_ref().map(|v| stmts(v)).transpose()?,
        ),
        While(c, b) => ast::Stmt::While(expr_to_legacy(c)?, stmts(b)?),
        ForEach(n, e, b) => ast::Stmt::ForEach(n.clone(), expr_to_legacy(e)?, stmts(b)?),
        Return(e) => ast::Stmt::Return(e.as_ref().map(expr_to_legacy).transpose()?),
        Break => ast::Stmt::Break,
        Continue => ast::Stmt::Continue,
        Function(f) if f.result.is_none() && f.params.iter().all(|p| p.ty.is_none()) => {
            ast::Stmt::Function(ast::Function {
                name: f.name.clone(),
                params: f.params.iter().map(|p| p.name.clone()).collect(),
                body: stmts(&f.body)?,
            })
        }
        _ => return Err(unsupported(s.span)),
    })
}

/// Routes only native-only constructs to the typed checker. The walk includes
/// already flattened imported statements; ordinary dynamic programs stay legacy.
pub fn requires_typed(p: &Program) -> bool {
    p.statements.iter().any(stmt_requires_typed)
}
fn expr_requires_typed(e: &Expr) -> bool {
    use ExprKind::*;
    match &e.kind {
        Cast(..) | NullPointer(..) | Address(..) | Load(..) | Store(..) => true,
        Unary(_, a) | Assign(_, a) => expr_requires_typed(a),
        Binary(a, _, b) | Index(a, b) => expr_requires_typed(a) || expr_requires_typed(b),
        AssignIndex(_, a, b) => expr_requires_typed(a) || expr_requires_typed(b),
        List(v) | Call(_, v) => v.iter().any(expr_requires_typed),
        Map(v) => v.iter().any(|(_, v)| expr_requires_typed(v)),
        _ => false,
    }
}
fn stmt_requires_typed(s: &Stmt) -> bool {
    use StmtKind::*;
    match &s.kind {
        ExternFunction(_) | ExportFunction { .. } | Unsafe(_) => true,
        Let { ty, value, .. } => ty.is_some() || expr_requires_typed(value),
        Function(f) => {
            f.result.is_some()
                || f.params.iter().any(|p| p.ty.is_some())
                || f.body.iter().any(stmt_requires_typed)
        }
        Block(v) => v.iter().any(stmt_requires_typed),
        If(c, y, n) => {
            expr_requires_typed(c)
                || y.iter().any(stmt_requires_typed)
                || n.as_ref()
                    .map(|n| n.iter().any(stmt_requires_typed))
                    .unwrap_or(false)
        }
        While(c, b) | ForEach(_, c, b) => {
            expr_requires_typed(c) || b.iter().any(stmt_requires_typed)
        }
        Print(e) | Expr(e) => expr_requires_typed(e),
        Return(Some(e)) => expr_requires_typed(e),
        _ => false,
    }
}
