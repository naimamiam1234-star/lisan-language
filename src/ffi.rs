use crate::types::{FunctionSig, Type};
pub fn validate_symbol(s: &str) -> Result<(), &'static str> {
    let mut chars = s.chars();
    if !matches!(chars.next(),Some(c) if c.is_ascii_alphabetic()||c=='_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err("رمز الربط يجب أن يطابق [A-Za-z_][A-Za-z0-9_]*");
    }
    if s == "main"
        || s.starts_with("lisan_fn_")
        || s.starts_with("lisan_rt_")
        || s.starts_with("llvm.")
    {
        return Err("رمز الربط محجوز للمترجم");
    }
    Ok(())
}
pub fn validate_signature(s: &FunctionSig) -> Result<(), &'static str> {
    fn scalar(t: &Type) -> bool {
        match t {
            Type::Int { bits, .. } => [8, 16, 32, 64].contains(bits),
            Type::Float { bits } => [32, 64].contains(bits),
            Type::Bool => true,
            Type::Pointer(p) => **p == Type::Void || scalar(p),
            _ => false,
        }
    }
    if !s.params.iter().all(scalar) || (s.result != Type::Void && !scalar(&s.result)) {
        return Err("توقيع C يتطلب أنواعًا عددية أو منطقية أو مؤشرات؛ فراغ للنتيجة فقط");
    }
    Ok(())
}
