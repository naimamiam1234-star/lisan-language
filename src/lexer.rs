#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Let, Print, If, Else, While, ForEach, In, Function, Return, True, False, Null,
    Identifier(String), Number(f64), String(String),
    LeftParen, RightParen, LeftBrace, RightBrace, LeftBracket, RightBracket, Comma, Semicolon,
    Plus, Minus, Star, Slash, Percent,
    Equal, EqualEqual, Bang, BangEqual,
    Less, LessEqual, Greater, GreaterEqual, AndAnd, OrOr, Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

pub fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0;
    let mut line = 1;
    let mut column = 1;
    let mut result = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            i += 1;
            line += 1;
            column = 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            column += 1;
            continue;
        }
        if c == '#' || (c == '/' && chars.get(i + 1) == Some(&'/')) {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
                column += 1;
            }
            continue;
        }

        let start_line = line;
        let start_col = column;
        let start = i;
        let kind = match c {
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '{' => TokenKind::LeftBrace,
            '}' => TokenKind::RightBrace,
            '[' => TokenKind::LeftBracket,
            ']' => TokenKind::RightBracket,
            ',' | '،' => TokenKind::Comma,
            ';' | '؛' => TokenKind::Semicolon,
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            '=' if chars.get(i + 1) == Some(&'=') => { i += 1; column += 1; TokenKind::EqualEqual }
            '=' => TokenKind::Equal,
            '!' if chars.get(i + 1) == Some(&'=') => { i += 1; column += 1; TokenKind::BangEqual }
            '!' => TokenKind::Bang,
            '<' if chars.get(i + 1) == Some(&'=') => { i += 1; column += 1; TokenKind::LessEqual }
            '<' => TokenKind::Less,
            '>' if chars.get(i + 1) == Some(&'=') => { i += 1; column += 1; TokenKind::GreaterEqual }
            '>' => TokenKind::Greater,
            '&' if chars.get(i + 1) == Some(&'&') => { i += 1; column += 1; TokenKind::AndAnd }
            '|' if chars.get(i + 1) == Some(&'|') => { i += 1; column += 1; TokenKind::OrOr }
            '"' => {
                i += 1;
                column += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < chars.len() {
                    let x = chars[i];
                    if x == '"' { closed = true; break; }
                    if x == '\n' {
                        return Err(format!("السطر {}، العمود {}: نص غير مغلق", start_line, start_col));
                    }
                    if x == '\\' {
                        i += 1;
                        column += 1;
                        let escaped = chars.get(i).ok_or_else(|| format!("السطر {}: تسلسل هروب غير مكتمل", start_line))?;
                        s.push(match escaped {
                            'n' => '\n', 't' => '\t', 'r' => '\r', '"' => '"', '\\' => '\\',
                            _ => return Err(format!("السطر {}: تسلسل هروب غير معروف", start_line)),
                        });
                    } else { s.push(x); }
                    i += 1;
                    column += 1;
                }
                if !closed {
                    return Err(format!("السطر {}، العمود {}: نص غير مغلق", start_line, start_col));
                }
                TokenKind::String(s)
            }
            _ if is_digit(c) => {
                let mut num = String::new();
                while i < chars.len() && is_digit(chars[i]) {
                    num.push(to_ascii_digit(chars[i]).unwrap());
                    i += 1;
                    column += 1;
                }
                if (chars.get(i) == Some(&'.') || chars.get(i) == Some(&'٫'))
                    && chars.get(i + 1).copied().map(is_digit).unwrap_or(false)
                {
                    num.push('.');
                    i += 1;
                    column += 1;
                    while i < chars.len() && is_digit(chars[i]) {
                        num.push(to_ascii_digit(chars[i]).unwrap());
                        i += 1;
                        column += 1;
                    }
                }
                i -= 1;
                column -= 1;
                TokenKind::Number(num.parse().map_err(|_| format!("السطر {}: رقم غير صحيح", start_line))?)
            }
            _ if is_ident_start(c) => {
                while i + 1 < chars.len() && is_ident_continue(chars[i + 1]) {
                    i += 1;
                    column += 1;
                }
                let word: String = chars[start..=i].iter().collect();
                match word.as_str() {
                    "دع" => TokenKind::Let,
                    "اطبع" => TokenKind::Print,
                    "إذا" | "اذا" => TokenKind::If,
                    "وإلا" | "والا" => TokenKind::Else,
                    "طالما" => TokenKind::While,
                    "لكل" => TokenKind::ForEach,
                    "في" => TokenKind::In,
                    "دالة" => TokenKind::Function,
                    "أرجع" | "ارجع" => TokenKind::Return,
                    "صحيح" => TokenKind::True,
                    "خطأ" | "خطا" => TokenKind::False,
                    "لاشيء" => TokenKind::Null,
                    "و" => TokenKind::AndAnd,
                    "أو" | "او" => TokenKind::OrOr,
                    "ليس" => TokenKind::Bang,
                    _ => TokenKind::Identifier(word),
                }
            }
            _ => return Err(format!("السطر {}، العمود {}: رمز غير معروف '{}'", line, column, c)),
        };
        result.push(Token { kind, line: start_line, column: start_col });
        i += 1;
        column += 1;
    }
    result.push(Token { kind: TokenKind::Eof, line, column });
    Ok(result)
}

fn to_ascii_digit(c: char) -> Option<char> {
    match c {
        '0'..='9' => Some(c),
        '٠'..='٩' => Some((b'0' + (c as u32 - '٠' as u32) as u8) as char),
        '۰'..='۹' => Some((b'0' + (c as u32 - '۰' as u32) as u8) as char),
        _ => None,
    }
}
fn is_digit(c: char) -> bool { to_ascii_digit(c).is_some() }
fn is_ident_start(c: char) -> bool { c.is_alphabetic() || c == '_' }
fn is_ident_continue(c: char) -> bool { c.is_alphanumeric() || c == '_' }
