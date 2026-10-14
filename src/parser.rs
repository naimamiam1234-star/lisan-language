use crate::ast::{Expr, Function, Op, Stmt};
use crate::lexer::{Token, TokenKind};
use std::mem::discriminant;

pub fn parse(tokens: Vec<Token>) -> Result<Vec<Stmt>, String> {
    Parser { tokens, cursor: 0 }.program()
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn current(&self) -> &Token { &self.tokens[self.cursor] }
    fn is(&self, kind: &TokenKind) -> bool {
        discriminant(&self.current().kind) == discriminant(kind)
    }
    fn take(&mut self, kind: &TokenKind) -> bool {
        if self.is(kind) { self.cursor += 1; true } else { false }
    }
    fn error(&self, reason: &str) -> String {
        format!("السطر {}، العمود {}: {}", self.current().line, self.current().column, reason)
    }
    fn expect(&mut self, kind: &TokenKind, reason: &str) -> Result<(), String> {
        if self.take(kind) { Ok(()) } else { Err(self.error(reason)) }
    }
    fn identifier(&mut self) -> Result<String, String> {
        match self.current().kind.clone() {
            TokenKind::Identifier(name) => { self.cursor += 1; Ok(name) }
            _ => Err(self.error("متوقع اسم عربي لمتغير أو دالة")),
        }
    }
    fn program(&mut self) -> Result<Vec<Stmt>, String> {
        let mut statements = Vec::new();
        while !self.is(&TokenKind::Eof) {
            statements.push(self.statement()?);
        }
        Ok(statements)
    }
    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&TokenKind::LeftBrace, "متوقع '{' لبدء الكتلة")?;
        let mut statements = Vec::new();
        while !self.is(&TokenKind::RightBrace) && !self.is(&TokenKind::Eof) {
            statements.push(self.statement()?);
        }
        self.expect(&TokenKind::RightBrace, "متوقع '}' لإنهاء الكتلة")?;
        Ok(statements)
    }
    fn statement(&mut self) -> Result<Stmt, String> {
        if self.take(&TokenKind::Function) {
            let name = self.identifier()?;
            self.expect(&TokenKind::LeftParen, "متوقع '(' بعد اسم الدالة")?;
            let mut params = Vec::new();
            if !self.is(&TokenKind::RightParen) {
                loop {
                    let name = self.identifier()?;
                    if params.contains(&name) { return Err(self.error("معامل مكرر في الدالة")); }
                    params.push(name);
                    if !self.take(&TokenKind::Comma) { break; }
                }
            }
            self.expect(&TokenKind::RightParen, "متوقع ')' بعد معاملات الدالة")?;
            return Ok(Stmt::Function(Function { name, params, body: self.block()? }));
        }
        if self.take(&TokenKind::Let) {
            let name = self.identifier()?;
            self.expect(&TokenKind::Equal, "متوقع '=' لتعريف المتغير")?;
            let value = self.expression()?;
            self.semicolon()?;
            return Ok(Stmt::Let(name, value));
        }
        if self.take(&TokenKind::Print) {
            self.expect(&TokenKind::LeftParen, "متوقع '(' بعد اطبع")?;
            let expr = self.expression()?;
            self.expect(&TokenKind::RightParen, "متوقع ')' بعد اطبع")?;
            self.semicolon()?;
            return Ok(Stmt::Print(expr));
        }
        if self.take(&TokenKind::If) {
            self.expect(&TokenKind::LeftParen, "متوقع '(' بعد إذا")?;
            let cond = self.expression()?;
            self.expect(&TokenKind::RightParen, "متوقع ')' بعد الشرط")?;
            let yes = self.block()?;
            let no = if self.take(&TokenKind::Else) { Some(self.block()?) } else { None };
            return Ok(Stmt::If(cond, yes, no));
        }
        if self.take(&TokenKind::ForEach) {
            self.expect(&TokenKind::LeftParen, "متوقع '(' بعد لكل")?;
            let name = self.identifier()?;
            self.expect(&TokenKind::In, "متوقع 'في' داخل حلقة لكل")?;
            let iterable = self.expression()?;
            self.expect(&TokenKind::RightParen, "متوقع ')' بعد تعبير لكل")?;
            return Ok(Stmt::ForEach(name, iterable, self.block()?));
        }
        if self.take(&TokenKind::While) {
            self.expect(&TokenKind::LeftParen, "متوقع '(' بعد طالما")?;
            let cond = self.expression()?;
            self.expect(&TokenKind::RightParen, "متوقع ')' بعد الشرط")?;
            return Ok(Stmt::While(cond, self.block()?));
        }
        if self.take(&TokenKind::Return) {
            let val = if self.is(&TokenKind::Semicolon) { None } else { Some(self.expression()?) };
            self.semicolon()?;
            return Ok(Stmt::Return(val));
        }
        if self.is(&TokenKind::LeftBrace) { return Ok(Stmt::Block(self.block()?)); }
        let expr = self.expression()?;
        self.semicolon()?;
        Ok(Stmt::Expr(expr))
    }
    fn semicolon(&mut self) -> Result<(), String> {
        self.expect(&TokenKind::Semicolon, "متوقع ';' أو '؛' في نهاية التعليمة")
    }
    fn expression(&mut self) -> Result<Expr, String> { self.assignment() }
    fn assignment(&mut self) -> Result<Expr, String> {
        let lhs = self.or()?;
        if self.take(&TokenKind::Equal) {
            let rhs = self.assignment()?;
            match lhs {
                Expr::Variable(name) => Ok(Expr::Assign(name, Box::new(rhs))),
                Expr::Index(target, index) => match *target {
                    Expr::Variable(name) => Ok(Expr::AssignIndex(name, index, Box::new(rhs))),
                    _ => Err(self.error("إسناد عناصر القائمة يتطلب متغيرًا مباشرًا")),
                },
                _ => Err(self.error("الطرف الأيسر للإسناد يجب أن يكون اسم متغير")),
            }
        } else { Ok(lhs) }
    }
    fn or(&mut self) -> Result<Expr, String> {
        let mut expr = self.and()?;
        while self.take(&TokenKind::OrOr) {
            expr = Expr::Binary(Box::new(expr), Op::Or, Box::new(self.and()?));
        }
        Ok(expr)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut expr = self.equality()?;
        while self.take(&TokenKind::AndAnd) {
            expr = Expr::Binary(Box::new(expr), Op::And, Box::new(self.equality()?));
        }
        Ok(expr)
    }
    fn equality(&mut self) -> Result<Expr, String> {
        let mut expr = self.comparison()?;
        loop {
            let op = if self.take(&TokenKind::EqualEqual) { Op::Equal }
                else if self.take(&TokenKind::BangEqual) { Op::NotEqual }
                else { break };
            expr = Expr::Binary(Box::new(expr), op, Box::new(self.comparison()?));
        }
        Ok(expr)
    }
    fn comparison(&mut self) -> Result<Expr, String> {
        let mut expr = self.term()?;
        loop {
            let op = if self.take(&TokenKind::Less) { Op::Less }
                else if self.take(&TokenKind::LessEqual) { Op::LessEqual }
                else if self.take(&TokenKind::Greater) { Op::Greater }
                else if self.take(&TokenKind::GreaterEqual) { Op::GreaterEqual }
                else { break };
            expr = Expr::Binary(Box::new(expr), op, Box::new(self.term()?));
        }
        Ok(expr)
    }
    fn term(&mut self) -> Result<Expr, String> {
        let mut expr = self.factor()?;
        loop {
            let op = if self.take(&TokenKind::Plus) { Op::Add }
                else if self.take(&TokenKind::Minus) { Op::Subtract }
                else { break };
            expr = Expr::Binary(Box::new(expr), op, Box::new(self.factor()?));
        }
        Ok(expr)
    }
    fn factor(&mut self) -> Result<Expr, String> {
        let mut expr = self.unary()?;
        loop {
            let op = if self.take(&TokenKind::Star) { Op::Multiply }
                else if self.take(&TokenKind::Slash) { Op::Divide }
                else if self.take(&TokenKind::Percent) { Op::Modulo }
                else { break };
            expr = Expr::Binary(Box::new(expr), op, Box::new(self.unary()?));
        }
        Ok(expr)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.take(&TokenKind::Bang) { return Ok(Expr::Unary(Op::Not, Box::new(self.unary()?))); }
        if self.take(&TokenKind::Minus) { return Ok(Expr::Unary(Op::Negate, Box::new(self.unary()?))); }
        self.call()
    }
    fn call(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;
        loop {
            if self.take(&TokenKind::LeftBracket) {
                let index = self.expression()?;
                self.expect(&TokenKind::RightBracket, "متوقع ']' بعد الفهرس")?;
                expr = Expr::Index(Box::new(expr), Box::new(index));
                continue;
            }
            if !self.take(&TokenKind::LeftParen) { break; }
            let mut args = Vec::new();
            if !self.is(&TokenKind::RightParen) {
                loop {
                    args.push(self.expression()?);
                    if !self.take(&TokenKind::Comma) { break; }
                }
            }
            self.expect(&TokenKind::RightParen, "متوقع ')' بعد وسائط الاستدعاء")?;
            expr = match expr {
                Expr::Variable(name) => Expr::Call(name, args),
                _ => return Err(self.error("الاستدعاء يحتاج إلى اسم دالة")),
            };
        }
        Ok(expr)
    }
    fn primary(&mut self) -> Result<Expr, String> {
        let kind = self.current().kind.clone();
        match kind {
            TokenKind::Number(n) => { self.cursor += 1; Ok(Expr::Number(n)) }
            TokenKind::String(s) => { self.cursor += 1; Ok(Expr::Text(s)) }
            TokenKind::True => { self.cursor += 1; Ok(Expr::Bool(true)) }
            TokenKind::False => { self.cursor += 1; Ok(Expr::Bool(false)) }
            TokenKind::Null => { self.cursor += 1; Ok(Expr::Null) }
            TokenKind::LeftBracket => {
                self.cursor += 1;
                let mut items = Vec::new();
                if !self.is(&TokenKind::RightBracket) {
                    loop {
                        items.push(self.expression()?);
                        if !self.take(&TokenKind::Comma) { break; }
                    }
                }
                self.expect(&TokenKind::RightBracket, "متوقع ']' لإنهاء القائمة")?;
                Ok(Expr::List(items))
            }
            TokenKind::Identifier(s) => { self.cursor += 1; Ok(Expr::Variable(s)) }
            TokenKind::LeftParen => {
                self.cursor += 1;
                let expr = self.expression()?;
                self.expect(&TokenKind::RightParen, "متوقع ')' بعد التعبير")?;
                Ok(expr)
            }
            _ => Err(self.error("تعبير غير مكتمل أو رمز غير متوقع")),
        }
    }
}
