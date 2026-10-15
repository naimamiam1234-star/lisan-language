//! Native (ahead-of-time) compiler: Lisan AST -> LLVM IR -> machine code via Clang.
//! The emitted executable never includes the Rust interpreter or serialized AST.
use crate::ast::{Expr, Function, Op, Stmt};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Type { Number, Boolean, Text }
impl Type {
    fn llvm(self) -> &'static str { match self { Self::Number => "double", Self::Boolean => "i1", Self::Text => "ptr" } }
    fn arabic(self) -> &'static str { match self { Self::Number => "عدد", Self::Boolean => "منطقي", Self::Text => "نص" } }
}
#[derive(Clone)]
struct Value { ty: Type, operand: String }
#[derive(Clone)]
struct Binding { ty: Type, ptr: String }
struct LoopTarget { continue_label: String, break_label: String }

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
        Self { module, lines: vec!["entry:".into()], scopes: vec![HashMap::new()], loops: Vec::new(), next: 0, terminated: false, in_function }
    }
    fn new_reg(&mut self) -> String { let i=self.next; self.next+=1; format!("%t{}", i) }
    fn new_label(&mut self, prefix: &str) -> String { let i=self.next; self.next+=1; format!("{}.{}",prefix,i) }
    fn instruction(&mut self, s: impl Into<String>) { self.lines.push(format!("  {}",s.into())); }
    fn branch(&mut self, label: &str) { self.instruction(format!("br label %{}",label)); self.terminated=true; }
    fn label(&mut self, label: &str) { self.lines.push(format!("{}:",label)); self.terminated=false; }
    fn scope<F>(&mut self, f: F) -> Result<(), String> where F: FnOnce(&mut Self)->Result<(),String> {
        self.scopes.push(HashMap::new()); let result=f(self); self.scopes.pop(); result
    }
    fn lookup(&self, name:&str)->Result<Binding,String> {
        self.scopes.iter().rev().find_map(|m|m.get(name)).cloned()
            .ok_or_else(||format!("المتغير '{}' غير معرّف أثناء الترجمة الأصلية",name))
    }
    fn register(&mut self, name:&str, value:Value)->Result<(),String> {
        if self.scopes.last().unwrap().contains_key(name) {return Err(format!("المتغير '{}' معرّف مسبقًا في الكتلة",name));}
        let ptr=self.new_reg(); let llvm=value.ty.llvm();
        self.instruction(format!("{} = alloca {}",ptr,llvm));
        self.instruction(format!("store {} {}, ptr {}",llvm,value.operand,ptr));
        self.scopes.last_mut().unwrap().insert(name.into(),Binding { ty:value.ty,ptr }); Ok(())
    }
    fn requires(&self, value:&Value, expected:Type, what:&str)->Result<(),String> {
        if value.ty!=expected {return Err(format!("{} تتطلب {}، لكن القيمة {}",what,expected.arabic(),value.ty.arabic()));} Ok(())
    }
    fn generate_expr(&mut self, e:&Expr)->Result<Value,String> {
        match e {
            Expr::Number(n) => {
                if !n.is_finite() {return Err("الأرقام غير المنتهية غير مدعومة".into());}
                Ok(Value { ty:Type::Number, operand:format!("{:.17e}",n) })
            }
            Expr::Bool(v)=>Ok(Value { ty:Type::Boolean,operand:if *v {"true"}else{"false"}.into() }),
            Expr::Text(v)=>{
                let sym=self.module.intern_text(v);
                Ok(Value {ty:Type::Text,operand:format!("@{}",sym)})
            }
            Expr::Variable(name)=>{
                let binding=self.lookup(name)?;let reg=self.new_reg();
                self.instruction(format!("{} = load {}, ptr {}",reg,binding.ty.llvm(),binding.ptr));
                Ok(Value {ty:binding.ty,operand:reg})
            }
            Expr::Assign(name, rhs)=>{
                let rhs=self.generate_expr(rhs)?;let binding=self.lookup(name)?;
                self.requires(&rhs,binding.ty,"الإسناد")?;
                self.instruction(format!("store {} {}, ptr {}",binding.ty.llvm(),rhs.operand,binding.ptr));
                Ok(rhs)
            }
            Expr::Unary(op, rhs)=>{
                let rhs=self.generate_expr(rhs)?;
                match op {
                    Op::Negate=>{
                        self.requires(&rhs,Type::Number,"السالب")?;
                        let reg=self.new_reg();self.instruction(format!("{} = fneg double {}",reg,rhs.operand));
                        Ok(Value{ty:Type::Number,operand:reg})
                    }
                    Op::Not=>{
                        self.requires(&rhs,Type::Boolean,"ليس")?;
                        let reg=self.new_reg();self.instruction(format!("{} = xor i1 {}, true",reg,rhs.operand));
                        Ok(Value{ty:Type::Boolean,operand:reg})
                    }
                    _=>Err("عملية أحادية غير مدعومة".into())
                }
            }
            Expr::Binary(left,op,right)=>{
                if matches!(op,Op::And | Op::Or) {
                    let a=self.generate_expr(left)?;
                    self.requires(&a,Type::Boolean,"الربط المنطقي")?;
                    let eval=self.new_label("logic.rhs");let done=self.new_label("logic.end");
                    let skip=if matches!(op,Op::And){"false"}else{"true"};
                    let before=self.new_label("logic.skip");
                    if matches!(op,Op::And){self.instruction(format!("br i1 {}, label %{}, label %{}",a.operand,eval,before));}
                    else {self.instruction(format!("br i1 {}, label %{}, label %{}",a.operand,before,eval));}
                    self.terminated=true;
                    self.label(&before);self.branch(&done);
                    self.label(&eval);
                    let b=self.generate_expr(right)?;
                    self.requires(&b,Type::Boolean,"الربط المنطقي")?;
                    let last_label=self.new_label("logic.value");
                    self.branch(&last_label);
                    self.label(&last_label); self.branch(&done);
                    self.label(&done);
                    let reg=self.new_reg();
                    self.instruction(format!("{} = phi i1 [{}, %{}], [{}, %{}]",reg,skip,before,b.operand,last_label));
                    return Ok(Value{ty:Type::Boolean,operand:reg});
                }
                let a=self.generate_expr(left)?;let b=self.generate_expr(right)?;
                if a.ty!=b.ty {return Err("لا يجوز خلط الأنواع في العملية الثنائية".into());}
                let (cmd,out_ty) = match op {
                    Op::Add if a.ty==Type::Number => ("fadd",Type::Number),
                    Op::Subtract if a.ty==Type::Number => ("fsub",Type::Number),
                    Op::Multiply if a.ty==Type::Number => ("fmul",Type::Number),
                    Op::Divide if a.ty==Type::Number => ("fdiv",Type::Number),
                    Op::Modulo if a.ty==Type::Number => ("frem",Type::Number),
                    Op::Power if a.ty==Type::Number => {
                        let reg=self.new_reg();
                        self.instruction(format!("{} = call double @llvm.pow.f64(double {}, double {})",reg,a.operand,b.operand));
                        self.module.needs_pow=true;
                        return Ok(Value{ty:Type::Number,operand:reg});
                    }
                    Op::Less if a.ty==Type::Number => ("fcmp olt",Type::Boolean),
                    Op::LessEqual if a.ty==Type::Number => ("fcmp ole",Type::Boolean),
                    Op::Greater if a.ty==Type::Number => ("fcmp ogt",Type::Boolean),
                    Op::GreaterEqual if a.ty==Type::Number => ("fcmp oge",Type::Boolean),
                    Op::Equal if a.ty==Type::Number => ("fcmp oeq",Type::Boolean),
                    Op::NotEqual if a.ty==Type::Number => ("fcmp une",Type::Boolean),
                    Op::Equal if a.ty==Type::Boolean => ("icmp eq",Type::Boolean),
                    Op::NotEqual if a.ty==Type::Boolean => ("icmp ne",Type::Boolean),
                    _=>return Err(format!("العملية {:?} على {} غير مدعومة بعد في المترجم الأصلي",op,a.ty.arabic()))
                };
                let reg=self.new_reg();
                self.instruction(format!("{} = {} {} {}, {}",reg,cmd,a.ty.llvm(),a.operand,b.operand));
                Ok(Value{ty:out_ty,operand:reg})
            }
            Expr::Call(name,args)=>{
                let (symbol,count)=self.module.functions.get(name).cloned().ok_or_else(||format!("الدالة '{}' غير مدعومة في المترجم الأصلي",name))?;
                if count!=args.len(){return Err(format!("الدالة '{}' تتطلب {} معاملات",name,count));}
                let mut operands=Vec::new();
                for arg in args {let v=self.generate_expr(arg)?;self.requires(&v,Type::Number,"معامل الدالة")?;operands.push(format!("double {}",v.operand));}
                let reg=self.new_reg();
                self.instruction(format!("{} = call double @{}({})",reg,symbol,operands.join(", ")));
                Ok(Value{ty:Type::Number,operand:reg})
            }
            _=>Err("هذه البنية لم تُنفّذ بعد في المترجم الأصلي (القوائم والقواميس والفهرسة)".into())
        }
    }
    fn statements(&mut self,stmts:&[Stmt])->Result<(),String>{
        for s in stmts {if self.terminated {break;}self.statement(s)?;} Ok(())
    }
    fn statement(&mut self,stmt:&Stmt)->Result<(),String>{
        match stmt {
            Stmt::Let(n,e)=>{let v=self.generate_expr(e)?;self.register(n,v)},
            Stmt::Expr(e)=>{self.generate_expr(e)?;Ok(())},
            Stmt::Print(e)=>{
                let value=self.generate_expr(e)?;
                match value.ty {
                    Type::Number=>self.instruction(format!("call i32 (ptr, ...) @printf(ptr @.number_format, double {})",value.operand)),
                    Type::Text=>self.instruction(format!("call i32 @puts(ptr {})",value.operand)),
                    Type::Boolean=>{
                        let reg=self.new_reg();
                        self.instruction(format!("{} = select i1 {}, ptr @.true_text, ptr @.false_text",reg,value.operand));
                        self.instruction(format!("call i32 @puts(ptr {})",reg));
                    }
                }
                Ok(())
            }
            Stmt::Block(block)=>self.scope(|this|this.statements(block)),
            Stmt::If(condition,yes,no)=>{
                let cond=self.generate_expr(condition)?;self.requires(&cond,Type::Boolean,"شرط إذا")?;
                let yes_label=self.new_label("if.yes");let no_label=self.new_label("if.no");let end_label=self.new_label("if.end");
                self.instruction(format!("br i1 {}, label %{}, label %{}",cond.operand,yes_label,no_label));self.terminated=true;
                self.label(&yes_label);self.scope(|this|this.statements(yes))?;
                if !self.terminated {self.branch(&end_label);}
                self.label(&no_label);
                if let Some(no)=no {self.scope(|this|this.statements(no))?;}
                if !self.terminated {self.branch(&end_label);}
                self.label(&end_label);Ok(())
            }
            Stmt::While(condition,body)=>{
                let cond_label=self.new_label("while.cond");let body_label=self.new_label("while.body");let end_label=self.new_label("while.end");
                self.branch(&cond_label); self.label(&cond_label);
                let cond=self.generate_expr(condition)?;self.requires(&cond,Type::Boolean,"شرط بينما")?;
                self.instruction(format!("br i1 {}, label %{}, label %{}",cond.operand,body_label,end_label)); self.terminated=true;
                self.label(&body_label);self.loops.push(LoopTarget{continue_label:cond_label.clone(),break_label:end_label.clone()});
                let result=self.scope(|this|this.statements(body));self.loops.pop();result?;
                if !self.terminated {self.branch(&cond_label);}self.label(&end_label);Ok(())
            }
            Stmt::Break=>{let label=self.loops.last().ok_or("توقف خارج حلقة")?.break_label.clone();self.branch(&label);Ok(())},
            Stmt::Continue=>{let label=self.loops.last().ok_or("تابع خارج حلقة")?.continue_label.clone();self.branch(&label);Ok(())},
            Stmt::Return(result)=>{
                if !self.in_function{return Err("أرجع خارج دالة".into());}
                let result=match result {Some(expr)=>self.generate_expr(expr)?,None=>Value{ty:Type::Number,operand:"0.0".into()}};
                self.requires(&result,Type::Number,"القيمة المعادة من الدالة")?;
                self.instruction(format!("ret double {}",result.operand));self.terminated=true;Ok(())
            }
            Stmt::Function(_)=>Err("تعريف دالة داخل كتلة غير مدعوم في المترجم الأصلي".into()),
            Stmt::Import(_)=>Err("الاستيراد المتداخل غير مسموح".into()),
            Stmt::ForEach(_,_,_)=>Err("حلقة لكل غير مدعومة بعد في المترجم الأصلي، استخدم بينما".into()),
        }
    }
}

struct Module { strings: Vec<(String,Vec<u8>)>, functions:HashMap<String,(String,usize)>, needs_pow:bool }
impl Module {
    fn new()->Self { Self { strings:Vec::new(),functions:HashMap::new(),needs_pow:false } }
    fn intern_text(&mut self, value:&str)->String {
        let sym=format!(".text_{}",self.strings.len());
        let mut bytes=value.as_bytes().to_vec(); bytes.push(0);
        self.strings.push((sym.clone(),bytes));sym
    }
}

/// Emit LLVM IR directly from the Lisan AST. This does not call or embed the interpreter.
pub fn generate_llvm(stmts:&[Stmt])->Result<String,String>{
    let mut module=Module::new();
    let mut functions=Vec::<Function>::new();let mut main_stmts=Vec::new();
    for stmt in stmts {
        match stmt {
            Stmt::Function(f)=>{
                if module.functions.contains_key(&f.name){return Err(format!("تعريف مكرر للدالة '{}'",f.name));}
                let symbol=format!("lisan_fn_{}",functions.len());
                module.functions.insert(f.name.clone(),(symbol,f.params.len()));functions.push(f.clone());
            }
            other=>main_stmts.push(other.clone()),
        }
    }
    let mut function_ir=Vec::new();
    for fun in &functions {
        let (symbol,_)=module.functions.get(&fun.name).unwrap().clone();
        let mut writer=FunctionWriter::new(&mut module,true);
        for (i,param) in fun.params.iter().enumerate() {
            writer.register(param,Value{ty:Type::Number,operand:format!("%arg{}",i)})?;
        }
        writer.statements(&fun.body)?;
        if !writer.terminated {writer.instruction("ret double 0.0");}
        let params=(0..fun.params.len()).map(|i|format!("double %arg{}",i)).collect::<Vec<_>>().join(", ");
        function_ir.push(format!("define double @{}({}) {{\n{}\n}}",symbol,params,writer.lines.join("\n")));
    }
    let mut writer=FunctionWriter::new(&mut module,false);
    writer.statements(&main_stmts)?;
    if !writer.terminated {writer.instruction("ret i32 0");}
    let main_ir=format!("define i32 @main() {{\n{}\n}}",writer.lines.join("\n"));
    let mut ir=String::from("; Native Lisan: source language compiled to LLVM IR, not interpreted\nsource_filename = \"lisan_native\"\n\n");
    ir.push_str("declare i32 @puts(ptr)\ndeclare i32 @printf(ptr, ...)\n");
    if module.needs_pow {ir.push_str("declare double @llvm.pow.f64(double, double)\n");}
    let number_fmt=b"%.15g\n\0";
    ir.push_str(&format!("@.number_format = private unnamed_addr constant [{} x i8] c\"{}\"\n",number_fmt.len(),escape_llvm(number_fmt)));
    for (sym,value) in [('.'.to_string()+"true_text","صحيح"),('.'.to_string()+"false_text","خطأ")] {
        let mut bytes=value.as_bytes().to_vec();bytes.push(0);
        ir.push_str(&format!("@{} = private unnamed_addr constant [{} x i8] c\"{}\"\n",sym,bytes.len(),escape_llvm(&bytes)));
    }
    for (sym,bytes) in &module.strings {
        ir.push_str(&format!("@{} = private unnamed_addr constant [{} x i8] c\"{}\"\n",sym,bytes.len(),escape_llvm(bytes)));
    }
    ir.push('\n');for function in function_ir {ir.push_str(&function);ir.push_str("\n\n");}ir.push_str(&main_ir);ir.push('\n');Ok(ir)
}
fn escape_llvm(bytes:&[u8])->String {
    bytes.iter().map(|b|if (32..=126).contains(b)&&*b!=b'"'&&*b!=b'\\'{(*b as char).to_string()} else {format!("\\{:02X}",b)}).collect()
}

pub fn emit_file(path:&Path)->Result<String,String>{
    let ast=crate::modules::load_file(path)?;
    generate_llvm(&ast)
}

/// Compiles a Lisan file to a genuine native executable, without a VM or bundled interpreter.
/// Clang is used as the LLVM assembler/code generator and system linker.
pub fn build_native(source:&Path,output:&Path)->Result<PathBuf,String>{
    let ir=emit_file(source)?;
    let destination=if output.is_absolute(){output.to_path_buf()} else {std::env::current_dir().map_err(|e|e.to_string())?.join(output)};
    let parent=destination.parent().ok_or("مسار الناتج غير صالح")?;
    fs::create_dir_all(parent).map_err(|e|format!("تعذر إعداد مجلد الناتج: {}",e))?;
    let temporary=parent.join(format!(".lisan-ir-{}-{}.ll",std::process::id(),destination.file_name().unwrap().to_string_lossy()));
    fs::write(&temporary,ir).map_err(|e|format!("تعذر كتابة LLVM IR: {}",e))?;
    let command=Command::new("clang").arg("-x").arg("ir").arg(&temporary).arg("-O2").arg("-o").arg(&destination).arg("-lm").output();
    let _=fs::remove_file(&temporary);
    let result=command.map_err(|e|format!("تعذر تشغيل Clang؛ ثبّت clang وأضفه إلى PATH: {}",e))?;
    if !result.status.success(){return Err(format!("فشلت الترجمة الأصلية: {}",String::from_utf8_lossy(&result.stderr)));}
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn translate(source:&str)->Result<String,String>{generate_llvm(&crate::parser::parse(crate::lexer::tokenize(source)? )?)}
    #[test] fn real_machine_ir(){let ir=translate("ليكن س = 3؛ اطبع(س ** 2)؛").unwrap();assert!(ir.contains("define i32 @main"));assert!(ir.contains("llvm.pow.f64"));assert!(!ir.contains("Interpreter"));}
    #[test] fn dynamic_features_rejected(){let e=translate("اطبع([1، 2])؛").unwrap_err();assert!(e.contains("لم تُنفّذ"));}
    #[test] fn types_checked(){let e=translate("ليكن س = 2؛ س = صحيح؛").unwrap_err();assert!(e.contains("الإسناد"));}
    #[test] fn short_circuit_ir(){let ir=translate("اطبع(صحيح و خطأ)؛").unwrap();assert!(ir.contains("phi i1"));}
}
