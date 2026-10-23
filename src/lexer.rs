use crate::{
    diagnostic::Diagnostic,
    source::{SourceId, Span},
};
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Let,
    Print,
    If,
    Else,
    While,
    ForEach,
    In,
    Function,
    Return,
    Import,
    Break,
    Continue,
    True,
    False,
    Null,
    Identifier(String),
    Number(f64),
    String(String),
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Comma,
    Colon,
    Semicolon,
    Plus,
    Minus,
    Star,
    Power,
    Slash,
    Percent,
    Equal,
    EqualEqual,
    Bang,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    AndAnd,
    OrOr,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct LocatedToken {
    pub kind: TokenKind,
    pub span: Span,
    pub lexeme: String,
}

pub fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let mut last = 0;
    let mut line = 1;
    let mut column = 1;
    tokenize_impl(source, SourceId(0))
        .map(|v| {
            v.into_iter()
                .map(|t| {
                    for ch in source[last..t.span.start].chars() {
                        if ch == '\n' {
                            line += 1;
                            column = 1;
                        } else {
                            column += 1;
                        }
                    }
                    last = t.span.start;
                    Token {
                        kind: t.kind,
                        line,
                        column,
                    }
                })
                .collect()
        })
        .map_err(|error| error.message)
}
pub fn tokenize_located(
    source: &str,
    source_id: SourceId,
) -> Result<Vec<LocatedToken>, Diagnostic> {
    tokenize_impl(source, source_id)
}
fn tokenize_impl(source: &str, source_id: SourceId) -> Result<Vec<LocatedToken>, Diagnostic> {
    let chars: Vec<char> = source.chars().collect();
    let offsets: Vec<usize> = source
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(source.len()))
        .collect();
    let failure = |start: usize, end: usize, message: String| {
        Diagnostic::new(
            "E_PARSE",
            Span {
                source: source_id,
                start: offsets[start],
                end: offsets[end],
            },
            message,
        )
    };
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
            ':' => TokenKind::Colon,
            ';' | '؛' => TokenKind::Semicolon,
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' if chars.get(i + 1) == Some(&'*') => {
                i += 1;
                column += 1;
                TokenKind::Power
            }
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            '=' if chars.get(i + 1) == Some(&'=') => {
                i += 1;
                column += 1;
                TokenKind::EqualEqual
            }
            '=' => TokenKind::Equal,
            '!' if chars.get(i + 1) == Some(&'=') => {
                i += 1;
                column += 1;
                TokenKind::BangEqual
            }
            '!' => TokenKind::Bang,
            '<' if chars.get(i + 1) == Some(&'=') => {
                i += 1;
                column += 1;
                TokenKind::LessEqual
            }
            '<' => TokenKind::Less,
            '>' if chars.get(i + 1) == Some(&'=') => {
                i += 1;
                column += 1;
                TokenKind::GreaterEqual
            }
            '>' => TokenKind::Greater,
            '&' if chars.get(i + 1) == Some(&'&') => {
                i += 1;
                column += 1;
                TokenKind::AndAnd
            }
            '|' if chars.get(i + 1) == Some(&'|') => {
                i += 1;
                column += 1;
                TokenKind::OrOr
            }
            '"' => {
                i += 1;
                column += 1;
                let mut s = String::new();
                let mut closed = false;
                while i < chars.len() {
                    let x = chars[i];
                    if x == '"' {
                        closed = true;
                        break;
                    }
                    if x == '\n' {
                        return Err(failure(
                            start,
                            i,
                            format!("السطر {}، العمود {}: نص غير مغلق", start_line, start_col),
                        ));
                    }
                    if x == '\\' {
                        i += 1;
                        column += 1;
                        let escaped = chars.get(i).ok_or_else(|| {
                            failure(i, i, format!("السطر {}: تسلسل هروب غير مكتمل", start_line))
                        })?;
                        s.push(match escaped {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            '"' => '"',
                            '\\' => '\\',
                            _ => {
                                return Err(failure(
                                    i,
                                    i + 1,
                                    format!("السطر {}: تسلسل هروب غير معروف", start_line),
                                ))
                            }
                        });
                    } else {
                        s.push(x);
                    }
                    i += 1;
                    column += 1;
                }
                if !closed {
                    return Err(failure(
                        start,
                        i,
                        format!("السطر {}، العمود {}: نص غير مغلق", start_line, start_col),
                    ));
                }
                TokenKind::String(s)
            }
            _ if c.is_numeric() && !c.is_ascii_digit() => {
                return Err(failure(
                    i,
                    i + 1,
                    format!(
                        "السطر {}، العمود {}: استخدم الأرقام اللاتينية 0-9 فقط",
                        line, column
                    ),
                ));
            }
            _ if c.is_ascii_digit() => {
                let mut num = String::new();
                while i < chars.len() && chars[i].is_ascii_digit() {
                    num.push(chars[i]);
                    i += 1;
                    column += 1;
                }
                if chars.get(i) == Some(&'.')
                    && chars
                        .get(i + 1)
                        .copied()
                        .map(|x| x.is_ascii_digit())
                        .unwrap_or(false)
                {
                    num.push('.');
                    i += 1;
                    column += 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        num.push(chars[i]);
                        i += 1;
                        column += 1;
                    }
                }
                if matches!(chars.get(i).copied(), Some('e' | 'E')) {
                    num.push(chars[i]);
                    i += 1;
                    column += 1;
                    if matches!(chars.get(i).copied(), Some('+' | '-')) {
                        num.push(chars[i]);
                        i += 1;
                        column += 1;
                    }
                    if !chars
                        .get(i)
                        .copied()
                        .map(|c| c.is_ascii_digit())
                        .unwrap_or(false)
                    {
                        return Err(failure(
                            start,
                            i,
                            format!(
                                "السطر {}، العمود {}: أس العدد العلمي غير مكتمل",
                                start_line, start_col
                            ),
                        ));
                    }
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        num.push(chars[i]);
                        i += 1;
                        column += 1;
                    }
                }
                i -= 1;
                column -= 1;
                let parsed: f64 = num.parse().map_err(|_| {
                    failure(start, i + 1, format!("السطر {}: رقم غير صحيح", start_line))
                })?;
                if !parsed.is_finite() {
                    return Err(failure(
                        start,
                        i + 1,
                        format!("السطر {}: الرقم خارج النطاق المسموح", start_line),
                    ));
                }
                TokenKind::Number(parsed)
            }
            _ if is_ident_start(c) => {
                while i + 1 < chars.len() && is_ident_continue(chars[i + 1]) {
                    i += 1;
                    column += 1;
                }
                let word: String = chars[start..=i].iter().collect();
                match word.as_str() {
                    "دع" | "ليكن" | "متغير" => TokenKind::Let,
                    "اطبع" => TokenKind::Print,
                    "إذا" | "اذا" => TokenKind::If,
                    "وإلا" | "والا" => TokenKind::Else,
                    "طالما" | "بينما" => TokenKind::While,
                    "لكل" => TokenKind::ForEach,
                    "في" => TokenKind::In,
                    "دالة" => TokenKind::Function,
                    "استورد" => TokenKind::Import,
                    "أرجع" | "ارجع" => TokenKind::Return,
                    "توقف" => TokenKind::Break,
                    "تابع" => TokenKind::Continue,
                    "صحيح" => TokenKind::True,
                    "خطأ" | "خطا" => TokenKind::False,
                    "لاشيء" => TokenKind::Null,
                    "و" => TokenKind::AndAnd,
                    "أو" | "او" => TokenKind::OrOr,
                    "ليس" => TokenKind::Bang,
                    _ => TokenKind::Identifier(word),
                }
            }
            _ => {
                return Err(failure(
                    i,
                    i + 1,
                    format!("السطر {}، العمود {}: رمز غير معروف '{}'", line, column, c),
                ))
            }
        };
        let span = Span {
            source: source_id,
            start: offsets[start],
            end: offsets[i + 1],
        };
        result.push(LocatedToken {
            kind,
            span,
            lexeme: source[span.start..span.end].to_string(),
        });
        i += 1;
        column += 1;
    }
    result.push(LocatedToken {
        kind: TokenKind::Eof,
        span: Span {
            source: source_id,
            start: source.len(),
            end: source.len(),
        },
        lexeme: String::new(),
    });
    Ok(result)
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}
fn is_ident_continue(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || matches!(c, '\u{064b}'..='\u{065f}')
}
