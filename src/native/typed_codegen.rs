//! LLVM lowering for the checked scalar IR. Every potentially poisonous numeric
//! instruction is dominated by its range guard. Only the selected branch of a
//! short-circuit expression executes. The emitted program has no interpreter.
use crate::{
    ast::Op,
    diagnostic::Diagnostic,
    source::Span,
    typed::*,
    types::{Abi, FunctionSig, Type},
};
use std::collections::{BTreeSet, HashMap, HashSet};

type Result<T> = std::result::Result<T, Diagnostic>;
fn error(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new("E_NATIVE_TYPED", span, message)
}
fn llvm(t: &Type) -> Result<String> {
    Ok(match t {
        Type::Int { bits, .. } if [8, 16, 32, 64].contains(bits) => format!("i{bits}"),
        Type::Float { bits: 32 } => "float".into(),
        Type::Float { bits: 64 } => "double".into(),
        Type::Bool => "i1".into(),
        Type::Void => "void".into(),
        Type::Pointer(_) => "ptr".into(),
        _ => {
            return Err(error(
                Span::default(),
                format!("النوع {t} غير مدعوم في المترجم العددي"),
            ))
        }
    })
}
// Foreign non-null pointers must name valid scalar storage; address validity and
// lifetime remain the unsafe caller's responsibility. Unaligned storage is valid.
fn scalar_pointee(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int {
            bits: 8 | 16 | 32 | 64,
            ..
        } | Type::Float { bits: 32 | 64 }
            | Type::Bool
    )
}
fn checked_pointee(ty: &Type, span: Span) -> Result<&Type> {
    match ty {
        Type::Pointer(inner) if scalar_pointee(inner) => Ok(inner),
        _ => Err(error(span, "المؤشر يتطلب قيمة عددية أو منطقية")),
    }
}
fn symbol(s: &str) -> String {
    format!(
        "@\"{}\"",
        s.as_bytes()
            .iter()
            .map(|b| if b.is_ascii_alphanumeric() || b"_.$".contains(b) {
                (*b as char).to_string()
            } else {
                format!("\\{b:02X}")
            })
            .collect::<String>()
    )
}
// Clang x86_64 C ABI: SysV extends narrow integers; MSVC does not. Both
// ABIs represent C _Bool as zeroext i1. LLVM types do not encode signedness.
fn extension(t: &Type, abi: Abi) -> &'static str {
    if abi != Abi::C {
        return "";
    }
    match t {
        Type::Bool => "zeroext ",
        Type::Int {
            signed: true,
            bits: 8 | 16,
        } if !cfg!(windows) => "signext ",
        Type::Int {
            signed: false,
            bits: 8 | 16,
        } if !cfg!(windows) => "zeroext ",
        _ => "",
    }
}
fn result_type(sig: &FunctionSig) -> Result<String> {
    Ok(format!(
        "{}{}",
        extension(&sig.result, sig.abi),
        llvm(&sig.result)?
    ))
}
fn parameter(t: &Type, abi: Abi, value: &str) -> Result<String> {
    Ok(format!("{} {}{value}", llvm(t)?, extension(t, abi)))
}
fn float_constant(v: f64, bits: u8) -> String {
    let v = if bits == 32 { v as f32 as f64 } else { v };
    format!("0x{:016X}", v.to_bits())
}
#[derive(Clone)]
struct Callee {
    symbol: String,
    signature: FunctionSig,
}
struct Module {
    callees: HashMap<FunctionId, Callee>,
    globals: Vec<String>,
    intrinsics: BTreeSet<String>,
}
impl Module {
    fn text(&mut self, text: &str) -> String {
        let id = self.globals.len();
        let bytes = text.as_bytes();
        self.globals.push(format!(
            "@.lisan.text.{id} = private unnamed_addr constant [{} x i8] c\"{}\\00\"",
            bytes.len() + 1,
            bytes
                .iter()
                .map(|b| format!("\\{b:02X}"))
                .collect::<String>()
        ));
        format!("@.lisan.text.{id}")
    }
}
struct Writer<'a> {
    module: &'a mut Module,
    lines: Vec<String>,
    allocas: Vec<String>,
    locals: HashMap<LocalId, (Type, String)>,
    loops: Vec<(String, String)>,
    next: usize,
    terminated: bool,
    result: Type,
    main: bool,
}
impl<'a> Writer<'a> {
    fn new(module: &'a mut Module, result: Type, main: bool) -> Self {
        Self {
            module,
            lines: vec!["entry:".into()],
            allocas: vec![],
            locals: HashMap::new(),
            loops: vec![],
            next: 0,
            terminated: false,
            result,
            main,
        }
    }
    fn temp(&mut self) -> String {
        let n = self.next;
        self.next += 1;
        format!("%v{n}")
    }
    fn block(&mut self) -> String {
        let n = self.next;
        self.next += 1;
        format!("b{n}")
    }
    fn emit(&mut self, s: impl Into<String>) {
        self.lines.push(format!("  {}", s.into()));
    }
    fn value(&mut self, s: impl Into<String>) -> String {
        let r = self.temp();
        self.emit(format!("{r} = {}", s.into()));
        r
    }
    fn label(&mut self, l: &str) {
        self.lines.push(format!("{l}:"));
        self.terminated = false;
    }
    fn jump(&mut self, l: &str) {
        self.emit(format!("br label %{l}"));
        self.terminated = true;
    }
    fn guard(&mut self, condition: &str) {
        let ok = self.block();
        let fail = self.block();
        self.emit(format!("br i1 {condition}, label %{ok}, label %{fail}"));
        self.label(&fail);
        let message = self
            .module
            .text("خطأ: تجاوز عددي أو قسمة على صفر أو تحويل خارج النطاق");
        self.emit(format!("call i32 @puts(ptr {message})"));
        self.emit("call void @exit(i32 1)");
        self.emit("unreachable");
        self.label(&ok);
    }
    fn finite(&mut self, v: &str, bits: u8) {
        let t = if bits == 32 { "float" } else { "double" };
        let name = format!("llvm.fabs.f{bits}");
        self.module
            .intrinsics
            .insert(format!("declare {t} @{name}({t})"));
        let abs = self.value(format!("call {t} @{name}({t} {v})"));
        let max = if bits == 32 {
            f32::MAX as f64
        } else {
            f64::MAX
        };
        let valid = self.value(format!("fcmp ole {t} {abs}, {}", float_constant(max, bits)));
        self.guard(&valid);
    }
    fn bind(&mut self, id: LocalId, t: &Type, v: &str) -> Result<()> {
        let ty = llvm(t)?;
        let p = self.temp();
        self.allocas.push(format!("  {p} = alloca {ty}"));
        self.emit(format!("store {ty} {v}, ptr {p}"));
        self.locals.insert(id, (t.clone(), p));
        Ok(())
    }
    fn expr(&mut self, e: &TypedExpr) -> Result<String> {
        let t = if e.ty == Type::Text {
            String::new()
        } else {
            llvm(&e.ty).map_err(|mut d| {
                d.primary = e.span;
                d
            })?
        };
        Ok(match &e.kind {
            TypedExprKind::Int(v) => {
                if !e
                    .ty
                    .int_bounds()
                    .is_some_and(|(lo, hi)| *v >= lo && *v <= hi)
                {
                    return Err(error(e.span, "ثابت صحيح خارج النطاق"));
                }
                v.to_string()
            }
            TypedExprKind::Float(v) => {
                let Type::Float { bits } = e.ty else {
                    return Err(error(e.span, "نوع ثابت عشري غير صالح"));
                };
                if !v.is_finite() || (bits == 32 && !(*v as f32).is_finite()) {
                    return Err(error(e.span, "ثابت عشري غير منته"));
                }
                float_constant(*v, bits)
            }
            TypedExprKind::Bool(v) => if *v { "true" } else { "false" }.into(),
            TypedExprKind::NullPointer => "null".into(),
            TypedExprKind::Address(id) => {
                let (ty, slot) = self
                    .locals
                    .get(id)
                    .ok_or_else(|| error(e.span, "متغير عنوان غير معروف"))?;
                if !scalar_pointee(ty) || e.ty != Type::Pointer(Box::new(ty.clone())) {
                    return Err(error(e.span, "عنوان محلي من نوع عددي غير صالح"));
                }
                slot.clone()
            }
            TypedExprKind::Load(pointer) => {
                let pointee = checked_pointee(&pointer.ty, e.span)?;
                if e.ty != *pointee {
                    return Err(error(e.span, "نوع تحميل المؤشر غير مطابق"));
                }
                let p = self.expr(pointer)?;
                let valid = self.value(format!("icmp ne ptr {p}, null"));
                self.guard(&valid);
                let value = self.value(format!("load {t}, ptr {p}, align 1"));
                if let Type::Float { bits } = e.ty {
                    self.finite(&value, bits);
                }
                value
            }
            TypedExprKind::Store { pointer, value } => {
                let pointee = checked_pointee(&pointer.ty, e.span)?;
                if value.ty != *pointee || e.ty != Type::Void {
                    return Err(error(e.span, "نوع تخزين المؤشر غير مطابق"));
                }
                let p = self.expr(pointer)?;
                let v = self.expr(value)?;
                let valid = self.value(format!("icmp ne ptr {p}, null"));
                self.guard(&valid);
                self.emit(format!("store {} {v}, ptr {p}, align 1", llvm(pointee)?));
                String::new()
            }

            TypedExprKind::Text(_) => {
                return Err(error(
                    e.span,
                    "النصوص الديناميكية غير مدعومة؛ يمكن طباعة نص ثابت",
                ))
            }
            TypedExprKind::Variable(id) => {
                let (_, p) = self
                    .locals
                    .get(id)
                    .ok_or_else(|| error(e.span, "متغير غير معروف"))?
                    .clone();
                let value = self.value(format!("load {t}, ptr {p}"));
                if let Type::Float { bits } = e.ty {
                    self.finite(&value, bits);
                }
                value
            }
            TypedExprKind::Assign(id, rhs) => {
                let v = self.expr(rhs)?;
                let (_, p) = self
                    .locals
                    .get(id)
                    .ok_or_else(|| error(e.span, "متغير غير معروف"))?
                    .clone();
                self.emit(format!("store {t} {v}, ptr {p}"));
                v
            }
            TypedExprKind::Unary(op, a) => {
                let v = self.expr(a)?;
                match op {
                    Op::Not => self.value(format!("xor i1 {v}, true")),
                    Op::Negate => match e.ty {
                        Type::Int { signed, bits } => {
                            self.integer(Op::Subtract, "0", &v, signed, bits, e.span)?
                        }
                        Type::Float { bits } => {
                            let r = self.value(format!("fneg {t} {v}"));
                            self.finite(&r, bits);
                            r
                        }
                        _ => return Err(error(e.span, "معامل أحادي غير مدعوم")),
                    },
                    _ => return Err(error(e.span, "معامل أحادي غير مدعوم")),
                }
            }
            TypedExprKind::Binary(a, op, b) if matches!(op, Op::And | Op::Or) => {
                let av = self.expr(a)?;
                let rhs = self.block();
                let short = self.block();
                let done = self.block();
                let slot = self.temp();
                self.allocas.push(format!("  {slot} = alloca i1"));
                let (yes, no) = if matches!(op, Op::And) {
                    (&rhs, &short)
                } else {
                    (&short, &rhs)
                };
                self.emit(format!("br i1 {av}, label %{yes}, label %{no}"));
                self.label(&short);
                self.emit(format!(
                    "store i1 {}, ptr {slot}",
                    if matches!(op, Op::Or) {
                        "true"
                    } else {
                        "false"
                    }
                ));
                self.jump(&done);
                self.label(&rhs);
                let bv = self.expr(b)?;
                self.emit(format!("store i1 {bv}, ptr {slot}"));
                self.jump(&done);
                self.label(&done);
                self.value(format!("load i1, ptr {slot}"))
            }
            TypedExprKind::Binary(a, op, b) => {
                let av = self.expr(a)?;
                let bv = self.expr(b)?;
                self.binary(*op, &av, &bv, &a.ty, e.span)?
            }
            TypedExprKind::Cast(a) => {
                let v = self.expr(a)?;
                self.cast(&v, &a.ty, &e.ty, e.span)?
            }
            TypedExprKind::Call { callee, args } => {
                let c = self
                    .module
                    .callees
                    .get(callee)
                    .cloned()
                    .ok_or_else(|| error(e.span, "دالة غير معروفة"))?;
                if args.len() != c.signature.params.len() {
                    return Err(error(e.span, "عدد وسائط غير صالح"));
                }
                let mut values = vec![];
                for (a, ty) in args.iter().zip(&c.signature.params) {
                    let v = self.expr(a)?;
                    values.push(parameter(ty, c.signature.abi, &v)?);
                }
                let call = format!(
                    "call {} {}({})",
                    result_type(&c.signature)?,
                    symbol(&c.symbol),
                    values.join(", ")
                );
                let r = if e.ty == Type::Void {
                    self.emit(call);
                    String::new()
                } else {
                    self.value(call)
                };
                if let Type::Float { bits } = e.ty {
                    self.finite(&r, bits);
                }
                r
            }
        })
    }
    fn integer(
        &mut self,
        op: Op,
        a: &str,
        b: &str,
        signed: bool,
        bits: u8,
        span: Span,
    ) -> Result<String> {
        let t = format!("i{bits}");
        let prefix = if signed { "s" } else { "u" };
        match op {
            Op::Add | Op::Subtract | Op::Multiply => {
                let name = match op {
                    Op::Add => "add",
                    Op::Subtract => "sub",
                    _ => "mul",
                };
                let intrinsic = format!("llvm.{prefix}{name}.with.overflow.i{bits}");
                self.module
                    .intrinsics
                    .insert(format!("declare {{ {t}, i1 }} @{intrinsic}({t}, {t})"));
                let pair = self.value(format!("call {{ {t}, i1 }} @{intrinsic}({t} {a}, {t} {b})"));
                let overflow = self.value(format!("extractvalue {{ {t}, i1 }} {pair}, 1"));
                let valid = self.value(format!("xor i1 {overflow}, true"));
                self.guard(&valid);
                Ok(self.value(format!("extractvalue {{ {t}, i1 }} {pair}, 0")))
            }
            Op::Divide | Op::Modulo => {
                let mut valid = self.value(format!("icmp ne {t} {b}, 0"));
                if signed {
                    let min = -(1i128 << (bits - 1));
                    let amin = self.value(format!("icmp eq {t} {a}, {min}"));
                    let bneg = self.value(format!("icmp eq {t} {b}, -1"));
                    let bad = self.value(format!("and i1 {amin}, {bneg}"));
                    let good = self.value(format!("xor i1 {bad}, true"));
                    valid = self.value(format!("and i1 {valid}, {good}"));
                }
                self.guard(&valid);
                Ok(self.value(format!(
                    "{prefix}{} {t} {a}, {b}",
                    if matches!(op, Op::Divide) {
                        "div"
                    } else {
                        "rem"
                    }
                )))
            }
            _ => Err(error(span, "عملية صحيحة غير مدعومة")),
        }
    }
    fn binary(&mut self, op: Op, a: &str, b: &str, ty: &Type, span: Span) -> Result<String> {
        let t = llvm(ty)?;
        if matches!(
            op,
            Op::Equal | Op::NotEqual | Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual
        ) {
            let relation = match op {
                Op::Equal => "eq",
                Op::NotEqual => "ne",
                Op::Less => "lt",
                Op::LessEqual => "le",
                Op::Greater => "gt",
                _ => "ge",
            };
            let opcode = match ty {
                Type::Float { .. } => format!("fcmp o{relation}"),
                Type::Int { signed, .. } if !matches!(op, Op::Equal | Op::NotEqual) => {
                    format!("icmp {}{relation}", if *signed { "s" } else { "u" })
                }
                _ => format!("icmp {relation}"),
            };
            return Ok(self.value(format!("{opcode} {t} {a}, {b}")));
        }
        match ty {
            Type::Int { signed, bits } => self.integer(op, a, b, *signed, *bits, span),
            Type::Float { bits } => {
                let opcode = match op {
                    Op::Add => "fadd",
                    Op::Subtract => "fsub",
                    Op::Multiply => "fmul",
                    Op::Divide => "fdiv",
                    Op::Modulo => "frem",
                    Op::Power => {
                        let name = format!("llvm.pow.f{bits}");
                        self.module
                            .intrinsics
                            .insert(format!("declare {t} @{name}({t}, {t})"));
                        let r = self.value(format!("call {t} @{name}({t} {a}, {t} {b})"));
                        self.finite(&r, *bits);
                        return Ok(r);
                    }
                    _ => return Err(error(span, "عملية عشرية غير مدعومة")),
                };
                let r = self.value(format!("{opcode} {t} {a}, {b}"));
                self.finite(&r, *bits);
                Ok(r)
            }
            _ => Err(error(span, "عملية غير مدعومة على هذا النوع")),
        }
    }
    fn cast(&mut self, v: &str, from: &Type, to: &Type, span: Span) -> Result<String> {
        if from == to {
            return Ok(v.into());
        }
        let a = llvm(from)?;
        let b = llvm(to)?;
        match (from, to) {
            (
                Type::Int {
                    signed: fs,
                    bits: fb,
                },
                Type::Int {
                    signed: ts,
                    bits: tb,
                },
            ) => {
                let wide = self.value(format!(
                    "{} {a} {v} to i128",
                    if *fs { "sext" } else { "zext" }
                ));
                let (lo, hi) = to.int_bounds().unwrap();
                let low = self.value(format!("icmp sge i128 {wide}, {lo}"));
                let high = self.value(format!("icmp sle i128 {wide}, {hi}"));
                let valid = self.value(format!("and i1 {low}, {high}"));
                self.guard(&valid);
                if fb == tb {
                    Ok(v.into())
                } else {
                    let opcode = if tb < fb {
                        "trunc"
                    } else if *fs {
                        "sext"
                    } else {
                        "zext"
                    };
                    let _ = ts;
                    Ok(self.value(format!("{opcode} {a} {v} to {b}")))
                }
            }
            (Type::Int { signed, .. }, Type::Float { bits }) => {
                let r = self.value(format!(
                    "{} {a} {v} to {b}",
                    if *signed { "sitofp" } else { "uitofp" }
                ));
                self.finite(&r, *bits);
                Ok(r)
            }
            (Type::Float { bits: fb }, Type::Float { bits: tb }) => {
                let r = self.value(format!(
                    "{} {a} {v} to {b}",
                    if tb < fb { "fptrunc" } else { "fpext" }
                ));
                self.finite(&r, *tb);
                Ok(r)
            }
            (Type::Float { bits }, Type::Int { signed, bits: ib }) => {
                self.finite(v, *bits);
                let name = format!("llvm.trunc.f{bits}");
                self.module
                    .intrinsics
                    .insert(format!("declare {a} @{name}({a})"));
                let truncated = self.value(format!("call {a} @{name}({a} {v})"));
                let v = truncated.as_str();
                let pow = 2f64.powi(if *signed { *ib as i32 - 1 } else { *ib as i32 });
                let low = if *signed { -pow } else { 0. };
                let lower = self.value(format!("fcmp oge {a} {v}, {}", float_constant(low, *bits)));
                let upper = self.value(format!("fcmp olt {a} {v}, {}", float_constant(pow, *bits)));
                let valid = self.value(format!("and i1 {lower}, {upper}"));
                self.guard(&valid);
                Ok(self.value(format!(
                    "{} {a} {v} to {b}",
                    if *signed { "fptosi" } else { "fptoui" }
                )))
            }
            _ => Err(error(span, "تحويل غير مدعوم")),
        }
    }
    fn print(&mut self, e: &TypedExpr) -> Result<()> {
        if let TypedExprKind::Text(text) = &e.kind {
            if text.contains('\0') {
                return Err(error(
                    e.span,
                    "النص الثابت المطبوع يحتوي محرف NUL غير مدعوم",
                ));
            }
            let p = self.module.text(text);
            self.emit(format!("call i32 @puts(ptr {p})"));
            return Ok(());
        }
        let v = self.expr(e)?;
        match &e.ty {
            Type::Bool => {
                let yes = self.module.text("صحيح");
                let no = self.module.text("خطأ");
                let p = self.value(format!("select i1 {v}, ptr {yes}, ptr {no}"));
                self.emit(format!("call i32 @puts(ptr {p})"));
            }
            Type::Int { signed, bits } => {
                let v = if *bits < 64 {
                    self.value(format!(
                        "{} i{bits} {v} to i64",
                        if *signed { "sext" } else { "zext" }
                    ))
                } else {
                    v
                };
                let fmt = self.module.text(if *signed { "%lld\n" } else { "%llu\n" });
                self.emit(format!("call i32 (ptr, ...) @printf(ptr {fmt}, i64 {v})"));
            }
            Type::Float { bits } => {
                let v = if *bits == 32 {
                    self.value(format!("fpext float {v} to double"))
                } else {
                    v
                };
                let fmt = self
                    .module
                    .text(if *bits == 32 { "%.9g\n" } else { "%.17g\n" });
                self.emit(format!(
                    "call i32 (ptr, ...) @printf(ptr {fmt}, double {v})"
                ));
            }
            Type::Pointer(_) => {
                let fmt = self.module.text("%p\n");
                self.emit(format!("call i32 (ptr, ...) @printf(ptr {fmt}, ptr {v})"));
            }
            _ => return Err(error(e.span, "قيمة غير قابلة للطباعة في المترجم العددي")),
        }
        Ok(())
    }
    fn stmts(&mut self, stmts: &[TypedStmt]) -> Result<()> {
        for s in stmts {
            if self.terminated {
                break;
            }
            match &s.kind {
                TypedStmtKind::Let { id, ty, value, .. } => {
                    let v = self.expr(value)?;
                    self.bind(*id, ty, &v)?;
                }
                TypedStmtKind::Print(e) => self.print(e)?,
                TypedStmtKind::Expr(e) => {
                    self.expr(e)?;
                }
                TypedStmtKind::Block(body) => self.stmts(body)?,
                TypedStmtKind::If(cond, yes, no) => {
                    let v = self.expr(cond)?;
                    let y = self.block();
                    let n = self.block();
                    let end = self.block();
                    self.emit(format!("br i1 {v}, label %{y}, label %{n}"));
                    self.label(&y);
                    self.stmts(yes)?;
                    let y_done = self.terminated;
                    if !y_done {
                        self.jump(&end);
                    }
                    self.label(&n);
                    if let Some(no) = no {
                        self.stmts(no)?;
                    }
                    let n_done = self.terminated;
                    if !n_done {
                        self.jump(&end);
                    }
                    if y_done && n_done {
                        self.terminated = true;
                    } else {
                        self.label(&end);
                    }
                }
                TypedStmtKind::While(cond, body) => {
                    let test = self.block();
                    let run = self.block();
                    let end = self.block();
                    self.jump(&test);
                    self.label(&test);
                    let v = self.expr(cond)?;
                    self.emit(format!("br i1 {v}, label %{run}, label %{end}"));
                    self.label(&run);
                    self.loops.push((test.clone(), end.clone()));
                    self.stmts(body)?;
                    self.loops.pop();
                    if !self.terminated {
                        self.jump(&test);
                    }
                    self.label(&end);
                }
                TypedStmtKind::Break | TypedStmtKind::Continue => {
                    let (test, end) = self
                        .loops
                        .last()
                        .cloned()
                        .ok_or_else(|| error(s.span, "تحكم حلقة خارج الحلقة"))?;
                    self.jump(if matches!(s.kind, TypedStmtKind::Break) {
                        &end
                    } else {
                        &test
                    });
                }
                TypedStmtKind::Return(value) => {
                    if self.main {
                        return Err(error(s.span, "إرجاع خارج دالة"));
                    }
                    if let Some(value) = value {
                        let v = self.expr(value)?;
                        self.emit(format!("ret {} {v}", llvm(&self.result)?));
                    } else {
                        self.emit("ret void");
                    }
                    self.terminated = true;
                }
            }
        }
        Ok(())
    }
    fn finish(mut self) -> Result<String> {
        if !self.terminated {
            let zero = match self.result {
                Type::Void => {
                    self.emit("ret void");
                    None
                }
                Type::Pointer(_) => Some("null"),
                Type::Float { .. } => Some("0.0"),
                _ => Some("0"),
            };
            if let Some(z) = zero {
                self.emit(format!("ret {} {z}", llvm(&self.result)?));
            }
        }
        self.lines.splice(1..1, self.allocas);
        Ok(self.lines.join("\n"))
    }
}

/// Lower checked scalar IR for the host's x86-64 Linux or Windows C ABI.
/// Managed values and dynamic text deliberately produce source diagnostics.
pub fn generate(program: &TypedProgram) -> std::result::Result<String, Vec<Diagnostic>> {
    generate_inner(program, false).map_err(|e| vec![e])
}
/// Emit a host-linkable LLVM module without an entry point.
/// Library modules must have no top-level executable statements.
pub fn generate_library(program: &TypedProgram) -> std::result::Result<String, Vec<Diagnostic>> {
    generate_inner(program, true).map_err(|e| vec![e])
}
fn generate_inner(program: &TypedProgram, library: bool) -> Result<String> {
    if library && !program.main.is_empty() {
        return Err(error(
            program.main[0].span,
            "المكتبة لا تقبل تعليمات تنفيذية في المستوى الأعلى",
        ));
    }
    if !cfg!(target_arch = "x86_64") {
        return Err(error(
            Span::default(),
            "المترجم العددي يدعم المضيف x86-64 حاليا",
        ));
    }
    let mut m = Module {
        callees: HashMap::new(),
        globals: vec![],
        intrinsics: BTreeSet::new(),
    };
    let mut names: HashSet<String> = ["main", "printf", "puts", "exit"]
        .into_iter()
        .map(String::from)
        .collect();
    let definitions: HashSet<&str> = program
        .functions
        .iter()
        .map(|f| f.symbol.as_str())
        .collect();
    let mut aliases: HashMap<String, FunctionSig> = HashMap::new();
    for (id, name, sig, span) in program
        .functions
        .iter()
        .map(|f| (f.id, &f.symbol, &f.signature, f.span))
        .chain(
            program
                .externs
                .iter()
                .map(|f| (f.id, &f.symbol, &f.signature, f.span)),
        )
    {
        if name.starts_with("llvm.")
            || name.starts_with(".lisan.")
            || (!names.insert(name.clone())
                && (definitions.contains(name.as_str()) || aliases.get(name) != Some(sig)))
        {
            return Err(error(span, format!("رمز أصلي محجوز أو مكرر: {name}")));
        }
        if !definitions.contains(name.as_str()) {
            aliases.insert(name.clone(), sig.clone());
        }
        llvm(&sig.result)?;
        for p in &sig.params {
            llvm(p)?;
            if *p == Type::Void {
                return Err(error(span, "وسيط من نوع فراغ"));
            }
        }
        if m.callees
            .insert(
                id,
                Callee {
                    symbol: name.clone(),
                    signature: sig.clone(),
                },
            )
            .is_some()
        {
            return Err(error(span, "معرف دالة مكرر"));
        }
    }
    let mut functions = vec![];
    for f in &program.functions {
        if f.params.len() != f.signature.params.len() {
            return Err(error(f.span, "عدد معاملات الدالة غير صالح"));
        }
        let mut w = Writer::new(&mut m, f.signature.result.clone(), false);
        let mut params = vec![];
        for (i, (id, ty)) in f.params.iter().zip(&f.signature.params).enumerate() {
            let v = format!("%arg{i}");
            params.push(parameter(ty, f.signature.abi, &v)?);
            if f.signature.abi == Abi::C {
                if let Type::Float { bits } = ty {
                    w.finite(&v, *bits);
                }
            }
            w.bind(*id, ty, &v)?;
        }
        w.stmts(&f.body)?;
        functions.push(format!(
            "define {} {}({}) {{\n{}\n}}",
            result_type(&f.signature)?,
            symbol(&f.symbol),
            params.join(", "),
            w.finish()?
        ));
    }
    if !library {
        let mut w = Writer::new(
            &mut m,
            Type::Int {
                signed: true,
                bits: 32,
            },
            true,
        );
        w.stmts(&program.main)?;
        functions.push(format!("define i32 @main() {{\n{}\n}}", w.finish()?));
    }
    let mut out=String::from("; Lisan checked scalar LLVM backend\ndeclare i32 @puts(ptr)\ndeclare i32 @printf(ptr, ...)\ndeclare void @exit(i32) noreturn\n");
    let mut declared = HashSet::new();
    for f in &program.externs {
        if !declared.insert(&f.symbol) {
            continue;
        }
        let params = f
            .signature
            .params
            .iter()
            .map(|t| parameter(t, f.signature.abi, ""))
            .collect::<Result<Vec<_>>>()?;
        out.push_str(&format!(
            "declare {} {}({})\n",
            result_type(&f.signature)?,
            symbol(&f.symbol),
            params.join(", ")
        ));
    }
    for d in m.intrinsics {
        out.push_str(&format!("{d}\n"));
    }
    for g in m.globals {
        out.push_str(&format!("{g}\n"));
    }
    out.push_str(&functions.join("\n\n"));
    out.push('\n');
    Ok(out)
}
