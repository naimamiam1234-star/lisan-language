use crate::{
    ast::Op,
    diagnostic::Diagnostic,
    lexer::{LocatedToken, TokenKind as K},
    source::Span,
    syntax::*,
    types::Type,
};
use std::mem::discriminant;
pub fn parse_located(tokens: Vec<LocatedToken>) -> Result<Program, Diagnostic> {
    if tokens.last().map(|t| matches!(t.kind, K::Eof)) != Some(true) {
        return Err(Diagnostic::new(
            "E_PARSE",
            tokens.last().map(|t| t.span).unwrap_or_default(),
            "تدفق الرموز يحتاج علامة النهاية",
        ));
    }
    let mut p = Parser {
        tokens,
        pos: 0,
        loops: 0,
        functions: 0,
    };
    let mut statements = vec![];
    while !p.is(&K::Eof) {
        statements.push(p.statement()?);
    }
    Ok(Program { statements })
}
struct Parser {
    tokens: Vec<LocatedToken>,
    pos: usize,
    loops: usize,
    functions: usize,
}
impl Parser {
    fn current(&self) -> &LocatedToken {
        &self.tokens[self.pos]
    }
    fn is(&self, k: &K) -> bool {
        discriminant(&self.current().kind) == discriminant(k)
    }
    fn take(&mut self, k: &K) -> bool {
        if self.is(k) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn word(&self, w: &str) -> bool {
        matches!(&self.current().kind,K::Identifier(s) if s==w)
    }
    fn err(&self, msg: &str) -> Diagnostic {
        Diagnostic::new("E_PARSE", self.current().span, msg)
    }
    fn expect(&mut self, k: &K, msg: &str) -> Result<(), Diagnostic> {
        if self.take(k) {
            Ok(())
        } else {
            Err(self.err(msg))
        }
    }
    fn identifier(&mut self) -> Result<String, Diagnostic> {
        if let K::Identifier(s) = self.current().kind.clone() {
            self.pos += 1;
            Ok(s)
        } else {
            Err(self.err("متوقع اسم"))
        }
    }
    fn string(&mut self) -> Result<String, Diagnostic> {
        if let K::String(s) = self.current().kind.clone() {
            self.pos += 1;
            Ok(s)
        } else {
            Err(self.err("متوقع رمز ربط بين علامتي اقتباس"))
        }
    }
    fn span(&self, start: Span) -> Span {
        start.join(self.tokens[self.pos.saturating_sub(1)].span)
    }
    fn semi(&mut self) -> Result<(), Diagnostic> {
        self.expect(&K::Semicolon, "متوقع ';' أو '؛' في نهاية التعليمة")
    }
    fn ty(&mut self) -> Result<TypeSyntax, Diagnostic> {
        let span = self.current().span;
        let name = self.identifier()?;
        let kind = match name.as_str() {
            "ص8" => Type::Int {
                signed: true,
                bits: 8,
            },
            "ص16" => Type::Int {
                signed: true,
                bits: 16,
            },
            "ص32" => Type::Int {
                signed: true,
                bits: 32,
            },
            "ص64" => Type::Int {
                signed: true,
                bits: 64,
            },
            "ط8" => Type::Int {
                signed: false,
                bits: 8,
            },
            "ط16" => Type::Int {
                signed: false,
                bits: 16,
            },
            "ط32" => Type::Int {
                signed: false,
                bits: 32,
            },
            "ط64" => Type::Int {
                signed: false,
                bits: 64,
            },
            "ع32" => Type::Float { bits: 32 },
            "ع64" | "عدد" => Type::Float { bits: 64 },
            "منطقي" => Type::Bool,
            "نص" => Type::Text,
            "فراغ" => Type::Void,
            "مؤشر" => {
                self.expect(&K::Less, "متوقع '<' بعد مؤشر")?;
                let inner = self.ty()?;
                self.expect(&K::Greater, "متوقع '>' بعد نوع المؤشر")?;
                Type::Pointer(Box::new(inner.kind))
            }
            _ => {
                return Err(Diagnostic::new(
                    "E_PARSE",
                    span,
                    format!("نوع غير معروف: {name}"),
                ))
            }
        };
        Ok(Node {
            kind,
            span: self.span(span),
        })
    }
    fn block(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        self.expect(&K::LeftBrace, "متوقع '{'")?;
        let mut v = vec![];
        while !self.is(&K::RightBrace) && !self.is(&K::Eof) {
            v.push(self.statement()?);
        }
        self.expect(&K::RightBrace, "متوقع '}'")?;
        Ok(v)
    }
    fn signature(&mut self) -> Result<(String, Vec<Param>, Option<TypeSyntax>), Diagnostic> {
        self.expect(&K::Function, "متوقع دالة")?;
        let name = self.identifier()?;
        self.expect(&K::LeftParen, "متوقع '('")?;
        let mut params: Vec<Param> = vec![];
        if !self.is(&K::RightParen) {
            loop {
                let span = self.current().span;
                let name = self.identifier()?;
                if params.iter().any(|p| p.name == name) {
                    return Err(Diagnostic::new("E_DUPLICATE", span, "معامل مكرر"));
                }
                let ty = if self.take(&K::Colon) {
                    Some(self.ty()?)
                } else {
                    None
                };
                if matches!(ty.as_ref().map(|t| &t.kind), Some(Type::Void)) {
                    return Err(Diagnostic::new(
                        "E_PARSE",
                        ty.as_ref().unwrap().span,
                        "فراغ لا يصلح نوعًا لمعامل",
                    ));
                }
                params.push(Param {
                    name,
                    ty,
                    span: self.span(span),
                });
                if !self.take(&K::Comma) {
                    break;
                }
            }
        }
        self.expect(&K::RightParen, "متوقع ')'")?;
        let result = if self.take(&K::Colon) {
            Some(self.ty()?)
        } else {
            None
        };
        if params.iter().any(|p| p.ty.is_some()) || result.is_some() {
            if result.is_none() || params.iter().any(|p| p.ty.is_none()) {
                return Err(self.err("التوقيع المحدد يتطلب نوع كل معامل ونوع النتيجة"));
            }
        }
        Ok((name, params, result))
    }
    fn statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.current().span;
        let foreign = self.word("خارجي")
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(K::Function)
            );
        let export = (self.word("صدّر") || self.word("صدر"))
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(K::Function)
            );
        let kind = if foreign || export || self.is(&K::Function) {
            if foreign || export {
                self.pos += 1;
            }
            let (name, params, result) = self.signature()?;
            if foreign || export {
                if result.is_none() {
                    return Err(self.err("التصريح الخارجي أو المصدر يتطلب توقيعًا كاملًا"));
                }
                self.expect(&K::Equal, "متوقع '=' ثم رمز الربط الصريح")?;
                let symbol = self.string()?;
                if foreign {
                    self.semi()?;
                    StmtKind::ExternFunction(ExternFunction {
                        name,
                        symbol,
                        params,
                        result: result.unwrap(),
                    })
                } else {
                    let body = self.function_body()?;
                    StmtKind::ExportFunction {
                        function: Function {
                            name,
                            params,
                            result,
                            body,
                        },
                        symbol,
                    }
                }
            } else {
                let body = self.function_body()?;
                StmtKind::Function(Function {
                    name,
                    params,
                    result,
                    body,
                })
            }
        } else if self.take(&K::Import) {
            let p = self.string()?;
            self.semi()?;
            StmtKind::Import(p)
        } else if self.take(&K::Let) {
            let name = self.identifier()?;
            let ty = if self.take(&K::Colon) {
                Some(self.ty()?)
            } else {
                None
            };
            if matches!(ty.as_ref().map(|t| &t.kind), Some(Type::Void)) {
                return Err(self.err("فراغ لا يصلح نوعًا لمتغير"));
            }
            self.expect(&K::Equal, "متوقع '='")?;
            let value = self.expression()?;
            self.semi()?;
            StmtKind::Let { name, ty, value }
        } else if self.take(&K::Print) {
            self.expect(&K::LeftParen, "متوقع '('")?;
            let e = self.expression()?;
            self.expect(&K::RightParen, "متوقع ')'")?;
            self.semi()?;
            StmtKind::Print(e)
        } else if self.take(&K::If) {
            let c = self.condition()?;
            let yes = self.block()?;
            let no = if self.take(&K::Else) {
                Some(self.block()?)
            } else {
                None
            };
            StmtKind::If(c, yes, no)
        } else if self.take(&K::While) {
            let c = self.condition()?;
            self.loops += 1;
            let body = self.block()?;
            self.loops -= 1;
            StmtKind::While(c, body)
        } else if self.take(&K::ForEach) {
            self.expect(&K::LeftParen, "متوقع '('")?;
            let n = self.identifier()?;
            self.expect(&K::In, "متوقع في")?;
            let e = self.expression()?;
            self.expect(&K::RightParen, "متوقع ')'")?;
            self.loops += 1;
            let b = self.block()?;
            self.loops -= 1;
            StmtKind::ForEach(n, e, b)
        } else if self.take(&K::Return) {
            if self.functions == 0 {
                return Err(self.err("لا يجوز استخدام أرجع خارج الدالة"));
            }
            let e = if self.is(&K::Semicolon) {
                None
            } else {
                Some(self.expression()?)
            };
            self.semi()?;
            StmtKind::Return(e)
        } else if self.take(&K::Break) {
            if self.loops == 0 {
                return Err(self.err("لا يجوز استخدام توقف خارج الحلقة"));
            }
            self.semi()?;
            StmtKind::Break
        } else if self.take(&K::Continue) {
            if self.loops == 0 {
                return Err(self.err("لا يجوز استخدام تابع خارج الحلقة"));
            }
            self.semi()?;
            StmtKind::Continue
        } else if self.word("غير_آمن")
            && matches!(
                self.tokens.get(self.pos + 1).map(|t| &t.kind),
                Some(K::LeftBrace)
            )
        {
            self.pos += 1;
            StmtKind::Unsafe(self.block()?)
        } else if self.is(&K::LeftBrace) {
            StmtKind::Block(self.block()?)
        } else {
            let e = self.expression()?;
            self.semi()?;
            StmtKind::Expr(e)
        };
        Ok(Node {
            kind,
            span: self.span(start),
        })
    }
    fn function_body(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        let old = self.loops;
        self.loops = 0;
        self.functions += 1;
        let body = self.block();
        self.functions -= 1;
        self.loops = old;
        body
    }
    fn condition(&mut self) -> Result<Expr, Diagnostic> {
        self.expect(&K::LeftParen, "متوقع '('")?;
        let e = self.expression()?;
        self.expect(&K::RightParen, "متوقع ')'")?;
        Ok(e)
    }
    fn expression(&mut self) -> Result<Expr, Diagnostic> {
        let a = self.binary(0)?;
        if self.take(&K::Equal) {
            let b = self.expression()?;
            let span = a.span.join(b.span);
            let kind = match a.kind {
                ExprKind::Variable(n) => ExprKind::Assign(n, Box::new(b)),
                ExprKind::Index(t, i) => match t.kind {
                    ExprKind::Variable(n) => ExprKind::AssignIndex(n, i, Box::new(b)),
                    _ => return Err(self.err("هدف إسناد غير صالح")),
                },
                _ => return Err(self.err("هدف إسناد غير صالح")),
            };
            Ok(Node { kind, span })
        } else {
            Ok(a)
        }
    }
    fn operator(&self) -> Option<(u8, Op)> {
        Some(match self.current().kind {
            K::OrOr => (0, Op::Or),
            K::AndAnd => (1, Op::And),
            K::EqualEqual => (2, Op::Equal),
            K::BangEqual => (2, Op::NotEqual),
            K::Less => (3, Op::Less),
            K::LessEqual => (3, Op::LessEqual),
            K::Greater => (3, Op::Greater),
            K::GreaterEqual => (3, Op::GreaterEqual),
            K::Plus => (4, Op::Add),
            K::Minus => (4, Op::Subtract),
            K::Star => (5, Op::Multiply),
            K::Slash => (5, Op::Divide),
            K::Percent => (5, Op::Modulo),
            _ => return None,
        })
    }
    fn binary(&mut self, min: u8) -> Result<Expr, Diagnostic> {
        let mut lhs = self.unary()?;
        while let Some((p, op)) = self.operator() {
            if p < min {
                break;
            }
            self.pos += 1;
            let rhs = self.binary(p + 1)?;
            let span = lhs.span.join(rhs.span);
            lhs = Node {
                kind: ExprKind::Binary(Box::new(lhs), op, Box::new(rhs)),
                span,
            };
        }
        Ok(lhs)
    }
    fn unary(&mut self) -> Result<Expr, Diagnostic> {
        let span = self.current().span;
        let op = if self.take(&K::Minus) {
            Some(Op::Negate)
        } else if self.take(&K::Bang) {
            Some(Op::Not)
        } else {
            None
        };
        if let Some(op) = op {
            let e = self.unary()?;
            let span = span.join(e.span);
            return Ok(Node {
                kind: ExprKind::Unary(op, Box::new(e)),
                span,
            });
        }
        let lhs = self.call()?;
        if self.take(&K::Power) {
            let rhs = self.unary()?;
            let span = lhs.span.join(rhs.span);
            Ok(Node {
                kind: ExprKind::Binary(Box::new(lhs), Op::Power, Box::new(rhs)),
                span,
            })
        } else {
            Ok(lhs)
        }
    }
    fn call(&mut self) -> Result<Expr, Diagnostic> {
        let mut e = self.primary()?;
        loop {
            if self.take(&K::LeftBracket) {
                let i = self.expression()?;
                self.expect(&K::RightBracket, "متوقع ']'")?;
                let span = self.span(e.span);
                e = Node {
                    kind: ExprKind::Index(Box::new(e), Box::new(i)),
                    span,
                };
                continue;
            }
            if !self.take(&K::LeftParen) {
                break;
            }
            let mut args = vec![];
            if !self.is(&K::RightParen) {
                loop {
                    args.push(self.expression()?);
                    if !self.take(&K::Comma) {
                        break;
                    }
                }
            }
            self.expect(&K::RightParen, "متوقع ')'")?;
            let span = self.span(e.span);
            let name = match e.kind {
                ExprKind::Variable(n) => n,
                _ => return Err(self.err("الاستدعاء يحتاج إلى اسم دالة")),
            };
            let kind = match name.as_str() {
                "عنوان" | "تحميل" if args.len() == 1 => {
                    let x = Box::new(args.remove(0));
                    if name == "عنوان" {
                        ExprKind::Address(x)
                    } else {
                        ExprKind::Load(x)
                    }
                }
                "خزن" if args.len() == 2 => {
                    let y = args.remove(1);
                    ExprKind::Store(Box::new(args.remove(0)), Box::new(y))
                }
                "عنوان" | "تحميل" | "خزن" => {
                    return Err(self.err("عدد وسائط عملية المؤشر غير صحيح"))
                }
                _ => ExprKind::Call(name, args),
            };
            e = Node { kind, span };
        }
        Ok(e)
    }
    // Contextual builtin names remain ordinary identifiers unless a complete
    // generic type followed by a call opener can be parsed without consuming input.
    fn typed_call_ahead(&mut self) -> bool {
        let saved = self.pos;
        let matched = (|| {
            self.expect(&K::Less, "")?;
            self.ty()?;
            self.expect(&K::Greater, "")?;
            self.expect(&K::LeftParen, "")?;
            Ok::<(), Diagnostic>(())
        })()
        .is_ok();
        self.pos = saved;
        matched
    }
    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let token = self.current().clone();
        let start = token.span;
        self.pos += 1;
        let kind = match token.kind {
            K::Number(_) => ExprKind::Number(NumericLiteral { text: token.lexeme }),
            K::String(s) => ExprKind::Text(s),
            K::True => ExprKind::Bool(true),
            K::False => ExprKind::Bool(false),
            K::Null => ExprKind::Null,
            K::Identifier(s) if (s == "تحويل" || s == "مؤشر_فارغ") && self.typed_call_ahead() =>
            {
                self.pos += 1;
                let t = self.ty()?;
                self.expect(&K::Greater, "متوقع '>'")?;
                self.expect(&K::LeftParen, "متوقع '('")?;
                let k = if s == "تحويل" {
                    ExprKind::Cast(t, Box::new(self.expression()?))
                } else {
                    ExprKind::NullPointer(t)
                };
                self.expect(&K::RightParen, "متوقع ')'")?;
                k
            }
            K::Identifier(s) => ExprKind::Variable(s),
            K::LeftParen => {
                let e = self.expression()?;
                self.expect(&K::RightParen, "متوقع ')'")?;
                return Ok(Node {
                    span: self.span(start),
                    ..e
                });
            }
            K::LeftBracket => {
                let mut v = vec![];
                if !self.is(&K::RightBracket) {
                    loop {
                        v.push(self.expression()?);
                        if !self.take(&K::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&K::RightBracket, "متوقع ']'")?;
                ExprKind::List(v)
            }
            K::LeftBrace => {
                let mut v = vec![];
                if !self.is(&K::RightBrace) {
                    loop {
                        let k = self.string()?;
                        self.expect(&K::Colon, "متوقع ':'")?;
                        let e = self.expression()?;
                        v.push((k, e));
                        if !self.take(&K::Comma) {
                            break;
                        }
                    }
                }
                self.expect(&K::RightBrace, "متوقع '}'")?;
                ExprKind::Map(v)
            }
            _ => {
                self.pos -= 1;
                return Err(self.err("تعبير غير مكتمل أو رمز غير متوقع"));
            }
        };
        Ok(Node {
            kind,
            span: self.span(start),
        })
    }
}
