use crate::ast::{Expr, Function, Op, Stmt};
use std::collections::HashMap;
use std::fmt;

const MAX_STEPS: usize = 250_000;
const MAX_CALL_DEPTH: usize = 128;
const MAX_LIST_SIZE: usize = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    Null,
    List(Vec<Value>),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Number(n) => write!(f, "{}", n),
            Value::Text(s) => write!(f, "{}", s),
            Value::Bool(true) => write!(f, "صحيح"),
            Value::Bool(false) => write!(f, "خطأ"),
            Value::Null => write!(f, "لاشيء"),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { write!(f, "، ")?; }
                    match item {
                        Value::Text(s) => write!(f, "{:?}", s)?,
                        value => write!(f, "{}", value)?,
                    }
                }
                write!(f, "]")
            }
        }
    }
}

pub struct Interpreter {
    scopes: Vec<HashMap<String, Value>>,
    functions: HashMap<String, Function>,
    output: Vec<String>,
    steps: usize,
    call_depth: usize,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            output: Vec::new(),
            steps: 0,
            call_depth: 0,
        }
    }
}

impl Interpreter {
    pub fn run_source(&mut self, source: &str) -> Result<Vec<String>, String> {
        let tokens = crate::lexer::tokenize(source)?;
        let statements = crate::parser::parse(tokens)?;
        self.output.clear();
        self.steps = 0;
        if self.execute_statements(&statements, false)?.is_some() {
            return Err("لا يجوز استخدام أرجع خارج الدالة".into());
        }
        Ok(std::mem::take(&mut self.output))
    }

    fn tick(&mut self) -> Result<(), String> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            Err("تجاوز البرنامج الحد الآمن لعدد خطوات التنفيذ".into())
        } else { Ok(()) }
    }

    fn execute_statements(&mut self, stmts: &[Stmt], scoped: bool) -> Result<Option<Value>, String> {
        if scoped { self.scopes.push(HashMap::new()); }
        let result = (|| {
            for stmt in stmts {
                if let Some(value) = self.execute(stmt)? { return Ok(Some(value)); }
            }
            Ok(None)
        })();
        if scoped { self.scopes.pop(); }
        result
    }

    fn execute(&mut self, stmt: &Stmt) -> Result<Option<Value>, String> {
        self.tick()?;
        match stmt {
            Stmt::Let(name, expr) => {
                let value = self.eval(expr)?;
                let scope = self.scopes.last_mut().expect("scope exists");
                if scope.contains_key(name) {
                    return Err(format!("المتغير '{}' معرّف من قبل في هذا النطاق", name));
                }
                scope.insert(name.clone(), value);
            }
            Stmt::Print(expr) => {
                let value = self.eval(expr)?;
                self.output.push(value.to_string());
            }
            Stmt::Expr(expr) => { self.eval(expr)?; }
            Stmt::Block(stmts) => { return self.execute_statements(stmts, true); }
            Stmt::If(cond, yes, no) => {
                if self.eval_bool(cond)? { return self.execute_statements(yes, true); }
                if let Some(no) = no { return self.execute_statements(no, true); }
            }
            Stmt::While(cond, stmts) => {
                loop {
                    self.tick()?;
                    if !self.eval_bool(cond)? { break; }
                    if let Some(value) = self.execute_statements(stmts, true)? {
                        return Ok(Some(value));
                    }
                }
            }
            Stmt::ForEach(name, iterable, stmts) => {
                let values = match self.eval(iterable)? {
                    Value::List(items) => items,
                    Value::Text(s) => s.chars().map(|c| Value::Text(c.to_string())).collect(),
                    _ => return Err("حلقة لكل تتطلب قائمة أو نصًا".into()),
                };
                for value in values {
                    self.tick()?;
                    self.scopes.push(HashMap::from([(name.clone(), value)]));
                    let result = self.execute_statements(stmts, false);
                    self.scopes.pop();
                    if let Some(value) = result? { return Ok(Some(value)); }
                }
            }
            Stmt::Function(f) => { self.functions.insert(f.name.clone(), f.clone()); }
            Stmt::Return(expr) => {
                let value = match expr { Some(expr) => self.eval(expr)?, None => Value::Null };
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    fn get_variable(&self, name: &str) -> Result<Value, String> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name)).cloned()
            .ok_or_else(|| format!("متغير غير معرّف: '{}'", name))
    }

    fn eval_bool(&mut self, expr: &Expr) -> Result<bool, String> {
        match self.eval(expr)? {
            Value::Bool(b) => Ok(b),
            _ => Err("الشرط يجب أن ينتج قيمة منطقية (صحيح أو خطأ)".into()),
        }
    }

    fn eval(&mut self, expr: &Expr) -> Result<Value, String> {
        self.tick()?;
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Text(s) => Ok(Value::Text(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Null => Ok(Value::Null),
            Expr::List(items) => {
                if items.len() > MAX_LIST_SIZE { return Err("القائمة تجاوزت الحد الأقصى".into()); }
                let mut values = Vec::with_capacity(items.len());
                for item in items { values.push(self.eval(item)?); }
                Ok(Value::List(values))
            }
            Expr::Variable(name) => self.get_variable(name),
            Expr::Assign(name, expr) => {
                let value = self.eval(expr)?;
                for scope in self.scopes.iter_mut().rev() {
                    if scope.contains_key(name) {
                        scope.insert(name.clone(), value.clone());
                        return Ok(value);
                    }
                }
                Err(format!("لا يمكن الإسناد لمتغير غير معرّف: '{}'", name))
            }
            Expr::Index(target, index) => {
                let value = self.eval(target)?;
                let idx = checked_index(self.eval(index)?)?;
                match value {
                    Value::List(items) => items.get(idx).cloned()
                        .ok_or_else(|| format!("الفهرس {} خارج حدود القائمة", idx)),
                    Value::Text(text) => text.chars().nth(idx)
                        .map(|c| Value::Text(c.to_string()))
                        .ok_or_else(|| format!("الفهرس {} خارج حدود النص", idx)),
                    _ => Err("الفهرسة تتطلب قائمة أو نصًا".into()),
                }
            }
            Expr::AssignIndex(name, index, expr) => {
                let idx = checked_index(self.eval(index)?)?;
                let value = self.eval(expr)?;
                for scope in self.scopes.iter_mut().rev() {
                    if let Some(target) = scope.get_mut(name) {
                        return match target {
                            Value::List(items) => {
                                if idx >= items.len() {
                                    Err(format!("الفهرس {} خارج حدود القائمة", idx))
                                } else {
                                    items[idx] = value.clone();
                                    Ok(value)
                                }
                            }
                            _ => Err("إسناد عنصر يتطلب متغير قائمة".into()),
                        };
                    }
                }
                Err(format!("متغير غير معرّف: '{}'", name))
            }
            Expr::Unary(op, expr) => {
                let value = self.eval(expr)?;
                match (op, value) {
                    (Op::Negate, Value::Number(n)) => Ok(Value::Number(-n)),
                    (Op::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    _ => Err("نوع قيمة غير صالح للعملية الأحادية".into()),
                }
            }
            Expr::Binary(left, Op::And, right) => {
                let lhs = self.eval_bool(left)?;
                if !lhs { return Ok(Value::Bool(false)); }
                Ok(Value::Bool(self.eval_bool(right)?))
            }
            Expr::Binary(left, Op::Or, right) => {
                let lhs = self.eval_bool(left)?;
                if lhs { return Ok(Value::Bool(true)); }
                Ok(Value::Bool(self.eval_bool(right)?))
            }
            Expr::Binary(left, op, right) => {
                let lhs = self.eval(left)?;
                let rhs = self.eval(right)?;
                self.binary(lhs, *op, rhs)
            }
            Expr::Call(name, args) => self.call(name, args),
        }
    }

    fn binary(&self, lhs: Value, op: Op, rhs: Value) -> Result<Value, String> {
        match op {
            Op::Equal => return Ok(Value::Bool(lhs == rhs)),
            Op::NotEqual => return Ok(Value::Bool(lhs != rhs)),
            Op::Add => {
                return match (lhs, rhs) {
                    (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),
                    (Value::Text(a), b) => Ok(Value::Text(a + &b.to_string())),
                    (a, Value::Text(b)) => Ok(Value::Text(a.to_string() + &b)),
                    (Value::List(mut a), Value::List(b)) if a.len() + b.len() <= MAX_LIST_SIZE => {
                        a.extend(b);
                        Ok(Value::List(a))
                    }
                    (Value::List(_), Value::List(_)) => Err("القائمة تجاوزت الحد الأقصى".into()),
                    _ => Err("الجمع يحتاج رقمين أو نصًا أو قائمتين".into()),
                }
            }
            _ => {}
        }
        match (lhs, rhs) {
            (Value::Number(a), Value::Number(b)) => {
                match op {
                    Op::Subtract => Ok(Value::Number(a - b)),
                    Op::Multiply => Ok(Value::Number(a * b)),
                    Op::Divide if b != 0.0 => Ok(Value::Number(a / b)),
                    Op::Modulo if b != 0.0 => Ok(Value::Number(a % b)),
                    Op::Divide | Op::Modulo => Err("القسمة على صفر ممنوعة".into()),
                    Op::Less => Ok(Value::Bool(a < b)),
                    Op::LessEqual => Ok(Value::Bool(a <= b)),
                    Op::Greater => Ok(Value::Bool(a > b)),
                    Op::GreaterEqual => Ok(Value::Bool(a >= b)),
                    _ => Err("عملية غير صالحة على الأرقام".into()),
                }
            }
            _ => Err("عملية حسابية/مقارنة تحتاج قيمتين رقميتين".into()),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<Value, String> {
        if !self.functions.contains_key(name) && is_builtin(name) {
            let mut values = Vec::with_capacity(args.len());
            for arg in args { values.push(self.eval(arg)?); }
            return call_builtin(name, values);
        }
        let function = self.functions.get(name).cloned()
            .ok_or_else(|| format!("دالة غير معرّفة: '{}'", name))?;
        if args.len() != function.params.len() {
            return Err(format!("الدالة '{}' تتوقع {} معاملات، استلمت {}",
                name, function.params.len(), args.len()));
        }
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err("تجاوز الحد الآمن لعمق استدعاء الدوال".into());
        }
        let mut local = HashMap::new();
        for (param, arg) in function.params.iter().zip(args) {
            local.insert(param.clone(), self.eval(arg)?);
        }
        // Local variables of the caller are invisible; globals are visible and writable.
        let globals = self.scopes[0].clone();
        let previous_scopes = std::mem::replace(&mut self.scopes, vec![globals, local]);
        self.call_depth += 1;
        let result = self.execute_statements(&function.body, false);
        self.call_depth -= 1;
        let updated_globals = self.scopes[0].clone();
        self.scopes = previous_scopes;
        self.scopes[0] = updated_globals;
        Ok(result?.unwrap_or(Value::Null))
    }
}

fn checked_index(index: Value) -> Result<usize, String> {
    match index {
        Value::Number(n) if n.is_finite() && n >= 0.0 && n.fract() == 0.0 && n < usize::MAX as f64 => Ok(n as usize),
        _ => Err("الفهرس يجب أن يكون عددًا صحيحًا غير سالب".into()),
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(name, "طول" | "نوع" | "نص" | "عدد" | "أضف" | "اضف" | "مدى" | "يحتوي")
}

fn require_args(name: &str, values: &[Value], count: usize) -> Result<(), String> {
    if values.len() == count { Ok(()) }
    else { Err(format!("الدالة '{}' تتوقع {} معاملات، استلمت {}", name, count, values.len())) }
}

fn parse_arabic_number(source: &str) -> Result<f64, String> {
    let mut output = String::new();
    for c in source.trim().chars() {
        let c = match c {
            '٠'..='٩' => (b'0' + (c as u32 - '٠' as u32) as u8) as char,
            '۰'..='۹' => (b'0' + (c as u32 - '۰' as u32) as u8) as char,
            '٫' => '.',
            c => c,
        };
        output.push(c);
    }
    let number: f64 = output.parse().map_err(|_| "لا يمكن تحويل النص إلى رقم".to_string())?;
    if number.is_finite() { Ok(number) }
    else { Err("الرقم خارج النطاق المسموح".into()) }
}

fn call_builtin(name: &str, args: Vec<Value>) -> Result<Value, String> {
    match name {
        "طول" => {
            require_args(name, &args, 1)?;
            match &args[0] {
                Value::List(items) => Ok(Value::Number(items.len() as f64)),
                Value::Text(text) => Ok(Value::Number(text.chars().count() as f64)),
                _ => Err("طول يقبل قائمة أو نصًا فقط".into()),
            }
        }
        "نوع" => {
            require_args(name, &args, 1)?;
            let kind = match args[0] {
                Value::Number(_) => "رقم",
                Value::Text(_) => "نص",
                Value::Bool(_) => "منطقي",
                Value::Null => "لاشيء",
                Value::List(_) => "قائمة",
            };
            Ok(Value::Text(kind.into()))
        }
        "نص" => {
            require_args(name, &args, 1)?;
            Ok(Value::Text(args[0].to_string()))
        }
        "عدد" => {
            require_args(name, &args, 1)?;
            match &args[0] {
                Value::Number(n) => Ok(Value::Number(*n)),
                Value::Text(s) => Ok(Value::Number(parse_arabic_number(s)?)),
                _ => Err("عدد يقبل رقمًا أو نصًا رقميًا".into()),
            }
        }
        "أضف" | "اضف" => {
            require_args(name, &args, 2)?;
            let mut items = match &args[0] {
                Value::List(items) => items.clone(),
                _ => return Err("أضف تتطلب قائمة كمعامل أول".into()),
            };
            if items.len() >= MAX_LIST_SIZE { return Err("القائمة تجاوزت الحد الأقصى".into()); }
            items.push(args[1].clone());
            Ok(Value::List(items))
        }
        "يحتوي" => {
            require_args(name, &args, 2)?;
            match (&args[0], &args[1]) {
                (Value::List(items), sought) => Ok(Value::Bool(items.contains(sought))),
                (Value::Text(text), Value::Text(sought)) => Ok(Value::Bool(text.contains(sought))),
                _ => Err("يحتوي تتطلب قائمة أو نصين".into()),
            }
        }
        "مدى" => {
            if args.len() != 1 && args.len() != 2 {
                return Err("مدى تتوقع رقمًا واحدًا أو رقمين".into());
            }
            let (start, end) = if args.len() == 1 {
                (0.0, &args[0])
            } else {
                let start = match args[0] {
                    Value::Number(n) => n,
                    _ => return Err("مدى تتطلب أعدادًا صحيحة".into()),
                };
                (start, &args[1])
            };
            let end = match end {
                Value::Number(n) => *n,
                _ => return Err("مدى تتطلب أعدادًا صحيحة".into()),
            };
            if !start.is_finite() || !end.is_finite() || start.fract() != 0.0 || end.fract() != 0.0 {
                return Err("مدى تتطلب أعدادًا صحيحة".into());
            }
            if start.abs() > 9_007_199_254_740_991.0 || end.abs() > 9_007_199_254_740_991.0 {
                return Err("مدى خارج حدود الأعداد الصحيحة الدقيقة".into());
            }
            let length = (end - start).max(0.0);
            if length > MAX_LIST_SIZE as f64 { return Err("مدى تجاوز الحد الأقصى للقائمة".into()); }
            Ok(Value::List((0..length as usize).map(|i| Value::Number(start + i as f64)).collect()))
        }
        _ => Err(format!("دالة غير معرّفة: '{}'", name)),
    }
}
