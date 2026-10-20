//! Native (ahead-of-time) compiler: Lisan AST -> LLVM IR -> machine code via Clang.
//! The emitted executable never includes the Rust interpreter or serialized AST.
use crate::ast::{Expr, Function, Op, Stmt};
mod build;
pub mod typed_codegen;
pub use build::{compile_ir, compile_ir_with_options, save_ir, NativeBuildOptions, NativeEmit};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Type {
    Number,
    Boolean,
    Text,
    NumberList,
}
impl Type {
    fn llvm(self) -> &'static str {
        match self {
            Self::Number => "double",
            Self::Boolean => "i1",
            Self::Text => "ptr",
            Self::NumberList => "{ i64, ptr }",
        }
    }
    fn arabic(self) -> &'static str {
        match self {
            Self::Number => "عدد",
            Self::Boolean => "منطقي",
            Self::Text => "نص",
            Self::NumberList => "قائمة أرقام",
        }
    }
}
#[derive(Clone)]
struct Value {
    ty: Type,
    operand: String,
}
#[derive(Clone)]
struct Binding {
    ty: Type,
    ptr: String,
}
struct LoopTarget {
    continue_label: String,
    break_label: String,
}

struct FunctionWriter<'a> {
    module: &'a mut Module,
    lines: Vec<String>,
    scopes: Vec<HashMap<String, Binding>>,
    loops: Vec<LoopTarget>,
    next: usize,
    terminated: bool,
    in_function: bool,
}

impl<'a> FunctionWriter<'a> {
    fn new(module: &'a mut Module, in_function: bool) -> Self {
        Self {
            module,
            lines: vec!["entry:".into()],
            scopes: vec![HashMap::new()],
            loops: Vec::new(),
            next: 0,
            terminated: false,
            in_function,
        }
    }
    fn new_reg(&mut self) -> String {
        let i = self.next;
        self.next += 1;
        format!("%t{}", i)
    }
    fn new_label(&mut self, prefix: &str) -> String {
        let i = self.next;
        self.next += 1;
        format!("{}.{}", prefix, i)
    }
    fn instruction(&mut self, s: impl Into<String>) {
        self.lines.push(format!("  {}", s.into()));
    }
    // LLVM alloca in the function entry avoids unbounded stack growth inside loops.
    fn entry_alloca(&mut self, s: impl Into<String>) {
        self.lines.insert(1, format!("  {}", s.into()));
    }
    fn branch(&mut self, label: &str) {
        self.instruction(format!("br label %{}", label));
        self.terminated = true;
    }
    fn label(&mut self, label: &str) {
        self.lines.push(format!("{}:", label));
        self.terminated = false;
    }
    fn scope<F>(&mut self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut Self) -> Result<(), String>,
    {
        self.scopes.push(HashMap::new());
        let result = f(self);
        self.scopes.pop();
        result
    }
    fn lookup(&self, name: &str) -> Result<Binding, String> {
        self.scopes
            .iter()
            .rev()
            .find_map(|m| m.get(name))
            .cloned()
            .ok_or_else(|| format!("المتغير '{}' غير معرّف أثناء الترجمة الأصلية", name))
    }
    fn register(&mut self, name: &str, value: Value) -> Result<(), String> {
        if self.scopes.last().unwrap().contains_key(name) {
            return Err(format!("المتغير '{}' معرّف مسبقًا في الكتلة", name));
        }
        let ptr = self.new_reg();
        let llvm = value.ty.llvm();
        self.entry_alloca(format!("{} = alloca {}", ptr, llvm));
        self.instruction(format!("store {} {}, ptr {}", llvm, value.operand, ptr));
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.into(), Binding { ty: value.ty, ptr });
        Ok(())
    }
    fn requires(&self, value: &Value, expected: Type, what: &str) -> Result<(), String> {
        if value.ty != expected {
            return Err(format!(
                "{} تتطلب {}، لكن القيمة {}",
                what,
                expected.arabic(),
                value.ty.arabic()
            ));
        }
        Ok(())
    }
    fn convert_to_text(&mut self, value: Value) -> Result<Value, String> {
        match value.ty {
            Type::Text => Ok(value),
            Type::Boolean => {
                let reg = self.new_reg();
                self.instruction(format!(
                    "{} = select i1 {}, ptr @.true_text, ptr @.false_text",
                    reg, value.operand
                ));
                Ok(Value {
                    ty: Type::Text,
                    operand: reg,
                })
            }
            Type::Number => {
                let ptr = self.new_reg();
                self.instruction(format!("{} = call ptr @malloc(i64 64)", ptr));
                self.instruction(format!("call i32 (ptr, i64, ptr, ...) @snprintf(ptr {}, i64 64, ptr @.number_text_format, double {})",ptr,value.operand));
                Ok(Value {
                    ty: Type::Text,
                    operand: ptr,
                })
            }
            Type::NumberList => Err("لا يمكن دمج قائمة مع نص مباشرة في المترجم الأصلي".into()),
        }
    }
    fn reject_unless(&mut self, condition: String, message: &str) {
        let ok = self.new_label("guard.ok");
        let bad = self.new_label("guard.error");
        self.instruction(format!(
            "br i1 {}, label %{}, label %{}",
            condition, ok, bad
        ));
        self.terminated = true;
        self.label(&bad);
        let sym = self.module.intern_text(message);
        self.instruction(format!("call i32 @puts(ptr @{})", sym));
        self.instruction("call void @exit(i32 1)");
        self.instruction("unreachable");
        self.terminated = true;
        self.label(&ok);
    }
    fn require_finite(&mut self, operand: &str) {
        self.module.intrinsics.insert("fabs".into());
        let magnitude = self.new_reg();
        self.instruction(format!(
            "{} = call double @llvm.fabs.f64(double {})",
            magnitude, operand
        ));
        let valid = self.new_reg();
        self.instruction(format!(
            "{} = fcmp ole double {}, 0x7FEFFFFFFFFFFFFF",
            valid, magnitude
        ));
        self.reject_unless(valid, "خطأ: نتيجة العملية غير منتهية أو خارج النطاق");
    }
    fn list_length_and_data(&mut self, list: &Value) -> (String, String) {
        let len = self.new_reg();
        self.instruction(format!(
            "{} = extractvalue {{ i64, ptr }} {}, 0",
            len, list.operand
        ));
        let ptr = self.new_reg();
        self.instruction(format!(
            "{} = extractvalue {{ i64, ptr }} {}, 1",
            ptr, list.operand
        ));
        (len, ptr)
    }
    fn list_item_ptr(&mut self, list: &Value, index: Value) -> Result<String, String> {
        self.requires(&index, Type::Number, "فهرس القائمة")?;
        let (len, data) = self.list_length_and_data(list);
        let is_finite = self.new_reg();
        self.instruction(format!(
            "{} = fcmp ord double {}, {}",
            is_finite, index.operand, index.operand
        ));
        let integral = self.new_reg();
        self.instruction(format!(
            "{} = call double @llvm.floor.f64(double {})",
            integral, index.operand
        ));
        let is_int = self.new_reg();
        self.instruction(format!(
            "{} = fcmp oeq double {}, {}",
            is_int, index.operand, integral
        ));
        let positive = self.new_reg();
        self.instruction(format!(
            "{} = fcmp oge double {}, 0.0",
            positive, index.operand
        ));
        let len_float = self.new_reg();
        self.instruction(format!("{} = uitofp i64 {} to double", len_float, len));
        let in_range = self.new_reg();
        self.instruction(format!(
            "{} = fcmp olt double {}, {}",
            in_range, index.operand, len_float
        ));
        let ok_a = self.new_reg();
        self.instruction(format!("{} = and i1 {}, {}", ok_a, is_finite, is_int));
        let ok_b = self.new_reg();
        self.instruction(format!("{} = and i1 {}, {}", ok_b, positive, in_range));
        let ok = self.new_reg();
        self.instruction(format!("{} = and i1 {}, {}", ok, ok_a, ok_b));
        self.reject_unless(ok, "خطأ: فهرس القائمة خارج الحدود أو غير صحيح");
        let pos = self.new_reg();
        self.instruction(format!("{} = fptoui double {} to i64", pos, index.operand));
        let gep = self.new_reg();
        self.instruction(format!(
            "{} = getelementptr double, ptr {}, i64 {}",
            gep, data, pos
        ));
        Ok(gep)
    }
    fn build_numeric_list(&mut self, items: &[Expr]) -> Result<Value, String> {
        if items.len() > 100_000 {
            return Err("تجاوزت القائمة الحد الأقصى المسموح".into());
        }
        let cap = items.len().max(1);
        let arr = self.new_reg();
        self.entry_alloca(format!("{} = alloca [{} x double]", arr, cap));
        for (idx, element) in items.iter().enumerate() {
            let item = self.generate_expr(element)?;
            self.requires(&item, Type::Number, "عنصر القائمة")?;
            let addr = self.new_reg();
            self.instruction(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 {}",
                addr, cap, arr, idx
            ));
            self.instruction(format!("store double {}, ptr {}", item.operand, addr));
        }
        let first = self.new_reg();
        self.instruction(format!(
            "{} = insertvalue {{ i64, ptr }} poison, i64 {}, 0",
            first,
            items.len()
        ));
        let second = self.new_reg();
        self.instruction(format!(
            "{} = insertvalue {{ i64, ptr }} {}, ptr {}, 1",
            second, first, arr
        ));
        Ok(Value {
            ty: Type::NumberList,
            operand: second,
        })
    }
    fn for_each(&mut self, name: &str, iterable: &Expr, body: &[Stmt]) -> Result<(), String> {
        if let Expr::Call(function, args) = iterable {
            if function == "مدى" {
                return self.for_range(name, args, body);
            }
        }
        let list = self.generate_expr(iterable)?;
        self.requires(&list, Type::NumberList, "حلقة لكل")?;
        let (length, data) = self.list_length_and_data(&list);
        let index_ptr = self.new_reg();
        self.entry_alloca(format!("{} = alloca i64", index_ptr));
        self.instruction(format!("store i64 0, ptr {}", index_ptr));
        let cond = self.new_label("foreach.cond");
        let step = self.new_label("foreach.step");
        let work = self.new_label("foreach.body");
        let end = self.new_label("foreach.end");
        self.branch(&cond);
        self.label(&cond);
        let index = self.new_reg();
        self.instruction(format!("{} = load i64, ptr {}", index, index_ptr));
        let test = self.new_reg();
        self.instruction(format!("{} = icmp ult i64 {}, {}", test, index, length));
        self.instruction(format!("br i1 {}, label %{}, label %{}", test, work, end));
        self.terminated = true;
        self.label(&work);
        let gep = self.new_reg();
        self.instruction(format!(
            "{} = getelementptr double, ptr {}, i64 {}",
            gep, data, index
        ));
        let value = self.new_reg();
        self.instruction(format!("{} = load double, ptr {}", value, gep));
        self.loops.push(LoopTarget {
            continue_label: step.clone(),
            break_label: end.clone(),
        });
        let r = self.scope(|this| {
            this.register(
                name,
                Value {
                    ty: Type::Number,
                    operand: value,
                },
            )?;
            this.statements(body)
        });
        self.loops.pop();
        r?;
        if !self.terminated {
            self.branch(&step);
        }
        self.label(&step);
        let current = self.new_reg();
        self.instruction(format!("{} = load i64, ptr {}", current, index_ptr));
        let next = self.new_reg();
        self.instruction(format!("{} = add i64 {}, 1", next, current));
        self.instruction(format!("store i64 {}, ptr {}", next, index_ptr));
        self.branch(&cond);
        self.label(&end);
        Ok(())
    }
    fn for_range(&mut self, name: &str, args: &[Expr], body: &[Stmt]) -> Result<(), String> {
        if !(1..=3).contains(&args.len()) {
            return Err("مدى تقبل من معامل إلى ثلاثة".into());
        }
        let mut params = Vec::new();
        self.module.intrinsics.insert("fabs".into());
        self.module.intrinsics.insert("ceil".into());
        for arg in args {
            let v = self.generate_expr(arg)?;
            self.requires(&v, Type::Number, "معامل مدى")?;
            let integral = self.new_reg();
            self.instruction(format!(
                "{} = call double @llvm.floor.f64(double {})",
                integral, v.operand
            ));
            let is_int = self.new_reg();
            self.instruction(format!(
                "{} = fcmp oeq double {}, {}",
                is_int, v.operand, integral
            ));
            let magnitude = self.new_reg();
            self.instruction(format!(
                "{} = call double @llvm.fabs.f64(double {})",
                magnitude, v.operand
            ));
            let in_range = self.new_reg();
            self.instruction(format!(
                "{} = fcmp ole double {}, 0x433FFFFFFFFFFFFF",
                in_range, magnitude
            ));
            let valid = self.new_reg();
            self.instruction(format!("{} = and i1 {}, {}", valid, is_int, in_range));
            self.reject_unless(valid, "مدى تتطلب أعدادًا صحيحة دقيقة ضمن النطاق المسموح");
            params.push(v.operand);
        }
        let (start, end, step) = match params.len() {
            1 => ("0.0".to_string(), params[0].clone(), "1.0".to_string()),
            2 => (params[0].clone(), params[1].clone(), "1.0".to_string()),
            _ => (params[0].clone(), params[1].clone(), params[2].clone()),
        };
        let step_good = self.new_reg();
        self.instruction(format!("{} = fcmp one double {}, 0.0", step_good, step));
        self.reject_unless(step_good, "خطأ: خطوة مدى لا يجوز أن تكون صفرًا");
        let distance = self.new_reg();
        self.instruction(format!("{} = fsub double {}, {}", distance, end, start));
        let quotient = self.new_reg();
        self.instruction(format!("{} = fdiv double {}, {}", quotient, distance, step));
        let ceil = self.new_reg();
        self.instruction(format!(
            "{} = call double @llvm.ceil.f64(double {})",
            ceil, quotient
        ));
        let positive = self.new_reg();
        self.instruction(format!("{} = fcmp ogt double {}, 0.0", positive, quotient));
        let count_float = self.new_reg();
        self.instruction(format!(
            "{} = select i1 {}, double {}, double 0.0",
            count_float, positive, ceil
        ));
        let bounded = self.new_reg();
        self.instruction(format!(
            "{} = fcmp ole double {}, 1.000000e5",
            bounded, count_float
        ));
        self.reject_unless(bounded, "مدى تجاوز الحد الأقصى للقائمة");
        let count = self.new_reg();
        self.instruction(format!("{} = fptoui double {} to i64", count, count_float));
        let ptr = self.new_reg();
        self.entry_alloca(format!("{} = alloca i64", ptr));
        self.instruction(format!("store i64 0, ptr {}", ptr));
        let cond = self.new_label("range.cond");
        let body_label = self.new_label("range.body");
        let step_label = self.new_label("range.step");
        let done = self.new_label("range.end");
        self.branch(&cond);
        self.label(&cond);
        let index = self.new_reg();
        self.instruction(format!("{} = load i64, ptr {}", index, ptr));
        let active = self.new_reg();
        self.instruction(format!("{} = icmp ult i64 {}, {}", active, index, count));
        self.instruction(format!(
            "br i1 {}, label %{}, label %{}",
            active, body_label, done
        ));
        self.terminated = true;
        self.label(&body_label);
        let position = self.new_reg();
        self.instruction(format!("{} = uitofp i64 {} to double", position, index));
        let offset = self.new_reg();
        self.instruction(format!("{} = fmul double {}, {}", offset, position, step));
        let current = self.new_reg();
        self.instruction(format!("{} = fadd double {}, {}", current, start, offset));
        self.loops.push(LoopTarget {
            continue_label: step_label.clone(),
            break_label: done.clone(),
        });
        let r = self.scope(|this| {
            this.register(
                name,
                Value {
                    ty: Type::Number,
                    operand: current,
                },
            )?;
            this.statements(body)
        });
        self.loops.pop();
        r?;
        if !self.terminated {
            self.branch(&step_label);
        }
        self.label(&step_label);
        let next = self.new_reg();
        self.instruction(format!("{} = add i64 {}, 1", next, index));
        self.instruction(format!("store i64 {}, ptr {}", next, ptr));
        self.branch(&cond);
        self.label(&done);
        Ok(())
    }
    fn generate_expr(&mut self, e: &Expr) -> Result<Value, String> {
        match e {
            Expr::Number(n) => {
                if !n.is_finite() {
                    return Err("الأرقام غير المنتهية غير مدعومة".into());
                }
                Ok(Value {
                    ty: Type::Number,
                    operand: format!("{:.17e}", n),
                })
            }
            Expr::Bool(v) => Ok(Value {
                ty: Type::Boolean,
                operand: if *v { "true" } else { "false" }.into(),
            }),
            Expr::Text(v) => {
                let sym = self.module.intern_text(v);
                Ok(Value {
                    ty: Type::Text,
                    operand: format!("@{}", sym),
                })
            }
            Expr::List(items) => self.build_numeric_list(items),
            Expr::Index(target, index) => {
                let list = self.generate_expr(target)?;
                self.requires(&list, Type::NumberList, "الفهرسة")?;
                let idx = self.generate_expr(index)?;
                let addr = self.list_item_ptr(&list, idx)?;
                let value = self.new_reg();
                self.instruction(format!("{} = load double, ptr {}", value, addr));
                Ok(Value {
                    ty: Type::Number,
                    operand: value,
                })
            }
            Expr::AssignIndex(name, index, rhs) => {
                let binding = self.lookup(name)?;
                if binding.ty != Type::NumberList {
                    return Err("إسناد الفهرس يتطلب قائمة أرقام".into());
                }
                let list_ptr = self.new_reg();
                self.instruction(format!(
                    "{} = load {{ i64, ptr }}, ptr {}",
                    list_ptr, binding.ptr
                ));
                let idx = self.generate_expr(index)?;
                let addr = self.list_item_ptr(
                    &Value {
                        ty: Type::NumberList,
                        operand: list_ptr,
                    },
                    idx,
                )?;
                let val = self.generate_expr(rhs)?;
                self.requires(&val, Type::Number, "القيمة المسندة إلى القائمة")?;
                self.instruction(format!("store double {}, ptr {}", val.operand, addr));
                Ok(val)
            }
            Expr::Variable(name) => {
                let binding = self.lookup(name)?;
                let reg = self.new_reg();
                self.instruction(format!(
                    "{} = load {}, ptr {}",
                    reg,
                    binding.ty.llvm(),
                    binding.ptr
                ));
                Ok(Value {
                    ty: binding.ty,
                    operand: reg,
                })
            }
            Expr::Assign(name, rhs) => {
                let rhs = self.generate_expr(rhs)?;
                let binding = self.lookup(name)?;
                self.requires(&rhs, binding.ty, "الإسناد")?;
                self.instruction(format!(
                    "store {} {}, ptr {}",
                    binding.ty.llvm(),
                    rhs.operand,
                    binding.ptr
                ));
                Ok(rhs)
            }
            Expr::Unary(op, rhs) => {
                let rhs = self.generate_expr(rhs)?;
                match op {
                    Op::Negate => {
                        self.requires(&rhs, Type::Number, "السالب")?;
                        let reg = self.new_reg();
                        self.instruction(format!("{} = fneg double {}", reg, rhs.operand));
                        Ok(Value {
                            ty: Type::Number,
                            operand: reg,
                        })
                    }
                    Op::Not => {
                        self.requires(&rhs, Type::Boolean, "ليس")?;
                        let reg = self.new_reg();
                        self.instruction(format!("{} = xor i1 {}, true", reg, rhs.operand));
                        Ok(Value {
                            ty: Type::Boolean,
                            operand: reg,
                        })
                    }
                    _ => Err("عملية أحادية غير مدعومة".into()),
                }
            }
            Expr::Binary(left, op, right) => {
                if matches!(op, Op::And | Op::Or) {
                    let a = self.generate_expr(left)?;
                    self.requires(&a, Type::Boolean, "الربط المنطقي")?;
                    let eval = self.new_label("logic.rhs");
                    let done = self.new_label("logic.end");
                    let skip = if matches!(op, Op::And) {
                        "false"
                    } else {
                        "true"
                    };
                    let before = self.new_label("logic.skip");
                    if matches!(op, Op::And) {
                        self.instruction(format!(
                            "br i1 {}, label %{}, label %{}",
                            a.operand, eval, before
                        ));
                    } else {
                        self.instruction(format!(
                            "br i1 {}, label %{}, label %{}",
                            a.operand, before, eval
                        ));
                    }
                    self.terminated = true;
                    self.label(&before);
                    self.branch(&done);
                    self.label(&eval);
                    let b = self.generate_expr(right)?;
                    self.requires(&b, Type::Boolean, "الربط المنطقي")?;
                    let last_label = self.new_label("logic.value");
                    self.branch(&last_label);
                    self.label(&last_label);
                    self.branch(&done);
                    self.label(&done);
                    let reg = self.new_reg();
                    self.instruction(format!(
                        "{} = phi i1 [{}, %{}], [{}, %{}]",
                        reg, skip, before, b.operand, last_label
                    ));
                    return Ok(Value {
                        ty: Type::Boolean,
                        operand: reg,
                    });
                }
                let a = self.generate_expr(left)?;
                let b = self.generate_expr(right)?;
                if matches!(op, Op::Add) && (a.ty == Type::Text || b.ty == Type::Text) {
                    let a = self.convert_to_text(a)?;
                    let b = self.convert_to_text(b)?;
                    let size_a = self.new_reg();
                    self.instruction(format!("{} = call i64 @strlen(ptr {})", size_a, a.operand));
                    let size_b = self.new_reg();
                    self.instruction(format!("{} = call i64 @strlen(ptr {})", size_b, b.operand));
                    let sum = self.new_reg();
                    self.instruction(format!("{} = add i64 {}, {}", sum, size_a, size_b));
                    let length = self.new_reg();
                    self.instruction(format!("{} = add i64 {}, 1", length, sum));
                    let buf = self.new_reg();
                    self.instruction(format!("{} = call ptr @malloc(i64 {})", buf, length));
                    self.instruction(format!("call ptr @strcpy(ptr {}, ptr {})", buf, a.operand));
                    self.instruction(format!("call ptr @strcat(ptr {}, ptr {})", buf, b.operand));
                    return Ok(Value {
                        ty: Type::Text,
                        operand: buf,
                    });
                }
                if matches!(op, Op::Equal | Op::NotEqual)
                    && a.ty == Type::Text
                    && b.ty == Type::Text
                {
                    let cmp = self.new_reg();
                    self.instruction(format!(
                        "{} = call i32 @strcmp(ptr {}, ptr {})",
                        cmp, a.operand, b.operand
                    ));
                    let out = self.new_reg();
                    let code = if matches!(op, Op::Equal) { "eq" } else { "ne" };
                    self.instruction(format!("{} = icmp {} i32 {}, 0", out, code, cmp));
                    return Ok(Value {
                        ty: Type::Boolean,
                        operand: out,
                    });
                }
                if a.ty != b.ty {
                    return Err("لا يجوز خلط الأنواع في العملية الثنائية".into());
                }
                if a.ty == Type::Number && matches!(op, Op::Divide | Op::Modulo) {
                    let valid = self.new_reg();
                    self.instruction(format!("{} = fcmp one double {}, 0.0", valid, b.operand));
                    self.reject_unless(valid, "خطأ: قسمة أو باقي قسمة على صفر");
                }
                let (cmd, out_ty) = match op {
                    Op::Add if a.ty == Type::Number => ("fadd", Type::Number),
                    Op::Subtract if a.ty == Type::Number => ("fsub", Type::Number),
                    Op::Multiply if a.ty == Type::Number => ("fmul", Type::Number),
                    Op::Divide if a.ty == Type::Number => ("fdiv", Type::Number),
                    Op::Modulo if a.ty == Type::Number => ("frem", Type::Number),
                    Op::Power if a.ty == Type::Number => {
                        let reg = self.new_reg();
                        self.instruction(format!(
                            "{} = call double @llvm.pow.f64(double {}, double {})",
                            reg, a.operand, b.operand
                        ));
                        self.module.needs_pow = true;
                        self.require_finite(&reg);
                        return Ok(Value {
                            ty: Type::Number,
                            operand: reg,
                        });
                    }
                    Op::Less if a.ty == Type::Number => ("fcmp olt", Type::Boolean),
                    Op::LessEqual if a.ty == Type::Number => ("fcmp ole", Type::Boolean),
                    Op::Greater if a.ty == Type::Number => ("fcmp ogt", Type::Boolean),
                    Op::GreaterEqual if a.ty == Type::Number => ("fcmp oge", Type::Boolean),
                    Op::Equal if a.ty == Type::Number => ("fcmp oeq", Type::Boolean),
                    Op::NotEqual if a.ty == Type::Number => ("fcmp une", Type::Boolean),
                    Op::Equal if a.ty == Type::Boolean => ("icmp eq", Type::Boolean),
                    Op::NotEqual if a.ty == Type::Boolean => ("icmp ne", Type::Boolean),
                    _ => {
                        return Err(format!(
                            "العملية {:?} على {} غير مدعومة بعد في المترجم الأصلي",
                            op,
                            a.ty.arabic()
                        ))
                    }
                };
                let reg = self.new_reg();
                self.instruction(format!(
                    "{} = {} {} {}, {}",
                    reg,
                    cmd,
                    a.ty.llvm(),
                    a.operand,
                    b.operand
                ));
                if out_ty == Type::Number {
                    self.require_finite(&reg);
                }
                Ok(Value {
                    ty: out_ty,
                    operand: reg,
                })
            }
            Expr::Call(name, args) => {
                match name.as_str() {
                    "طول" if args.len() == 1 => {
                        let v = self.generate_expr(&args[0])?;
                        let length = if v.ty == Type::NumberList {
                            let (len, _) = self.list_length_and_data(&v);
                            len
                        } else if v.ty == Type::Text {
                            let len = self.new_reg();
                            self.instruction(format!(
                                "{} = call i64 @lisan_utf8_len(ptr {})",
                                len, v.operand
                            ));
                            len
                        } else {
                            return Err("طول تتطلب نصًا أو قائمة أرقام".into());
                        };
                        let val = self.new_reg();
                        self.instruction(format!("{} = uitofp i64 {} to double", val, length));
                        return Ok(Value {
                            ty: Type::Number,
                            operand: val,
                        });
                    }
                    "مطلق" | "جذر" | "قرّب" | "قرب" if args.len() == 1 => {
                        let v = self.generate_expr(&args[0])?;
                        self.requires(&v, Type::Number, "دالة رياضية")?;
                        let intrinsic = match name.as_str() {
                            "مطلق" => "fabs",
                            "جذر" => "sqrt",
                            _ => "round",
                        };
                        if intrinsic == "sqrt" {
                            let valid = self.new_reg();
                            self.instruction(format!(
                                "{} = fcmp oge double {}, 0.0",
                                valid, v.operand
                            ));
                            self.reject_unless(valid, "خطأ: الجذر التربيعي لعدد سالب غير مدعوم");
                        }
                        let reg = self.new_reg();
                        self.instruction(format!(
                            "{} = call double @llvm.{}.f64(double {})",
                            reg, intrinsic, v.operand
                        ));
                        self.module.intrinsics.insert(intrinsic.into());
                        return Ok(Value {
                            ty: Type::Number,
                            operand: reg,
                        });
                    }
                    "أدنى" | "ادنى" | "أعلى" | "اعلى" if args.len() == 2 => {
                        let a = self.generate_expr(&args[0])?;
                        let b = self.generate_expr(&args[1])?;
                        self.requires(&a, Type::Number, "المقارنة")?;
                        self.requires(&b, Type::Number, "المقارنة")?;
                        let cmp = self.new_reg();
                        let op = if name == "أدنى" || name == "ادنى" {
                            "olt"
                        } else {
                            "ogt"
                        };
                        self.instruction(format!(
                            "{} = fcmp {} double {}, {}",
                            cmp, op, a.operand, b.operand
                        ));
                        let reg = self.new_reg();
                        self.instruction(format!(
                            "{} = select i1 {}, double {}, double {}",
                            reg, cmp, a.operand, b.operand
                        ));
                        return Ok(Value {
                            ty: Type::Number,
                            operand: reg,
                        });
                    }
                    "تحقق" if args.len() == 1 => {
                        let v = self.generate_expr(&args[0])?;
                        self.requires(&v, Type::Boolean, "تحقق")?;
                        self.reject_unless(v.operand.clone(), "فشل التحقق: الشرط غير صحيح");
                        return Ok(v);
                    }
                    "تحقق_يساوي" if args.len() == 2 => {
                        let left = self.generate_expr(&args[0])?;
                        let right = self.generate_expr(&args[1])?;
                        if left.ty != right.ty {
                            return Err("تحقق_يساوي تتطلب نوعين متساويين".into());
                        }
                        let comparison = match left.ty {
                            Type::Number => {
                                format!("fcmp oeq double {}, {}", left.operand, right.operand)
                            }
                            Type::Boolean => {
                                format!("icmp eq i1 {}, {}", left.operand, right.operand)
                            }
                            Type::Text => {
                                let cmp = self.new_reg();
                                self.instruction(format!(
                                    "{} = call i32 @strcmp(ptr {}, ptr {})",
                                    cmp, left.operand, right.operand
                                ));
                                format!("icmp eq i32 {}, 0", cmp)
                            }
                            Type::NumberList => {
                                return Err("تحقق_يساوي للقوائم غير مدعوم بعد".into())
                            }
                        };
                        let eq = self.new_reg();
                        self.instruction(format!("{} = {}", eq, comparison));
                        self.reject_unless(eq.clone(), "فشل التحقق: القيمتان غير متساويتين");
                        return Ok(Value {
                            ty: Type::Boolean,
                            operand: eq,
                        });
                    }
                    "نص" if args.len() == 1 => {
                        let v = self.generate_expr(&args[0])?;
                        return self.convert_to_text(v);
                    }
                    _ => {}
                }
                let (symbol, count) = self
                    .module
                    .functions
                    .get(name)
                    .cloned()
                    .ok_or_else(|| format!("الدالة '{}' غير مدعومة في المترجم الأصلي", name))?;
                if count != args.len() {
                    return Err(format!("الدالة '{}' تتطلب {} معاملات", name, count));
                }
                let mut operands = Vec::new();
                for arg in args {
                    let v = self.generate_expr(arg)?;
                    self.requires(&v, Type::Number, "معامل الدالة")?;
                    operands.push(format!("double {}", v.operand));
                }
                let reg = self.new_reg();
                self.instruction(format!(
                    "{} = call double @{}({})",
                    reg,
                    symbol,
                    operands.join(", ")
                ));
                Ok(Value {
                    ty: Type::Number,
                    operand: reg,
                })
            }
            _ => Err("هذه البنية لم تُنفّذ بعد في المترجم الأصلي (القواميس أو القيم الفارغة)".into()),
        }
    }
    fn statements(&mut self, stmts: &[Stmt]) -> Result<(), String> {
        for s in stmts {
            if self.terminated {
                break;
            }
            self.statement(s)?;
        }
        Ok(())
    }
    fn statement(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let(n, e) => {
                let v = self.generate_expr(e)?;
                self.register(n, v)
            }
            Stmt::Expr(e) => {
                self.generate_expr(e)?;
                Ok(())
            }
            Stmt::Print(e) => {
                let value = self.generate_expr(e)?;
                match value.ty {
                    Type::Number => self.instruction(format!(
                        "call i32 (ptr, ...) @printf(ptr @.number_format, double {})",
                        value.operand
                    )),
                    Type::Text => {
                        self.instruction(format!("call i32 @puts(ptr {})", value.operand))
                    }
                    Type::NumberList => {
                        return Err("اطبع لا تدعم طباعة قائمة كاملة مباشرة؛ استخدم لكل".into())
                    }
                    Type::Boolean => {
                        let reg = self.new_reg();
                        self.instruction(format!(
                            "{} = select i1 {}, ptr @.true_text, ptr @.false_text",
                            reg, value.operand
                        ));
                        self.instruction(format!("call i32 @puts(ptr {})", reg));
                    }
                }
                Ok(())
            }
            Stmt::Block(block) => self.scope(|this| this.statements(block)),
            Stmt::If(condition, yes, no) => {
                let cond = self.generate_expr(condition)?;
                self.requires(&cond, Type::Boolean, "شرط إذا")?;
                let yes_label = self.new_label("if.yes");
                let no_label = self.new_label("if.no");
                let end_label = self.new_label("if.end");
                self.instruction(format!(
                    "br i1 {}, label %{}, label %{}",
                    cond.operand, yes_label, no_label
                ));
                self.terminated = true;
                self.label(&yes_label);
                self.scope(|this| this.statements(yes))?;
                if !self.terminated {
                    self.branch(&end_label);
                }
                self.label(&no_label);
                if let Some(no) = no {
                    self.scope(|this| this.statements(no))?;
                }
                if !self.terminated {
                    self.branch(&end_label);
                }
                self.label(&end_label);
                Ok(())
            }
            Stmt::While(condition, body) => {
                let cond_label = self.new_label("while.cond");
                let body_label = self.new_label("while.body");
                let end_label = self.new_label("while.end");
                self.branch(&cond_label);
                self.label(&cond_label);
                let cond = self.generate_expr(condition)?;
                self.requires(&cond, Type::Boolean, "شرط بينما")?;
                self.instruction(format!(
                    "br i1 {}, label %{}, label %{}",
                    cond.operand, body_label, end_label
                ));
                self.terminated = true;
                self.label(&body_label);
                self.loops.push(LoopTarget {
                    continue_label: cond_label.clone(),
                    break_label: end_label.clone(),
                });
                let result = self.scope(|this| this.statements(body));
                self.loops.pop();
                result?;
                if !self.terminated {
                    self.branch(&cond_label);
                }
                self.label(&end_label);
                Ok(())
            }
            Stmt::Break => {
                let label = self
                    .loops
                    .last()
                    .ok_or("توقف خارج حلقة")?
                    .break_label
                    .clone();
                self.branch(&label);
                Ok(())
            }
            Stmt::Continue => {
                let label = self
                    .loops
                    .last()
                    .ok_or("تابع خارج حلقة")?
                    .continue_label
                    .clone();
                self.branch(&label);
                Ok(())
            }
            Stmt::Return(result) => {
                if !self.in_function {
                    return Err("أرجع خارج دالة".into());
                }
                let result = match result {
                    Some(expr) => self.generate_expr(expr)?,
                    None => Value {
                        ty: Type::Number,
                        operand: "0.0".into(),
                    },
                };
                self.requires(&result, Type::Number, "القيمة المعادة من الدالة")?;
                self.instruction(format!("ret double {}", result.operand));
                self.terminated = true;
                Ok(())
            }
            Stmt::Function(_) => Err("تعريف دالة داخل كتلة غير مدعوم في المترجم الأصلي".into()),
            Stmt::Import(_) => Err("الاستيراد المتداخل غير مسموح".into()),
            Stmt::ForEach(name, iterable, body) => self.for_each(name, iterable, body),
        }
    }
}

struct Module {
    strings: Vec<(String, Vec<u8>)>,
    functions: HashMap<String, (String, usize)>,
    needs_pow: bool,
    intrinsics: std::collections::HashSet<String>,
}
impl Module {
    fn new() -> Self {
        Self {
            strings: Vec::new(),
            functions: HashMap::new(),
            needs_pow: false,
            intrinsics: std::collections::HashSet::new(),
        }
    }
    fn intern_text(&mut self, value: &str) -> String {
        let sym = format!(".text_{}", self.strings.len());
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        self.strings.push((sym.clone(), bytes));
        sym
    }
}

/// Emit LLVM IR directly from the Lisan AST. This does not call or embed the interpreter.
pub fn generate_llvm(stmts: &[Stmt]) -> Result<String, String> {
    let mut module = Module::new();
    let mut functions = Vec::<Function>::new();
    let mut main_stmts = Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::Function(f) => {
                if module.functions.contains_key(&f.name) {
                    return Err(format!("تعريف مكرر للدالة '{}'", f.name));
                }
                let symbol = format!("lisan_fn_{}", functions.len());
                module
                    .functions
                    .insert(f.name.clone(), (symbol, f.params.len()));
                functions.push(f.clone());
            }
            other => main_stmts.push(other.clone()),
        }
    }
    let mut function_ir = Vec::new();
    for fun in &functions {
        let (symbol, _) = module.functions.get(&fun.name).unwrap().clone();
        let mut writer = FunctionWriter::new(&mut module, true);
        for (i, param) in fun.params.iter().enumerate() {
            writer.register(
                param,
                Value {
                    ty: Type::Number,
                    operand: format!("%arg{}", i),
                },
            )?;
        }
        writer.statements(&fun.body)?;
        if !writer.terminated {
            writer.instruction("ret double 0.0");
        }
        let params = (0..fun.params.len())
            .map(|i| format!("double %arg{}", i))
            .collect::<Vec<_>>()
            .join(", ");
        function_ir.push(format!(
            "define double @{}({}) {{\n{}\n}}",
            symbol,
            params,
            writer.lines.join("\n")
        ));
    }
    let mut writer = FunctionWriter::new(&mut module, false);
    writer.statements(&main_stmts)?;
    if !writer.terminated {
        writer.instruction("ret i32 0");
    }
    let main_ir = format!("define i32 @main() {{\n{}\n}}", writer.lines.join("\n"));
    let mut ir=String::from("; Native Lisan: source language compiled to LLVM IR, not interpreted\nsource_filename = \"lisan_native\"\n\n");
    ir.push_str("declare i32 @puts(ptr)\ndeclare i32 @printf(ptr, ...)\ndeclare i32 @snprintf(ptr, i64, ptr, ...)\ndeclare ptr @malloc(i64)\ndeclare ptr @strcpy(ptr, ptr)\ndeclare ptr @strcat(ptr, ptr)\ndeclare i64 @strlen(ptr)\ndeclare i32 @strcmp(ptr, ptr)\ndeclare void @exit(i32)\ndeclare double @llvm.floor.f64(double)\n");
    for intrinsic in ["fabs", "sqrt", "round", "ceil"] {
        if module.intrinsics.contains(intrinsic) {
            ir.push_str(&format!("declare double @llvm.{}.f64(double)\n", intrinsic));
        }
    }
    if module.needs_pow {
        ir.push_str("declare double @llvm.pow.f64(double, double)\n");
    }
    // UTF-8 length counts Unicode scalar values, not bytes.
    ir.push_str("define i64 @lisan_utf8_len(ptr %str) {\nentry:\n  br label %loop\nloop:\n  %p = phi ptr [ %str, %entry ], [ %next, %body ]\n  %count = phi i64 [ 0, %entry ], [ %add, %body ]\n  %byte = load i8, ptr %p\n  %end = icmp eq i8 %byte, 0\n  br i1 %end, label %done, label %body\nbody:\n  %mask = and i8 %byte, -64\n  %continuation = icmp eq i8 %mask, -128\n  %increment = zext i1 %continuation to i64\n  %amount = sub i64 1, %increment\n  %add = add i64 %count, %amount\n  %next = getelementptr i8, ptr %p, i64 1\n  br label %loop\ndone:\n  ret i64 %count\n}\n");
    let text_fmt = b"%.15g\0";
    ir.push_str(&format!(
        "@.number_text_format = private constant [{} x i8] c\"{}\"\n",
        text_fmt.len(),
        escape_llvm(text_fmt)
    ));
    let number_fmt = b"%.15g\n\0";
    ir.push_str(&format!(
        "@.number_format = private unnamed_addr constant [{} x i8] c\"{}\"\n",
        number_fmt.len(),
        escape_llvm(number_fmt)
    ));
    for (sym, value) in [
        ('.'.to_string() + "true_text", "صحيح"),
        ('.'.to_string() + "false_text", "خطأ"),
    ] {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        ir.push_str(&format!(
            "@{} = private unnamed_addr constant [{} x i8] c\"{}\"\n",
            sym,
            bytes.len(),
            escape_llvm(&bytes)
        ));
    }
    for (sym, bytes) in &module.strings {
        ir.push_str(&format!(
            "@{} = private unnamed_addr constant [{} x i8] c\"{}\"\n",
            sym,
            bytes.len(),
            escape_llvm(bytes)
        ));
    }
    ir.push('\n');
    for function in function_ir {
        ir.push_str(&function);
        ir.push_str("\n\n");
    }
    ir.push_str(&main_ir);
    ir.push('\n');
    Ok(ir)
}
fn escape_llvm(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| {
            if (32..=126).contains(b) && *b != b'"' && *b != b'\\' {
                (*b as char).to_string()
            } else {
                format!("\\{:02X}", b)
            }
        })
        .collect()
}

fn load_native_program(path: &Path) -> Result<crate::modules::LoadedProgram, String> {
    crate::modules::load_program(path).map_err(|errors| {
        errors
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    })
}

fn emit_loaded(loaded: &crate::modules::LoadedProgram, library: bool) -> Result<String, String> {
    if library || crate::syntax::requires_typed(&loaded.program) {
        let checked = crate::checker::check(&loaded.program).map_err(|errors| {
            errors
                .iter()
                .map(|d| d.render(&loaded.sources))
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        let generated = if library {
            typed_codegen::generate_library(&checked)
        } else {
            typed_codegen::generate(&checked)
        };
        generated.map_err(|errors| {
            errors
                .iter()
                .map(|d| d.render(&loaded.sources))
                .collect::<Vec<_>>()
                .join("\n")
        })
    } else {
        let ast =
            crate::syntax::to_legacy(&loaded.program).map_err(|d| d.render(&loaded.sources))?;
        generate_llvm(&ast)
    }
}

fn protect_loaded_sources(
    loaded: &crate::modules::LoadedProgram,
    output: &Path,
) -> Result<PathBuf, String> {
    let root = loaded
        .sources
        .sources
        .first()
        .ok_or("لا يوجد ملف مصدر محمل")?;
    // Resolve the destination exactly as the build driver will, including creation
    // and canonicalization of parents such as missing/../dependency.lsn.
    let destination = build::prepare_destination(&root.path, output)?;
    let resolved = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.clone());
    if loaded
        .sources
        .sources
        .iter()
        .any(|source| source.path == resolved)
    {
        return Err(format!(
            "لا يجوز استبدال أحد ملفات المصدر أو الاستيراد: {}",
            resolved.display()
        ));
    }
    Ok(destination)
}

pub fn emit_file(path: &Path) -> Result<String, String> {
    emit_loaded(&load_native_program(path)?, false)
}

/// Emit a checked scalar library without an application entry point.
pub fn emit_library_file(path: &Path) -> Result<String, String> {
    emit_loaded(&load_native_program(path)?, true)
}

/// Save LLVM while protecting every source in the resolved import graph.
pub fn emit_ir_file(source: &Path, output: &Path) -> Result<PathBuf, String> {
    let loaded = load_native_program(source)?;
    let output = protect_loaded_sources(&loaded, output)?;
    let ir = emit_loaded(&loaded, false)?;
    save_ir(&ir, source, &output)
}

/// Compiles a Lisan file to a genuine native executable, without a VM or bundled interpreter.
/// Clang is used as the LLVM assembler/code generator and system linker.
pub fn build_native(source: &Path, output: &Path) -> Result<PathBuf, String> {
    build_native_with_options(source, output, &NativeBuildOptions::default())
}

pub fn build_native_with_options(
    source: &Path,
    output: &Path,
    options: &NativeBuildOptions,
) -> Result<PathBuf, String> {
    let loaded = load_native_program(source)?;
    let output = protect_loaded_sources(&loaded, output)?;
    let ir = emit_loaded(&loaded, options.emit == NativeEmit::Object)?;
    compile_ir_with_options(&ir, source, &output, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn translate(source: &str) -> Result<String, String> {
        generate_llvm(&crate::parser::parse(crate::lexer::tokenize(source)?)?)
    }
    #[test]
    fn real_machine_ir() {
        let ir = translate("ليكن س = 3؛ اطبع(س ** 2)؛").unwrap();
        assert!(ir.contains("define i32 @main"));
        assert!(ir.contains("llvm.pow.f64"));
        assert!(!ir.contains("Interpreter"));
    }
    #[test]
    fn dynamic_features_rejected() {
        let e = translate("اطبع({\"س\": 2})؛").unwrap_err();
        assert!(e.contains("لم تُنفّذ"));
    }
    #[test]
    fn types_checked() {
        let e = translate("ليكن س = 2؛ س = صحيح؛").unwrap_err();
        assert!(e.contains("الإسناد"));
    }
    #[test]
    fn short_circuit_ir() {
        let ir = translate("اطبع(صحيح و خطأ)؛").unwrap();
        assert!(ir.contains("phi i1"));
    }
}
