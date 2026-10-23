//! Shared structural gate for the restricted owned-text preview.
use crate::{ast::Op, diagnostic::Diagnostic, source::Span, typed::*, types::Type};
fn err(span: Span) -> Diagnostic {
    Diagnostic::new("E_NATIVE_TEXT", span, "نوع أو سياق نص مدار غير مدعوم")
}
fn contains(t: &Type) -> bool {
    match t {
        Type::Text => true,
        Type::Pointer(t) => contains(t),
        _ => false,
    }
}
fn text(e: &TypedExpr) -> bool {
    e.ty == Type::Text
        || match &e.kind {
            TypedExprKind::Text(_)
            | TypedExprKind::TextLength(_)
            | TypedExprKind::TextIndex(_, _) => true,
            TypedExprKind::Unary(_, a)
            | TypedExprKind::Cast(a)
            | TypedExprKind::Load(a)
            | TypedExprKind::Assign(_, a) => text(a),
            TypedExprKind::Binary(a, _, b)
            | TypedExprKind::Store {
                pointer: a,
                value: b,
            } => text(a) || text(b),
            TypedExprKind::Call { args, .. } => args.iter().any(text),
            _ => false,
        }
}
fn expr(e: &TypedExpr, allowed: bool) -> Result<(), Diagnostic> {
    if (!allowed && text(e)) || (matches!(&e.ty, Type::Pointer(_)) && contains(&e.ty)) {
        return Err(err(e.span));
    }
    use TypedExprKind::*;
    if e.ty == Type::Text
        && !matches!(
            &e.kind,
            Text(_)
                | Variable(_)
                | Assign(_, _)
                | Binary(_, Op::Add, _)
                | TextIndex(_, _)
                | Call { .. }
        )
    {
        return Err(err(e.span));
    }
    match &e.kind {
        Text(_) if e.ty != Type::Text => return Err(err(e.span)),
        TextLength(a) => {
            if a.ty != Type::Text
                || e.ty
                    != (Type::Int {
                        signed: false,
                        bits: 64,
                    })
            {
                return Err(err(e.span));
            }
            expr(a, allowed)?;
        }
        TextIndex(a, b) => {
            if a.ty != Type::Text || e.ty != Type::Text || !b.ty.is_numeric() {
                return Err(err(e.span));
            }
            expr(a, allowed)?;
            expr(b, allowed)?;
        }
        Binary(a, op, b) => {
            if matches!(op, Op::And | Op::Or)
                && (a.ty != Type::Bool || b.ty != Type::Bool || e.ty != Type::Bool)
            {
                return Err(err(e.span));
            }
            if e.ty == Type::Text || a.ty == Type::Text || b.ty == Type::Text {
                let result = if matches!(op, Op::Equal | Op::NotEqual) {
                    Type::Bool
                } else if matches!(op, Op::Add) {
                    Type::Text
                } else {
                    return Err(err(e.span));
                };
                if a.ty != Type::Text || b.ty != Type::Text || e.ty != result {
                    return Err(err(e.span));
                }
            }
            expr(a, allowed)?;
            expr(b, allowed)?;
        }
        Assign(_, a) => {
            if e.ty != a.ty {
                return Err(err(e.span));
            }
            expr(a, allowed)?;
        }
        Unary(_, a) | Cast(a) | Load(a) => {
            if a.ty == Type::Text {
                return Err(err(e.span));
            }
            expr(a, allowed)?;
        }
        Store { pointer, value } => {
            if contains(&pointer.ty) || contains(&value.ty) {
                return Err(err(e.span));
            }
            expr(pointer, allowed)?;
            expr(value, allowed)?;
        }
        Call { args, .. } => {
            if matches!(&e.ty, Type::Pointer(_)) && contains(&e.ty)
                || args
                    .iter()
                    .any(|a| matches!(&a.ty, Type::Pointer(_)) && contains(&a.ty))
            {
                return Err(err(e.span));
            }
            for a in args {
                expr(a, allowed)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn stmts(body: &[TypedStmt], allowed: bool) -> Result<(), Diagnostic> {
    for s in body {
        use TypedStmtKind::*;
        match &s.kind {
            Print(TypedExpr {
                kind: TypedExprKind::Text(t),
                ty: Type::Text,
                ..
            }) if !t.contains('\0') => {}
            Let { ty, value, .. } => {
                if (contains(ty) && *ty != Type::Text) || *ty != value.ty {
                    return Err(err(s.span));
                }
                expr(value, allowed)?;
            }
            Print(e) | Expr(e) => expr(e, allowed)?,
            Block(b) => stmts(b, allowed)?,
            If(c, a, b) => {
                if c.ty != Type::Bool {
                    return Err(err(c.span));
                }
                expr(c, allowed)?;
                stmts(a, allowed)?;
                if let Some(b) = b {
                    stmts(b, allowed)?;
                }
            }
            While(c, b) => {
                if c.ty != Type::Bool {
                    return Err(err(c.span));
                }
                expr(c, allowed)?;
                stmts(b, allowed)?;
            }
            Return(Some(e)) => expr(e, allowed)?,
            _ => {}
        }
    }
    Ok(())
}
fn bindings_expr(
    e: &TypedExpr,
    locals: &std::collections::HashMap<LocalId, Type>,
) -> Result<(), Diagnostic> {
    use TypedExprKind::*;
    match &e.kind {
        Variable(id) | Assign(id, _) => {
            let bound = locals.get(id);
            if (e.ty == Type::Text || bound == Some(&Type::Text)) && bound != Some(&e.ty) {
                return Err(err(e.span));
            }
            if let Assign(_, a) = &e.kind {
                bindings_expr(a, locals)?;
            }
        }
        Address(id) => {
            if locals.get(id) == Some(&Type::Text) {
                return Err(err(e.span));
            }
        }
        Unary(_, a) | Cast(a) | Load(a) | TextLength(a) => bindings_expr(a, locals)?,
        Binary(a, _, b)
        | TextIndex(a, b)
        | Store {
            pointer: a,
            value: b,
        } => {
            bindings_expr(a, locals)?;
            bindings_expr(b, locals)?;
        }
        Call { args, .. } => {
            for a in args {
                bindings_expr(a, locals)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn bindings(
    body: &[TypedStmt],
    locals: &mut std::collections::HashMap<LocalId, Type>,
) -> Result<(), Diagnostic> {
    for s in body {
        use TypedStmtKind::*;
        match &s.kind {
            Let { id, ty, value, .. } => {
                bindings_expr(value, locals)?;
                if locals.insert(*id, ty.clone()).is_some() {
                    return Err(err(s.span));
                }
            }
            Print(e) | Expr(e) | Return(Some(e)) => bindings_expr(e, locals)?,
            Block(b) => bindings(b, &mut locals.clone())?,
            If(c, a, b) => {
                bindings_expr(c, locals)?;
                bindings(a, &mut locals.clone())?;
                if let Some(b) = b {
                    bindings(b, &mut locals.clone())?;
                }
            }
            While(c, b) => {
                bindings_expr(c, locals)?;
                bindings(b, &mut locals.clone())?;
            }
            _ => {}
        }
    }
    Ok(())
}
pub fn validate(p: &TypedProgram) -> Result<(), Diagnostic> {
    for f in &p.externs {
        if contains(&f.signature.result) || f.signature.params.iter().any(contains) {
            return Err(err(f.span));
        }
    }
    for f in &p.functions {
        if (f.signature.abi == crate::types::Abi::C
            && (contains(&f.signature.result) || f.signature.params.iter().any(contains)))
            || matches!(&f.signature.result, Type::Pointer(_)) && contains(&f.signature.result)
            || f.signature
                .params
                .iter()
                .any(|t| matches!(t, Type::Pointer(_)) && contains(t))
        {
            return Err(err(f.span));
        }
    }
    for f in &p.functions {
        stmts(&f.body, true)?;
        let mut locals = f
            .params
            .iter()
            .copied()
            .zip(f.signature.params.iter().cloned())
            .collect();
        bindings(&f.body, &mut locals)?;
    }
    bindings(&p.main, &mut std::collections::HashMap::new())?;
    stmts(&p.main, true)
}
pub fn uses_runtime(p: &TypedProgram) -> bool {
    fn body(b: &[TypedStmt]) -> bool {
        b.iter().any(|s| match &s.kind {
            TypedStmtKind::Print(TypedExpr {
                kind: TypedExprKind::Text(t),
                ..
            }) if !t.contains('\0') => false,
            TypedStmtKind::Print(e) | TypedStmtKind::Expr(e) | TypedStmtKind::Return(Some(e)) => {
                text(e)
            }
            TypedStmtKind::Let { value, .. } => text(value),
            TypedStmtKind::Block(b) => body(b),
            TypedStmtKind::If(c, a, b) => {
                text(c) || body(a) || b.as_ref().map_or(false, |b| body(b))
            }
            TypedStmtKind::While(c, b) => text(c) || body(b),
            _ => false,
        })
    }
    body(&p.main)
        || p.functions.iter().any(|f| {
            f.signature.result == Type::Text
                || f.signature.params.contains(&Type::Text)
                || body(&f.body)
        })
}
