//! LLVM lowering for checked scalar IR, hosted owned text and homogeneous lists.
//! Every potentially poisonous numeric
//! instruction is dominated by its range guard. Only the selected branch of a
//! short-circuit expression executes. The emitted program has no interpreter.
use crate::{
    ast::Op,
    diagnostic::Diagnostic,
    source::{SourceMap, Span},
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
        Type::Pointer(_) | Type::Text | Type::List(_) => "ptr".into(),
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
    if sig.result.is_managed() {
        return Ok("void".into());
    }
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
    sources: Option<SourceMap>,
    managed: bool,
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
    // Test label, exit label, and first lexical frame owned by this loop.
    loops: Vec<(String, String, usize)>,
    next: usize,
    terminated: bool,
    result: Type,
    main: bool,
    slots: Vec<String>,
    slot_types: HashMap<String, Type>,
    text_temps: Vec<String>,
    text_scopes: Vec<Vec<String>>,
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
            slots: vec![],
            slot_types: HashMap::new(),
            text_temps: vec![],
            text_scopes: vec![vec![]],
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
    fn guard(&mut self, condition: &str, span: Span) {
        let ok = self.block();
        let fail = self.block();
        self.emit(format!("br i1 {condition}, label %{ok}, label %{fail}"));
        self.label(&fail);
        if self.module.managed {
            self.emit("call void @.lisan.text.cleanup()");
        }
        let location = self
            .module
            .sources
            .as_ref()
            .map(|m| {
                let (line, col) = m.line_column(span);
                let path = m
                    .sources
                    .get(span.source.0 as usize)
                    .map(|s| s.path.display().to_string())
                    .unwrap_or_else(|| "<native>".into());
                format!("{path}:{line}:{col}")
            })
            .unwrap_or_else(|| "<native>:1:1".into());
        let message = self.module.text(&format!(
            "{location}: خطأ: تجاوز عددي أو قسمة على صفر أو تحويل خارج النطاق أو فشل عملية قيمة مدارة"
        ));
        self.emit(format!("call i32 @puts(ptr {message})"));
        self.emit("call void @exit(i32 1)");
        self.emit("unreachable");
        self.label(&ok);
    }
    fn finite(&mut self, v: &str, bits: u8, span: Span) {
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
        self.guard(&valid, span);
    }
    fn bind(&mut self, id: LocalId, t: &Type, v: &str, span: Span) -> Result<()> {
        if t.is_managed() {
            let p = self.managed_slot(t, false);
            self.clone_text(v, &p, span);
            self.text_scopes.last_mut().unwrap().push(p.clone());
            self.locals.insert(id, (t.clone(), p));
            return Ok(());
        }
        let ty = llvm(t)?;
        let p = self.temp();
        self.allocas.push(format!("  {p} = alloca {ty}"));
        self.emit(format!("store {ty} {v}, ptr {p}"));
        self.locals.insert(id, (t.clone(), p));
        Ok(())
    }
    fn text_slot(&mut self, temporary: bool) -> String {
        self.managed_slot(&Type::Text, temporary)
    }
    fn managed_slot(&mut self, ty: &Type, temporary: bool) -> String {
        let slot = self.temp();
        let layout = if *ty == Type::Text {
            "{i64, ptr}"
        } else {
            "ptr"
        };
        self.allocas.push(format!("  {slot} = alloca {layout}"));
        self.allocas
            .push(format!("  store {layout} zeroinitializer, ptr {slot}"));
        self.slots.push(slot.clone());
        self.slot_types.insert(slot.clone(), ty.clone());
        if temporary {
            self.text_temps.push(slot.clone());
        }
        slot
    }
    fn release_text(&mut self, slot: &str) {
        let kind = if matches!(self.slot_types.get(slot), Some(Type::List(_))) {
            "list"
        } else {
            "text"
        };
        self.emit(format!(
            "call void @lisan_rt_{kind}_release(ptr %text.ctx, ptr {slot})"
        ));
    }
    fn text_status(&mut self, status: &str, span: Span) {
        let ok = self.value(format!("icmp eq i32 {status}, 0"));
        self.guard(&ok, span);
    }
    fn clone_text(&mut self, source: &str, target: &str, span: Span) {
        let kind = if matches!(self.slot_types.get(target), Some(Type::List(_)))
            || (target == "%text.out" && matches!(self.result, Type::List(_)))
        {
            "list"
        } else {
            "text"
        };
        let status = self.value(format!(
            "call i32 @lisan_rt_{kind}_clone(ptr %text.ctx, ptr {source}, ptr {target})"
        ));
        self.text_status(&status, span);
    }
    fn move_managed(&mut self, source: &str, target: &str, ty: &Type) {
        let layout = if *ty == Type::Text {
            "{i64, ptr}"
        } else {
            "ptr"
        };
        let value = self.value(format!("load {layout}, ptr {source}"));
        self.emit(format!("store {layout} {value}, ptr {target}"));
        self.emit(format!("store {layout} zeroinitializer, ptr {source}"));
    }
    fn item_address(&mut self, e: &TypedExpr) -> Result<String> {
        let value = self.expr(e)?;
        if e.ty.is_managed() {
            let snapshot = self.managed_slot(&e.ty, true);
            self.clone_text(&value, &snapshot, e.span);
            Ok(snapshot)
        } else {
            let ty = llvm(&e.ty)?;
            let slot = self.temp();
            self.allocas.push(format!("  {slot} = alloca {ty}"));
            self.emit(format!("store {ty} {value}, ptr {slot}"));
            Ok(slot)
        }
    }
    fn list_mutation(
        &mut self,
        target: LocalId,
        index: Option<&TypedExpr>,
        value: &TypedExpr,
        span: Span,
    ) -> Result<String> {
        let (ty, local) = self
            .locals
            .get(&target)
            .cloned()
            .ok_or_else(|| error(span, "متغير قائمة غير معروف"))?;
        let Type::List(_) = &ty else {
            return Err(error(span, "نوع قائمة غير صالح"));
        };
        let snapshot = self.managed_slot(&ty, true);
        self.clone_text(&local, &snapshot, span);
        let index = if let Some(i) = index {
            let v = self.expr(i)?;
            Some(self.text_index(&v, &i.ty, i.span)?)
        } else {
            None
        };
        let address = self.item_address(value)?;
        let output = self.managed_slot(&ty, true);
        let call = if let Some(i) = index {
            format!("call i32 @lisan_rt_list_set(ptr %text.ctx, ptr {snapshot}, i64 {i}, ptr {address}, ptr {output})")
        } else {
            format!("call i32 @lisan_rt_list_append(ptr %text.ctx, ptr {snapshot}, ptr {address}, ptr {output})")
        };
        let status = self.value(call);
        self.text_status(&status, span);
        self.release_text(&local);
        self.move_managed(&output, &local, &ty);
        if value.ty.is_managed() {
            Ok(address)
        } else {
            Ok(self.value(format!("load {}, ptr {address}", llvm(&value.ty)?)))
        }
    }
    fn text_index(&mut self, v: &str, ty: &Type, span: Span) -> Result<String> {
        match ty {
            Type::Int { signed, bits } => {
                if *signed {
                    let ok = self.value(format!("icmp sge i{bits} {v}, 0"));
                    self.guard(&ok, span);
                }
                if *bits == 64 {
                    Ok(v.into())
                } else {
                    Ok(self.value(format!("zext i{bits} {v} to i64")))
                }
            }
            Type::Float { bits } => {
                self.finite(v, *bits, span);
                let t = llvm(ty)?;
                let name = format!("llvm.trunc.f{bits}");
                self.module
                    .intrinsics
                    .insert(format!("declare {t} @{name}({t})"));
                let trunc = self.value(format!("call {t} @{name}({t} {v})"));
                let whole = self.value(format!("fcmp oeq {t} {v}, {trunc}"));
                self.guard(&whole, span);
                let low = self.value(format!("fcmp oge {t} {v}, {}", float_constant(0., *bits)));
                self.guard(&low, span);
                let high = self.value(format!(
                    "fcmp olt {t} {v}, {}",
                    float_constant(18446744073709551616., *bits)
                ));
                self.guard(&high, span);
                Ok(self.value(format!("fptoui {t} {v} to i64")))
            }
            _ => Err(error(span, "فهرس نص غير صالح")),
        }
    }
    fn expr(&mut self, e: &TypedExpr) -> Result<String> {
        let t = if e.ty.is_managed() {
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
                self.guard(&valid, e.span);
                let value = self.value(format!("load {t}, ptr {p}, align 1"));
                if let Type::Float { bits } = e.ty {
                    self.finite(&value, bits, e.span);
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
                self.guard(&valid, e.span);
                self.emit(format!("store {} {v}, ptr {p}, align 1", llvm(pointee)?));
                String::new()
            }

            TypedExprKind::List(items) => {
                let Type::List(element) = &e.ty else {
                    return Err(error(e.span, "نوع قائمة غير صالح"));
                };
                let name = match element.as_ref() {
                    Type::Int { signed, bits } => {
                        format!("{}{bits}", if *signed { "i" } else { "u" })
                    }
                    Type::Float { bits } => format!("f{bits}"),
                    Type::Bool => "bool".into(),
                    Type::Text => "text".into(),
                    _ => return Err(error(e.span, "نوع عنصر غير مدعوم")),
                };
                let array = self.temp();
                let len = items.len();
                self.allocas
                    .push(format!("  {array} = alloca [{len} x ptr]"));
                for (i, item) in items.iter().enumerate() {
                    let address = self.item_address(item)?;
                    let at = self.value(format!(
                        "getelementptr [{len} x ptr], ptr {array}, i64 0, i64 {i}"
                    ));
                    self.emit(format!("store ptr {address}, ptr {at}"));
                }
                let slot = self.managed_slot(&e.ty, true);
                let status=self.value(format!("call i32 @lisan_rt_list_new(ptr %text.ctx, ptr @lisan_rt_ops_{name}, i64 {len}, ptr {array}, ptr {slot})"));
                self.text_status(&status, e.span);
                slot
            }
            TypedExprKind::ListLength(a) => {
                let v = self.expr(a)?;
                self.value(format!("call i64 @lisan_rt_list_len(ptr {v})"))
            }
            TypedExprKind::ListIndex(a, index) => {
                let v = self.expr(a)?;
                let snapshot = self.managed_slot(&a.ty, true);
                self.clone_text(&v, &snapshot, a.span);
                let i = self.expr(index)?;
                let i = self.text_index(&i, &index.ty, index.span)?;
                let output = if e.ty.is_managed() {
                    self.managed_slot(&e.ty, true)
                } else {
                    let p = self.temp();
                    self.allocas.push(format!("  {p} = alloca {t}"));
                    p
                };
                let status=self.value(format!("call i32 @lisan_rt_list_at(ptr %text.ctx, ptr {snapshot}, i64 {i}, ptr {output})"));
                self.text_status(&status, e.span);
                if e.ty.is_managed() {
                    output
                } else {
                    self.value(format!("load {t}, ptr {output}"))
                }
            }
            TypedExprKind::ListSet {
                target,
                index,
                value,
            } => self.list_mutation(*target, Some(index), value, e.span)?,
            TypedExprKind::ListAppend { target, value } => {
                self.list_mutation(*target, None, value, e.span)?;
                String::new()
            }
            TypedExprKind::Text(text) => {
                let bytes = self.module.text(text);
                let slot = self.text_slot(true);
                let status=self.value(format!("call i32 @lisan_rt_text_from_utf8(ptr %text.ctx, ptr {bytes}, i64 {}, ptr {slot})",text.len()));
                self.text_status(&status, e.span);
                slot
            }
            TypedExprKind::TextLength(a) => {
                let a = self.expr(a)?;
                self.value(format!("call i64 @lisan_rt_text_scalar_len(ptr {a})"))
            }
            TypedExprKind::TextIndex(a, b) => {
                let a_value = self.expr(a)?;
                let a = self.text_slot(true);
                self.clone_text(&a_value, &a, e.span);
                let index = self.expr(b)?;
                let index = self.text_index(&index, &b.ty, b.span)?;
                let slot = self.text_slot(true);
                let status = self.value(format!(
                    "call i32 @lisan_rt_text_at(ptr %text.ctx, ptr {a}, i64 {index}, ptr {slot})"
                ));
                self.text_status(&status, e.span);
                slot
            }
            TypedExprKind::Variable(id) => {
                let (_, p) = self
                    .locals
                    .get(id)
                    .ok_or_else(|| error(e.span, "متغير غير معروف"))?
                    .clone();
                if e.ty.is_managed() {
                    return Ok(p);
                }
                let value = self.value(format!("load {t}, ptr {p}"));
                if let Type::Float { bits } = e.ty {
                    self.finite(&value, bits, e.span);
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
                if e.ty.is_managed() {
                    let stage = self.managed_slot(&e.ty, true);
                    self.clone_text(&v, &stage, e.span);
                    self.release_text(&p);
                    self.move_managed(&stage, &p, &e.ty);
                    p
                } else {
                    self.emit(format!("store {t} {v}, ptr {p}"));
                    v
                }
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
                            self.finite(&r, bits, e.span);
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
                let mut av = self.expr(a)?;
                if a.ty.is_managed() {
                    let snapshot = self.managed_slot(&a.ty, true);
                    self.clone_text(&av, &snapshot, a.span);
                    av = snapshot;
                }
                let bv = self.expr(b)?;
                if matches!(a.ty, Type::List(_)) {
                    let equal = self.value(format!(
                        "call zeroext i1 @lisan_rt_list_equal(ptr {av}, ptr {bv})"
                    ));
                    if matches!(op, Op::NotEqual) {
                        self.value(format!("xor i1 {equal}, true"))
                    } else {
                        equal
                    }
                } else if a.ty == Type::Text {
                    if matches!(op, Op::Add) {
                        let slot = self.text_slot(true);
                        let status=self.value(format!("call i32 @lisan_rt_text_concat(ptr %text.ctx, ptr {av}, ptr {bv}, ptr {slot})"));
                        self.text_status(&status, e.span);
                        slot
                    } else {
                        let equal = self.value(format!(
                            "call zeroext i1 @lisan_rt_text_equal(ptr {av}, ptr {bv})"
                        ));
                        if matches!(op, Op::NotEqual) {
                            self.value(format!("xor i1 {equal}, true"))
                        } else {
                            equal
                        }
                    }
                } else {
                    self.binary(*op, &av, &bv, &a.ty, e.span)?
                }
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
                if e.ty != c.signature.result {
                    return Err(error(e.span, "نوع إرجاع الاستدعاء غير صالح"));
                }
                for (a, ty) in args.iter().zip(&c.signature.params) {
                    if a.ty != *ty {
                        return Err(error(a.span, "نوع وسيط غير صالح"));
                    }
                    let mut v = self.expr(a)?;
                    if ty.is_managed() {
                        let snapshot = self.managed_slot(ty, true);
                        self.clone_text(&v, &snapshot, a.span);
                        v = snapshot;
                    }
                    values.push(parameter(ty, c.signature.abi, &v)?);
                }
                let output = if e.ty.is_managed() {
                    let slot = self.managed_slot(&e.ty, true);
                    values.insert(0, format!("ptr {slot}"));
                    Some(slot)
                } else {
                    None
                };
                let call = format!(
                    "call {} {}({})",
                    result_type(&c.signature)?,
                    symbol(&c.symbol),
                    values.join(", ")
                );
                let r = if let Some(output) = output {
                    self.emit(call);
                    output
                } else if e.ty == Type::Void {
                    self.emit(call);
                    String::new()
                } else {
                    self.value(call)
                };
                if let Type::Float { bits } = e.ty {
                    self.finite(&r, bits, e.span);
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
                self.guard(&valid, span);
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
                self.guard(&valid, span);
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
                        self.finite(&r, *bits, span);
                        return Ok(r);
                    }
                    _ => return Err(error(span, "عملية عشرية غير مدعومة")),
                };
                let r = self.value(format!("{opcode} {t} {a}, {b}"));
                self.finite(&r, *bits, span);
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
                self.guard(&valid, span);
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
                self.finite(&r, *bits, span);
                Ok(r)
            }
            (Type::Float { bits: fb }, Type::Float { bits: tb }) => {
                let r = self.value(format!(
                    "{} {a} {v} to {b}",
                    if tb < fb { "fptrunc" } else { "fpext" }
                ));
                self.finite(&r, *tb, span);
                Ok(r)
            }
            (Type::Float { bits }, Type::Int { signed, bits: ib }) => {
                self.finite(v, *bits, span);
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
                self.guard(&valid, span);
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
            if !text.contains('\0') {
                let p = self.module.text(text);
                self.emit(format!("call i32 @puts(ptr {p})"));
                return Ok(());
            }
        }
        let v = self.expr(e)?;
        match &e.ty {
            Type::Text => {
                self.emit(format!("call void @lisan_rt_text_print(ptr {v})"));
                let newline = self.module.text("");
                self.emit(format!("call i32 @puts(ptr {newline})"));
            }
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
    fn flush_text_temps(&mut self) {
        for slot in std::mem::take(&mut self.text_temps).iter().rev() {
            self.release_text(slot);
        }
    }
    fn scoped_stmts(&mut self, body: &[TypedStmt]) -> Result<()> {
        self.text_scopes.push(vec![]);
        self.stmts(body)?;
        let slots = self.text_scopes.pop().unwrap();
        if !self.terminated {
            for slot in slots.iter().rev() {
                self.release_text(slot);
            }
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
                    self.bind(*id, ty, &v, s.span)?;
                }
                TypedStmtKind::Print(e) => self.print(e)?,
                TypedStmtKind::Expr(e) => {
                    self.expr(e)?;
                }
                TypedStmtKind::Block(body) => {
                    self.scoped_stmts(body)?;
                }
                TypedStmtKind::If(cond, yes, no) => {
                    let v = self.expr(cond)?;
                    self.flush_text_temps();
                    let y = self.block();
                    let n = self.block();
                    let end = self.block();
                    self.emit(format!("br i1 {v}, label %{y}, label %{n}"));
                    self.label(&y);
                    self.scoped_stmts(yes)?;
                    let y_done = self.terminated;
                    if !y_done {
                        self.jump(&end);
                    }
                    self.label(&n);
                    if let Some(no) = no {
                        self.scoped_stmts(no)?;
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
                    self.flush_text_temps();
                    self.emit(format!("br i1 {v}, label %{run}, label %{end}"));
                    self.label(&run);
                    self.loops
                        .push((test.clone(), end.clone(), self.text_scopes.len()));
                    self.scoped_stmts(body)?;
                    self.loops.pop();
                    if !self.terminated {
                        self.jump(&test);
                    }
                    self.label(&end);
                }
                TypedStmtKind::Break | TypedStmtKind::Continue => {
                    let (test, end, depth) = self
                        .loops
                        .last()
                        .cloned()
                        .ok_or_else(|| error(s.span, "تحكم حلقة خارج الحلقة"))?;
                    self.flush_text_temps();
                    // Emit only this path's cleanup; enclosing frames must remain
                    // intact while lowering other branches and fallthrough.
                    let slots: Vec<_> = self.text_scopes[depth..]
                        .iter()
                        .rev()
                        .flat_map(|frame| frame.iter().rev().cloned())
                        .collect();
                    for slot in slots {
                        self.release_text(&slot);
                    }
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
                        if value.ty != self.result {
                            return Err(error(s.span, "نوع الإرجاع غير صالح"));
                        }
                        let v = self.expr(value)?;
                        if self.result.is_managed() {
                            self.clone_text(&v, "%text.out", s.span);
                            self.leave_frame();
                            self.emit("ret void");
                        } else {
                            self.leave_frame();
                            self.emit(format!("ret {} {v}", llvm(&self.result)?));
                        }
                    } else {
                        self.leave_frame();
                        self.emit("ret void");
                    }
                    self.terminated = true;
                }
            }
            if !self.terminated {
                self.flush_text_temps();
            }
        }
        Ok(())
    }
    fn leave_frame(&mut self) {
        if !self.module.managed {
            return;
        }
        for slot in self.slots.clone().iter().rev() {
            self.release_text(slot);
        }
        self.emit("store ptr %text.previous, ptr @.lisan.text.frames");
    }
    fn finish(mut self) -> Result<String> {
        if !self.terminated {
            if self.main && self.module.managed {
                self.emit("call void @.lisan.text.cleanup()");
            }
            if !self.main {
                self.leave_frame();
            }
            let zero = match self.result {
                Type::Text | Type::List(_) => {
                    self.emit("ret void");
                    None
                }
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
        if self.module.managed {
            let count = self.slots.len();
            self.allocas
                .push(format!("  %text.slots = alloca [{count} x {{ptr,ptr}}]"));
            for (i, slot) in self.slots.iter().enumerate() {
                self.allocas.push(format!("  %text.slot.{i} = getelementptr [{count} x {{ptr,ptr}}], ptr %text.slots, i64 0, i64 {i}"));
                self.allocas
                    .push(format!("  store ptr {slot}, ptr %text.slot.{i}"));
                let kind = if matches!(self.slot_types.get(slot), Some(Type::List(_))) {
                    "list"
                } else {
                    "text"
                };
                self.allocas.push(format!("  %text.release.{i} = getelementptr {{ptr,ptr}}, ptr %text.slot.{i}, i32 0, i32 1"));
                self.allocas.push(format!(
                    "  store ptr @lisan_rt_{kind}_release, ptr %text.release.{i}"
                ));
            }
            self.allocas.extend([
                "  %text.frame = alloca {ptr, ptr, i64}".into(),
                "  %text.previous = load ptr, ptr @.lisan.text.frames".into(),
                "  store ptr %text.previous, ptr %text.frame".into(),
                "  %text.frame.slots = getelementptr {ptr, ptr, i64}, ptr %text.frame, i32 0, i32 1".into(),
                "  store ptr %text.slots, ptr %text.frame.slots".into(),
                "  %text.frame.count = getelementptr {ptr, ptr, i64}, ptr %text.frame, i32 0, i32 2".into(),
                format!("  store i64 {count}, ptr %text.frame.count"),
                "  store ptr %text.frame, ptr @.lisan.text.frames".into(),
            ]);
            if !self.main {
                self.allocas
                    .push("  %text.ctx = load ptr, ptr @.lisan.text.context".into());
            }
        }
        self.lines.splice(1..1, self.allocas);
        Ok(self.lines.join("\n"))
    }
}

/// Lower checked native IR for the host's x86-64 Linux or Windows C ABI.
/// Managed values use per-invocation slots and fatal cleanup of active frames.
pub fn generate(program: &TypedProgram) -> std::result::Result<String, Vec<Diagnostic>> {
    generate_inner(program, false, None).map_err(|e| vec![e])
}
/// Emit a host-linkable LLVM module without an entry point.
/// Library modules must have no top-level executable statements.
pub fn generate_library(program: &TypedProgram) -> std::result::Result<String, Vec<Diagnostic>> {
    generate_inner(program, true, None).map_err(|e| vec![e])
}
pub fn generate_with_sources(
    program: &TypedProgram,
    sources: &SourceMap,
) -> std::result::Result<String, Vec<Diagnostic>> {
    generate_inner(program, false, Some(sources)).map_err(|e| vec![e])
}
pub fn generate_library_with_sources(
    program: &TypedProgram,
    sources: &SourceMap,
) -> std::result::Result<String, Vec<Diagnostic>> {
    generate_inner(program, true, Some(sources)).map_err(|e| vec![e])
}
// Mirror source-checker reachability, but validate every return, including
// statements after an unconditional return which emission will never visit.
fn validate_returns(f: &TypedFunction) -> Result<()> {
    const FALL: u8 = 1;
    const RETURN: u8 = 2;
    const BREAK: u8 = 4;
    const CONTINUE: u8 = 8;
    fn body(stmts: &[TypedStmt], result: &Type) -> Result<u8> {
        let mut flow = FALL;
        for s in stmts {
            let next = match &s.kind {
                TypedStmtKind::Return(value) => {
                    if match value {
                        Some(value) => *result == Type::Void || value.ty != *result,
                        None => *result != Type::Void,
                    } {
                        return Err(error(s.span, "نوع أو قيمة الإرجاع غير صالح"));
                    }
                    RETURN
                }
                TypedStmtKind::Block(b) => body(b, result)?,
                TypedStmtKind::If(_, yes, no) => {
                    body(yes, result)?
                        | match no {
                            Some(no) => body(no, result)?,
                            None => FALL,
                        }
                }
                TypedStmtKind::While(cond, b) => {
                    let inner = body(b, result)?;
                    (inner & RETURN)
                        | if !matches!(cond.kind, TypedExprKind::Bool(true)) || inner & BREAK != 0 {
                            FALL
                        } else {
                            0
                        }
                }
                TypedStmtKind::Break => BREAK,
                TypedStmtKind::Continue => CONTINUE,
                _ => FALL,
            };
            if flow & FALL != 0 {
                flow = (flow & !FALL) | next;
            }
        }
        Ok(flow)
    }
    let flow = body(&f.body, &f.signature.result)?;
    if f.signature.result != Type::Void
        && (f.annotated || f.signature.result.is_managed())
        && flow & FALL != 0
    {
        return Err(error(f.span, "ليست كل مسارات الدالة تعيد قيمة"));
    }
    Ok(())
}
fn generate_inner(
    program: &TypedProgram,
    library: bool,
    sources: Option<&SourceMap>,
) -> Result<String> {
    super::managed::validate(program)?;
    for f in &program.functions {
        validate_returns(f)?;
    }
    if super::managed::uses_runtime(program)
        && (!program.exports.is_empty()
            || program.functions.iter().any(|f| f.signature.abi == Abi::C))
    {
        return Err(error(
            Span::default(),
            "تصدير دوال C في برنامج نص مدار غير مدعوم",
        ));
    }
    if library && super::managed::uses_runtime(program) {
        return Err(error(Span::default(), "تصدير النص المدار غير مدعوم"));
    }
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
        sources: sources.cloned(),
        managed: super::managed::uses_runtime(program),
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
        if name.starts_with("lisan_rt_")
            || name.starts_with("llvm.")
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
        if f.signature.result.is_managed() {
            params.push("ptr %text.out".into());
        }
        for (i, (id, ty)) in f.params.iter().zip(&f.signature.params).enumerate() {
            let v = format!("%arg{i}");
            params.push(parameter(ty, f.signature.abi, &v)?);
            if f.signature.abi == Abi::C {
                if let Type::Float { bits } = ty {
                    w.finite(&v, *bits, f.span);
                }
            }
            w.bind(*id, ty, &v, f.span)?;
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
        if w.module.managed {
            let size = w.value("call i64 @lisan_rt_context_size()");
            w.emit(format!("%text.ctx = alloca i8, i64 {size}, align 16"));
            w.emit("call void @lisan_rt_context_init(ptr %text.ctx)");
            w.emit("store ptr %text.ctx, ptr @.lisan.text.context");
        }
        w.stmts(&program.main)?;
        functions.push(format!("define i32 @main() {{\n{}\n}}", w.finish()?));
    }
    let mut out=String::from("; Lisan checked scalar LLVM backend\ndeclare i32 @puts(ptr)\ndeclare i32 @printf(ptr, ...)\ndeclare void @exit(i32) noreturn\n");
    if m.managed {
        out.push_str(RUNTIME_DECLS);
        out.push_str(LIST_RUNTIME_DECLS);
        for name in [
            "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64", "bool", "text",
        ] {
            out.push_str(&format!(
                "@lisan_rt_ops_{name} = external constant {{i64,i64,ptr,ptr,ptr}}\n"
            ));
        }
        out.push_str("@.lisan.text.context = private global ptr null\n");
        out.push_str("@.lisan.text.frames = private global ptr null\n");
        let cleanup = String::from(
            r#"define internal void @.lisan.text.cleanup() {
entry:
  %ctx = load ptr, ptr @.lisan.text.context
  %active = icmp ne ptr %ctx, null
  br i1 %active, label %frame, label %done
frame:
  %f = load ptr, ptr @.lisan.text.frames
  %has = icmp ne ptr %f, null
  br i1 %has, label %setup, label %destroy
setup:
  %prev = load ptr, ptr %f
  %sp = getelementptr {ptr,ptr,i64}, ptr %f, i32 0, i32 1
  %slots = load ptr, ptr %sp
  %cp = getelementptr {ptr,ptr,i64}, ptr %f, i32 0, i32 2
  %count = load i64, ptr %cp
  br label %loop
loop:
  %i = phi i64 [0, %setup], [%next, %release]
  %more = icmp ult i64 %i, %count
  br i1 %more, label %release, label %pop
release:
  %p = getelementptr {ptr,ptr}, ptr %slots, i64 %i
  %slot = load ptr, ptr %p
  %rp = getelementptr {ptr,ptr}, ptr %p, i32 0, i32 1
  %releasefn = load ptr, ptr %rp
  call void %releasefn(ptr %ctx, ptr %slot)
  %next = add i64 %i, 1
  br label %loop
pop:
  store ptr %prev, ptr @.lisan.text.frames
  br label %frame
destroy:
  call void @lisan_rt_context_destroy(ptr %ctx)
  store ptr null, ptr @.lisan.text.context
  br label %done
done:
  ret void
}
"#,
        );
        functions.push(cleanup);
    }
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

const RUNTIME_DECLS:&str = "; LISAN_MANAGED_TEXT_RUNTIME_V1\ndeclare i64 @lisan_rt_context_size()\ndeclare void @lisan_rt_context_init(ptr)\ndeclare void @lisan_rt_context_destroy(ptr)\ndeclare i32 @lisan_rt_text_from_utf8(ptr,ptr,i64,ptr)\ndeclare i32 @lisan_rt_text_clone(ptr,ptr,ptr)\ndeclare i32 @lisan_rt_text_concat(ptr,ptr,ptr,ptr)\ndeclare i32 @lisan_rt_text_at(ptr,ptr,i64,ptr)\ndeclare void @lisan_rt_text_release(ptr,ptr)\ndeclare i64 @lisan_rt_text_scalar_len(ptr)\ndeclare zeroext i1 @lisan_rt_text_equal(ptr,ptr)\ndeclare void @lisan_rt_text_print(ptr)\n";

const LIST_RUNTIME_DECLS:&str = "declare i32 @lisan_rt_list_new(ptr,ptr,i64,ptr,ptr)\ndeclare i32 @lisan_rt_list_clone(ptr,ptr,ptr)\ndeclare void @lisan_rt_list_release(ptr,ptr)\ndeclare i64 @lisan_rt_list_len(ptr)\ndeclare i32 @lisan_rt_list_at(ptr,ptr,i64,ptr)\ndeclare i32 @lisan_rt_list_set(ptr,ptr,i64,ptr,ptr)\ndeclare i32 @lisan_rt_list_append(ptr,ptr,ptr,ptr)\ndeclare zeroext i1 @lisan_rt_list_equal(ptr,ptr)\n";
