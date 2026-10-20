//! Name resolution and scalar type checking before LLVM emission.
use crate::{
    ast::Op,
    diagnostic::Diagnostic,
    ffi,
    source::Span,
    syntax::{self, ExprKind as E, StmtKind as S},
    typed::*,
    types::{Abi, FunctionSig, Type},
};
use std::collections::{HashMap, HashSet};
type Result<T> = std::result::Result<T, Diagnostic>;
#[derive(Clone)]
struct Decl {
    id: FunctionId,
    signature: FunctionSig,
    foreign: bool,
}
#[derive(Clone)]
struct Local {
    id: LocalId,
    ty: Type,
}
struct Checker {
    decls: HashMap<String, Decl>,
    scopes: Vec<HashMap<String, Local>>,
    next_local: u32,
    // Immutable pointer bindings make root sets stable across branches/loops.
    pointer_roots: HashMap<LocalId, HashSet<LocalId>>,
    result: Option<Type>,
    annotated: bool,
    unsafe_depth: usize,
    loop_depth: usize,
}
const FALL: u8 = 1;
const RETURN: u8 = 2;
const BREAK: u8 = 4;
const CONTINUE: u8 = 8;
fn error(code: &'static str, span: Span, msg: impl Into<String>) -> Diagnostic {
    Diagnostic::new(code, span, msg)
}
fn mismatch(span: Span, expected: &Type, actual: &Type) -> Diagnostic {
    error(
        "E_TYPE_MISMATCH",
        span,
        format!("متوقع {expected} لكن النوع هو {actual}"),
    )
}
fn signature(
    params: &[syntax::Param],
    result: Option<&syntax::TypeSyntax>,
    abi: Abi,
) -> FunctionSig {
    FunctionSig {
        params: params
            .iter()
            .map(|p| {
                p.ty.as_ref()
                    .map(|t| t.kind.clone())
                    .unwrap_or(Type::Float { bits: 64 })
            })
            .collect(),
        result: result
            .map(|t| t.kind.clone())
            .unwrap_or(Type::Float { bits: 64 }),
        abi,
    }
}
/// Checks independent function bodies in source order; never returns partial IR.
pub fn check(program: &syntax::Program) -> std::result::Result<TypedProgram, Vec<Diagnostic>> {
    let mut c = Checker {
        decls: HashMap::new(),
        scopes: vec![HashMap::new()],
        next_local: 0,
        pointer_roots: HashMap::new(),
        result: None,
        annotated: false,
        unsafe_depth: 0,
        loop_depth: 0,
    };
    let mut out = TypedProgram::default();
    let mut errors = vec![];
    let mut symbols: HashMap<String, (FunctionSig, Span, bool)> = HashMap::new();
    let mut next = 0;
    for s in &program.statements {
        let (name, sig, foreign, symbol) = match &s.kind {
            S::Function(f) => (
                f.name.clone(),
                signature(&f.params, f.result.as_ref(), Abi::Lisan),
                false,
                None,
            ),
            S::ExportFunction {
                function: f,
                symbol,
            } => (
                f.name.clone(),
                signature(&f.params, f.result.as_ref(), Abi::C),
                false,
                Some(symbol.clone()),
            ),
            S::ExternFunction(f) => (
                f.name.clone(),
                signature(&f.params, Some(&f.result), Abi::C),
                true,
                Some(f.symbol.clone()),
            ),
            _ => continue,
        };
        if c.decls.contains_key(&name) {
            errors.push(error(
                "E_DUPLICATE",
                s.span,
                format!("اسم دالة مكرر: {name}"),
            ));
            continue;
        }
        let id = FunctionId(next);
        next += 1;
        if let Some(sym) = &symbol {
            if let Err(m) = ffi::validate_symbol(sym) {
                errors.push(error("E_FFI_SYMBOL", s.span, m));
            }
            if let Err(m) = ffi::validate_signature(&sig) {
                errors.push(error("E_FFI_ABI", s.span, m));
            }
            if let Some((old, span, was_foreign)) = symbols.get(sym) {
                if !foreign || !*was_foreign || *old != sig {
                    let mut d = error("E_FFI_SYMBOL", s.span, format!("تعارض في رمز الربط {sym}"));
                    d.labels.push((*span, "التصريح السابق".into()));
                    errors.push(d);
                }
            } else {
                symbols.insert(sym.clone(), (sig.clone(), s.span, foreign));
            }
        }
        c.decls.insert(
            name.clone(),
            Decl {
                id,
                signature: sig.clone(),
                foreign,
            },
        );
        if foreign {
            out.externs.push(ExternFunction {
                id,
                name,
                symbol: symbol.unwrap(),
                signature: sig,
                span: s.span,
            });
        } else if let Some(symbol) = symbol {
            out.exports.push(ExportFunction {
                function_id: id,
                symbol,
                span: s.span,
            });
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    for s in &program.statements {
        let f = match &s.kind {
            S::Function(f) | S::ExportFunction { function: f, .. } => f,
            _ => continue,
        };
        let decl = c.decls[&f.name].clone();
        c.scopes = vec![HashMap::new()];
        c.pointer_roots.clear();
        c.result = Some(decl.signature.result.clone());
        c.annotated = f.result.is_some();
        c.unsafe_depth = 0;
        c.loop_depth = 0;
        let mut params = vec![];
        for (p, t) in f.params.iter().zip(&decl.signature.params) {
            match c.bind(&p.name, t.clone(), p.span) {
                Ok(id) => params.push(id),
                Err(e) => errors.push(e),
            }
        }
        match c.block(&f.body, false) {
            Ok((body, flow)) => {
                if c.annotated && decl.signature.result != Type::Void && flow & FALL != 0 {
                    errors.push(error("E_RETURN", s.span, "ليست كل مسارات الدالة تعيد قيمة"));
                } else {
                    out.functions.push(TypedFunction {
                        id: decl.id,
                        name: f.name.clone(),
                        symbol: match &s.kind {
                            S::ExportFunction { symbol, .. } => symbol.clone(),
                            _ => format!("lisan_fn_{}", decl.id.0),
                        },
                        signature: decl.signature,
                        params,
                        body,
                        span: s.span,
                        annotated: c.annotated,
                    });
                }
            }
            Err(e) => errors.push(e),
        }
    }
    c.scopes = vec![HashMap::new()];
    c.pointer_roots.clear();
    c.result = None;
    c.annotated = false;
    c.unsafe_depth = 0;
    c.loop_depth = 0;
    for s in &program.statements {
        if matches!(
            &s.kind,
            S::Function(_) | S::ExternFunction(_) | S::ExportFunction { .. }
        ) {
            continue;
        }
        match c.statement(s) {
            Ok((s, _)) => out.main.push(s),
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}
impl Checker {
    fn bind(&mut self, name: &str, ty: Type, span: Span) -> Result<LocalId> {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return Err(error("E_DUPLICATE", span, format!("متغير مكرر: {name}")));
        }
        let id = LocalId(self.next_local);
        self.next_local += 1;
        scope.insert(name.into(), Local { id, ty });
        Ok(id)
    }
    fn local(&self, name: &str, span: Span) -> Result<Local> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name))
            .cloned()
            .ok_or_else(|| error("E_TYPE_MISMATCH", span, format!("متغير غير معرف: {name}")))
    }
    fn block(&mut self, body: &[syntax::Stmt], scope: bool) -> Result<(Vec<TypedStmt>, u8)> {
        if scope {
            self.scopes.push(HashMap::new());
        }
        let result = (|| {
            let mut stmts = vec![];
            let mut flow = FALL;
            for s in body {
                let (s, next) = self.statement(s)?;
                stmts.push(s);
                if flow & FALL != 0 {
                    flow = (flow & !FALL) | next;
                }
            }
            Ok((stmts, flow))
        })();
        if scope {
            self.scopes.pop();
        }
        result
    }
    fn statement(&mut self, s: &syntax::Stmt) -> Result<(TypedStmt, u8)> {
        let mut flow = FALL;
        let kind = match &s.kind {
            S::Let { name, ty, value } => {
                let value = self.expr(value, ty.as_ref().map(|t| &t.kind))?;
                if value.ty == Type::Void {
                    return Err(error("E_TYPE_MISMATCH", value.span, "فراغ ليس قيمة"));
                }
                let ty = value.ty.clone();
                let roots = self.roots(&value);
                let id = self.bind(name, ty.clone(), s.span)?;
                if matches!(ty, Type::Pointer(_)) {
                    self.pointer_roots.insert(id, roots);
                }
                TypedStmtKind::Let {
                    id,
                    name: name.clone(),
                    ty,
                    value,
                }
            }
            S::Print(e) => {
                let e = self.expr(e, None)?;
                if e.ty == Type::Void || matches!(e.ty, Type::Pointer(_)) {
                    return Err(error("E_UNSUPPORTED", e.span, "لا يمكن طباعة هذا النوع"));
                }
                TypedStmtKind::Print(e)
            }
            S::Expr(e) => TypedStmtKind::Expr(self.expr(e, None)?),
            S::Block(b) => {
                let (b, f) = self.block(b, true)?;
                flow = f;
                TypedStmtKind::Block(b)
            }
            S::Unsafe(b) => {
                self.unsafe_depth += 1;
                let result = self.block(b, true);
                self.unsafe_depth -= 1;
                let (b, f) = result?;
                flow = f;
                TypedStmtKind::Block(b)
            }
            S::If(cond, yes, no) => {
                let cond = self.expr(cond, Some(&Type::Bool))?;
                let (yes, yf) = self.block(yes, true)?;
                let (no, nf) = match no {
                    Some(b) => {
                        let (b, f) = self.block(b, true)?;
                        (Some(b), f)
                    }
                    None => (None, FALL),
                };
                flow = yf | nf;
                TypedStmtKind::If(cond, yes, no)
            }
            S::While(cond, b) => {
                let cond = self.expr(cond, Some(&Type::Bool))?;
                self.loop_depth += 1;
                let result = self.block(b, true);
                self.loop_depth -= 1;
                let (b, bf) = result?;
                let always = matches!(cond.kind, TypedExprKind::Bool(true));
                flow = (bf & RETURN) | if !always || bf & BREAK != 0 { FALL } else { 0 };
                TypedStmtKind::While(cond, b)
            }
            S::Return(value) => {
                let ty = self
                    .result
                    .clone()
                    .ok_or_else(|| error("E_RETURN", s.span, "أرجع خارج الدالة"))?;
                let value = match (value, &ty) {
                    (None, Type::Void) => None,
                    (Some(_), Type::Void) => {
                        return Err(error("E_RETURN", s.span, "الدالة فراغ لا تعيد قيمة"))
                    }
                    (Some(v), _) => Some(self.expr(v, Some(&ty))?),
                    (None, _) if !self.annotated => Some(TypedExpr {
                        kind: TypedExprKind::Float(0.0),
                        ty,
                        span: s.span,
                    }),
                    _ => return Err(error("E_RETURN", s.span, "متوقع قيمة الإرجاع")),
                };
                if let Some(value) = &value {
                    if !self.roots(value).is_empty() {
                        return Err(error(
                            "E_POINTER_ESCAPE",
                            value.span,
                            "لا يمكن إرجاع مؤشر يشير إلى متغير محلي",
                        ));
                    }
                }
                flow = RETURN;
                TypedStmtKind::Return(value)
            }
            S::Break => {
                if self.loop_depth == 0 {
                    return Err(error("E_PARSE", s.span, "توقف خارج الحلقة"));
                }
                flow = BREAK;
                TypedStmtKind::Break
            }
            S::Continue => {
                if self.loop_depth == 0 {
                    return Err(error("E_PARSE", s.span, "تابع خارج الحلقة"));
                }
                flow = CONTINUE;
                TypedStmtKind::Continue
            }
            S::Function(_) | S::ExternFunction(_) | S::ExportFunction { .. } => {
                return Err(error(
                    "E_UNSUPPORTED",
                    s.span,
                    "التصريحات المتداخلة غير مدعومة",
                ))
            }
            S::Import(_) => {
                return Err(error(
                    "E_IMPORT",
                    s.span,
                    "يجب تحميل الاستيراد قبل فحص الأنواع",
                ))
            }
            _ => {
                return Err(error(
                    "E_UNSUPPORTED",
                    s.span,
                    "هذا التركيب غير مدعوم في الواجهة العددية المحددة",
                ))
            }
        };
        Ok((TypedStmt { kind, span: s.span }, flow))
    }
    fn literal(
        &self,
        text: &str,
        negative: bool,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<TypedExpr> {
        let ty = expected
            .filter(|t| t.is_numeric())
            .cloned()
            .unwrap_or(Type::Float { bits: 64 });
        let kind = match &ty {
            Type::Int { .. } => {
                if !text.chars().all(|c| c.is_ascii_digit()) {
                    return Err(error(
                        "E_TYPE_MISMATCH",
                        span,
                        "العدد الصحيح يتطلب أرقامًا عشرية دون فاصلة أو أس",
                    ));
                }
                let magnitude = text
                    .parse::<i128>()
                    .map_err(|_| error("E_TYPE_MISMATCH", span, "عدد صحيح خارج النطاق"))?;
                let n = if negative { -magnitude } else { magnitude };
                let (min, max) = ty.int_bounds().unwrap();
                if n < min || n > max {
                    return Err(error(
                        "E_TYPE_MISMATCH",
                        span,
                        format!("{n} خارج نطاق {ty}"),
                    ));
                }
                TypedExprKind::Int(n)
            }
            Type::Float { bits } => {
                let n = text
                    .parse::<f64>()
                    .map_err(|_| error("E_TYPE_MISMATCH", span, "عدد غير صحيح"))?;
                let n = if negative { -n } else { n };
                let n = if *bits == 32 { (n as f32) as f64 } else { n };
                if !n.is_finite() {
                    return Err(error("E_TYPE_MISMATCH", span, "عدد عشري خارج النطاق"));
                }
                TypedExprKind::Float(n)
            }
            _ => unreachable!(),
        };
        Ok(TypedExpr { kind, ty, span })
    }
    /// Find a nonliteral operand's type to contextualize literals symmetrically.
    fn hint(&self, e: &syntax::Expr) -> Option<Type> {
        match &e.kind {
            E::Variable(n) => self.local(n, e.span).ok().map(|l| l.ty),
            E::Call(n, _) => self.decls.get(n).map(|d| d.signature.result.clone()),
            E::Cast(t, _) => Some(t.kind.clone()),
            E::Bool(_) => Some(Type::Bool),
            E::Text(_) => Some(Type::Text),
            E::Binary(_, op, _)
                if matches!(
                    op,
                    Op::Equal
                        | Op::NotEqual
                        | Op::Less
                        | Op::LessEqual
                        | Op::Greater
                        | Op::GreaterEqual
                        | Op::And
                        | Op::Or
                ) =>
            {
                Some(Type::Bool)
            }
            E::Binary(a, _, b) => self.hint(a).or_else(|| self.hint(b)),
            E::Unary(Op::Not, _) => Some(Type::Bool),
            E::Unary(_, a) => self.hint(a),
            E::Assign(n, _) => self.local(n, e.span).ok().map(|l| l.ty),
            E::NullPointer(t) => Some(Type::Pointer(Box::new(t.kind.clone()))),
            _ => None,
        }
    }
    fn expr(&mut self, e: &syntax::Expr, expected: Option<&Type>) -> Result<TypedExpr> {
        let span = e.span;
        let ty;
        let kind = match &e.kind {
            E::Number(n) => {
                let result = self.literal(&n.text, false, expected, span)?;
                ty = result.ty;
                result.kind
            }
            E::Unary(Op::Negate, a) if matches!(&a.kind, E::Number(_)) => {
                let n = match &a.kind {
                    E::Number(n) => n,
                    _ => unreachable!(),
                };
                let result = self.literal(&n.text, true, expected, span)?;
                ty = result.ty;
                result.kind
            }
            E::Bool(b) => {
                ty = Type::Bool;
                TypedExprKind::Bool(*b)
            }
            E::Text(s) => {
                ty = Type::Text;
                TypedExprKind::Text(s.clone())
            }
            E::Variable(n) => {
                let l = self.local(n, span)?;
                ty = l.ty;
                TypedExprKind::Variable(l.id)
            }
            E::Assign(n, v) => {
                let l = self.local(n, span)?;
                if matches!(l.ty, Type::Pointer(_)) {
                    return Err(error(
                        "E_POINTER_IMMUTABLE",
                        span,
                        "ربط المؤشر ثابت؛ لا يمكن إعادة إسناده",
                    ));
                }
                let v = self.expr(v, Some(&l.ty))?;
                ty = l.ty;
                TypedExprKind::Assign(l.id, Box::new(v))
            }
            E::Unary(op, a) => {
                let a = self.expr(
                    a,
                    if matches!(op, Op::Not) {
                        Some(&Type::Bool)
                    } else {
                        expected
                    },
                )?;
                if matches!(op, Op::Negate)
                    && (!a.ty.is_numeric() || matches!(a.ty, Type::Int { signed: false, .. }))
                {
                    return Err(error(
                        "E_TYPE_MISMATCH",
                        span,
                        "النفي العددي يتطلب عددًا ذا إشارة",
                    ));
                }
                ty = a.ty.clone();
                TypedExprKind::Unary(*op, Box::new(a))
            }
            E::Binary(a, op, b) => {
                let logical = matches!(op, Op::And | Op::Or);
                let compare = matches!(
                    op,
                    Op::Equal
                        | Op::NotEqual
                        | Op::Less
                        | Op::LessEqual
                        | Op::Greater
                        | Op::GreaterEqual
                );
                let context = if logical {
                    Some(Type::Bool)
                } else {
                    expected
                        .filter(|t| !compare && t.is_numeric())
                        .cloned()
                        .or_else(|| self.hint(a))
                        .or_else(|| self.hint(b))
                };
                let a = self.expr(a, context.as_ref())?;
                let b = self.expr(b, Some(&a.ty))?;
                if a.ty == Type::Void {
                    return Err(error("E_TYPE_MISMATCH", span, "فراغ ليس قيمة"));
                }
                if !logical {
                    let equality = matches!(op, Op::Equal | Op::NotEqual);
                    if !a.ty.is_numeric()
                        && !(equality && matches!(a.ty, Type::Bool | Type::Pointer(_)))
                    {
                        return Err(error("E_TYPE_MISMATCH", span, "المعامل لا يدعم هذا النوع"));
                    }
                    if matches!(op, Op::Power) && a.ty != (Type::Float { bits: 64 }) {
                        return Err(error("E_TYPE_MISMATCH", span, "الأس يتطلب ع64"));
                    }
                }
                ty = if compare || logical {
                    Type::Bool
                } else {
                    a.ty.clone()
                };
                TypedExprKind::Binary(Box::new(a), *op, Box::new(b))
            }
            E::Call(name, args) => {
                let d = self.decls.get(name).cloned().ok_or_else(|| {
                    error("E_TYPE_MISMATCH", span, format!("دالة غير معرفة: {name}"))
                })?;
                if d.foreign && self.unsafe_depth == 0 {
                    return Err(error("E_UNSAFE", span, "استدعاء C يتطلب كتلة غير_آمن"));
                }
                if d.signature.params.len() != args.len() {
                    return Err(error(
                        "E_TYPE_MISMATCH",
                        span,
                        "عدد الوسائط لا يطابق توقيع الدالة",
                    ));
                }
                let args = args
                    .iter()
                    .zip(&d.signature.params)
                    .map(|(a, t)| self.expr(a, Some(t)))
                    .collect::<Result<Vec<_>>>()?;
                ty = d.signature.result;
                TypedExprKind::Call { callee: d.id, args }
            }
            E::Cast(t, v) => {
                if !t.kind.is_numeric() {
                    return Err(error(
                        "E_TYPE_MISMATCH",
                        span,
                        "التحويل الصريح متاح للأنواع العددية فقط",
                    ));
                }
                // Integer literals carry unlimited source precision into an explicit integer cast.
                let direct_integer = matches!(&v.kind,E::Number(n) if n.text.chars().all(|c|c.is_ascii_digit()))
                    || matches!(&v.kind,E::Unary(Op::Negate,a) if matches!(&a.kind,E::Number(n) if n.text.chars().all(|c|c.is_ascii_digit())));
                let v = self.expr(
                    v,
                    if direct_integer && matches!(t.kind, Type::Int { .. }) {
                        Some(&t.kind)
                    } else {
                        None
                    },
                )?;
                if !v.ty.is_numeric() {
                    return Err(error("E_TYPE_MISMATCH", span, "التحويل يتطلب قيمة عددية"));
                }
                self.check_constant_cast(&v, &t.kind, span)?;
                ty = t.kind.clone();
                TypedExprKind::Cast(Box::new(v))
            }
            E::NullPointer(t) => {
                ty = Type::Pointer(Box::new(t.kind.clone()));
                TypedExprKind::NullPointer
            }
            E::Address(value) => {
                let name = match &value.kind {
                    E::Variable(name) => name,
                    _ => {
                        return Err(error(
                            "E_TYPE_MISMATCH",
                            value.span,
                            "عنوان يتطلب اسم متغير محلي عددي أو منطقي",
                        ))
                    }
                };
                let local = self.local(name, value.span)?;
                if !scalar_pointee(&local.ty) {
                    return Err(error(
                        "E_UNSUPPORTED",
                        value.span,
                        "عنوان يدعم المتغيرات العددية والمنطقية فقط",
                    ));
                }
                ty = Type::Pointer(Box::new(local.ty));
                TypedExprKind::Address(local.id)
            }
            E::Load(pointer) => {
                if self.unsafe_depth == 0 {
                    return Err(error("E_UNSAFE", span, "تحميل يتطلب كتلة غير_آمن"));
                }
                let pointer = self.expr(pointer, None)?;
                ty = self.pointee(&pointer)?;
                TypedExprKind::Load(Box::new(pointer))
            }
            E::Store(pointer, value) => {
                if self.unsafe_depth == 0 {
                    return Err(error("E_UNSAFE", span, "خزن يتطلب كتلة غير_آمن"));
                }
                let pointer = self.expr(pointer, None)?;
                let pointee = self.pointee(&pointer)?;
                let value = self.expr(value, Some(&pointee))?;
                ty = Type::Void;
                TypedExprKind::Store {
                    pointer: Box::new(pointer),
                    value: Box::new(value),
                }
            }
            _ => {
                return Err(error(
                    "E_UNSUPPORTED",
                    span,
                    "هذا التعبير غير مدعوم في الواجهة العددية المحددة",
                ))
            }
        };
        if let Some(wanted) = expected {
            if &ty != wanted {
                return Err(mismatch(span, wanted, &ty));
            }
        }
        Ok(TypedExpr { kind, ty, span })
    }
    fn pointee(&self, pointer: &TypedExpr) -> Result<Type> {
        match &pointer.ty {
            Type::Pointer(pointee) if scalar_pointee(pointee) => Ok((**pointee).clone()),
            Type::Pointer(_) => Err(error(
                "E_UNSUPPORTED",
                pointer.span,
                "تحميل وخزن يدعمان مؤشرات الأعداد والمنطقي فقط",
            )),
            _ => Err(error("E_TYPE_MISMATCH", pointer.span, "متوقع مؤشر")),
        }
    }
    fn roots(&self, value: &TypedExpr) -> HashSet<LocalId> {
        if !matches!(value.ty, Type::Pointer(_)) {
            return HashSet::new();
        }
        match &value.kind {
            TypedExprKind::Address(id) => std::iter::once(*id).collect(),
            TypedExprKind::Variable(id) => self.pointer_roots.get(id).cloned().unwrap_or_default(),
            // A pointer result may alias any pointer argument, regardless of ABI.
            TypedExprKind::Call { args, .. } => {
                args.iter().flat_map(|arg| self.roots(arg)).collect()
            }
            _ => HashSet::new(),
        }
    }
    fn check_constant_cast(&self, v: &TypedExpr, to: &Type, span: Span) -> Result<()> {
        if let Some((min, max)) = to.int_bounds() {
            let fits = match constant_number(v) {
                Some(TypedExprKind::Int(n)) => n >= min && n <= max,
                Some(TypedExprKind::Float(n)) => {
                    let n = n.trunc();
                    n.is_finite() && n >= min as f64 && n < (max + 1) as f64
                }
                _ => true,
            };
            if !fits {
                return Err(error(
                    "E_TYPE_MISMATCH",
                    span,
                    "التحويل الثابت خارج نطاق النوع",
                ));
            }
        } else if *to == (Type::Float { bits: 32 }) {
            if let Some(TypedExprKind::Float(n)) = constant_number(v) {
                if !(n as f32).is_finite() {
                    return Err(error("E_TYPE_MISMATCH", span, "التحويل خارج نطاق ع32"));
                }
            }
        }
        Ok(())
    }
}

/// Evaluate scalar constants for diagnostics without rewriting checked runtime IR.
/// Integer arithmetic stays in i128 so all u64 values retain their precision.
fn constant_number(v: &TypedExpr) -> Option<TypedExprKind> {
    use TypedExprKind as K;
    let value = match &v.kind {
        K::Int(n) => K::Int(*n),
        K::Float(n) => K::Float(*n),
        K::Unary(Op::Negate, a) => match constant_number(a)? {
            K::Int(n) => K::Int(n.checked_neg()?),
            K::Float(n) => K::Float(-n),
            _ => return None,
        },
        K::Binary(a, op, b) => match (constant_number(a)?, constant_number(b)?) {
            (K::Int(a), K::Int(b)) => K::Int(match op {
                Op::Add => a.checked_add(b)?,
                Op::Subtract => a.checked_sub(b)?,
                Op::Multiply => a.checked_mul(b)?,
                Op::Divide => a.checked_div(b)?,
                Op::Modulo => a.checked_rem(b)?,
                _ => return None,
            }),
            (K::Float(a), K::Float(b)) => K::Float(match op {
                Op::Add => a + b,
                Op::Subtract => a - b,
                Op::Multiply => a * b,
                Op::Divide => a / b,
                Op::Modulo => a % b,
                Op::Power => a.powf(b),
                _ => return None,
            }),
            _ => return None,
        },
        K::Cast(a) => match (constant_number(a)?, &v.ty) {
            (K::Int(n), Type::Int { .. }) => K::Int(n),
            (K::Float(n), Type::Int { .. }) => K::Int(n.trunc() as i128),
            (K::Int(n), Type::Float { bits: 32 }) => K::Float((n as f32) as f64),
            (K::Int(n), Type::Float { .. }) => K::Float(n as f64),
            (K::Float(n), Type::Float { .. }) => K::Float(n),
            _ => return None,
        },
        _ => return None,
    };
    // Round at each f32 operation, just as the target does.
    Some(match (value, &v.ty) {
        (K::Float(n), Type::Float { bits: 32 }) => K::Float((n as f32) as f64),
        (value, _) => value,
    })
}

fn scalar_pointee(ty: &Type) -> bool {
    ty.is_numeric() || *ty == Type::Bool
}
