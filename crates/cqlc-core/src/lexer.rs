//! CQL lexer. Produces tokens with spans and collects comments separately
//! (query annotations live in comments).

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::span::{FileId, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// Unquoted identifier or keyword, original case.
    Ident(String),
    /// `"Quoted Identifier"`, unescaped.
    QuotedIdent(String),
    /// `'string'` or `$$string$$`, unescaped.
    String(String),
    /// Integer or float literal, verbatim.
    Number(String),
    Uuid(String),
    /// `0x...` blob literal, verbatim.
    Blob(String),
    /// A positional bind marker `?`.
    Bind,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Semi,
    Dot,
    Star,
    Plus,
    Minus,
    Colon,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Eof,
}

impl TokenKind {
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Ident(s) => format!("`{s}`"),
            TokenKind::QuotedIdent(s) => format!("`\"{s}\"`"),
            TokenKind::String(_) => "a string literal".into(),
            TokenKind::Number(n) => format!("`{n}`"),
            TokenKind::Uuid(_) => "a uuid literal".into(),
            TokenKind::Blob(_) => "a blob literal".into(),
            TokenKind::Bind => "`?`".into(),
            TokenKind::LParen => "`(`".into(),
            TokenKind::RParen => "`)`".into(),
            TokenKind::LBracket => "`[`".into(),
            TokenKind::RBracket => "`]`".into(),
            TokenKind::LBrace => "`{`".into(),
            TokenKind::RBrace => "`}`".into(),
            TokenKind::Comma => "`,`".into(),
            TokenKind::Semi => "`;`".into(),
            TokenKind::Dot => "`.`".into(),
            TokenKind::Star => "`*`".into(),
            TokenKind::Plus => "`+`".into(),
            TokenKind::Minus => "`-`".into(),
            TokenKind::Colon => "`:`".into(),
            TokenKind::Eq => "`=`".into(),
            TokenKind::Ne => "`!=`".into(),
            TokenKind::Lt => "`<`".into(),
            TokenKind::Le => "`<=`".into(),
            TokenKind::Gt => "`>`".into(),
            TokenKind::Ge => "`>=`".into(),
            TokenKind::Eof => "end of input".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    /// Comment body without the `--`, `//` or `/* */` markers.
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug, Default)]
pub struct Lexed {
    /// Always terminated by a [`TokenKind::Eof`] token.
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
}

/// Lexes `src`. Never panics; problems are returned as diagnostics and
/// lexing continues after them.
pub fn lex(file: FileId, src: &str) -> (Lexed, Vec<Diagnostic>) {
    let b = src.as_bytes();
    let mut out = Lexed::default();
    let mut diags = Vec::new();
    let mut i = 0;
    let span = |s: usize, e: usize| Span::new(file, s, e);

    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        let start = i;
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'-' if next == Some(b'-') => {
                i = line_end(b, i);
                out.comments.push(Comment {
                    text: src[start + 2..i].to_string(),
                    span: span(start, i),
                });
            }
            b'/' if next == Some(b'/') => {
                i = line_end(b, i);
                out.comments.push(Comment {
                    text: src[start + 2..i].to_string(),
                    span: span(start, i),
                });
            }
            b'/' if next == Some(b'*') => match src[i + 2..].find("*/") {
                Some(off) => {
                    i = i + 2 + off + 2;
                    out.comments.push(Comment {
                        text: src[start + 2..i - 2].to_string(),
                        span: span(start, i),
                    });
                }
                None => {
                    i = b.len();
                    diags.push(Diagnostic::error(
                        DiagnosticCode::Syntax,
                        span(start, start + 2),
                        "unterminated block comment",
                    ));
                }
            },
            b'\'' => {
                i += 1;
                let mut value = String::new();
                let mut closed = false;
                while i < b.len() {
                    if b[i] == b'\'' {
                        if b.get(i + 1) == Some(&b'\'') {
                            value.push('\'');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        closed = true;
                        break;
                    }
                    let ch = src[i..].chars().next().unwrap();
                    value.push(ch);
                    i += ch.len_utf8();
                }
                if closed {
                    out.tokens.push(Token {
                        kind: TokenKind::String(value),
                        span: span(start, i),
                    });
                } else {
                    diags.push(Diagnostic::error(
                        DiagnosticCode::Syntax,
                        span(start, start + 1),
                        "unterminated string literal",
                    ));
                }
            }
            b'$' if next == Some(b'$') => match src[i + 2..].find("$$") {
                Some(off) => {
                    let value = src[i + 2..i + 2 + off].to_string();
                    i = i + 2 + off + 2;
                    out.tokens.push(Token {
                        kind: TokenKind::String(value),
                        span: span(start, i),
                    });
                }
                None => {
                    i = b.len();
                    diags.push(Diagnostic::error(
                        DiagnosticCode::Syntax,
                        span(start, start + 2),
                        "unterminated `$$` string literal",
                    ));
                }
            },
            b'"' => {
                i += 1;
                let mut value = String::new();
                let mut closed = false;
                while i < b.len() {
                    if b[i] == b'"' {
                        if b.get(i + 1) == Some(&b'"') {
                            value.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        closed = true;
                        break;
                    }
                    let ch = src[i..].chars().next().unwrap();
                    value.push(ch);
                    i += ch.len_utf8();
                }
                if !closed {
                    diags.push(Diagnostic::error(
                        DiagnosticCode::Syntax,
                        span(start, start + 1),
                        "unterminated quoted identifier",
                    ));
                } else if value.is_empty() {
                    diags.push(Diagnostic::error(
                        DiagnosticCode::Syntax,
                        span(start, i),
                        "empty quoted identifier",
                    ));
                } else {
                    out.tokens.push(Token {
                        kind: TokenKind::QuotedIdent(value),
                        span: span(start, i),
                    });
                }
            }
            b'?' => {
                i += 1;
                push(&mut out, TokenKind::Bind, span(start, i));
            }
            b'0' if matches!(next, Some(b'x' | b'X')) && is_uuid(b, i).is_none() => {
                i += 2;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
                push(
                    &mut out,
                    TokenKind::Blob(src[start..i].to_string()),
                    span(start, i),
                );
            }
            c if c.is_ascii_alphanumeric() || c == b'_' => {
                if let Some(len) = is_uuid(b, i) {
                    i += len;
                    push(
                        &mut out,
                        TokenKind::Uuid(src[start..i].to_string()),
                        span(start, i),
                    );
                } else if c.is_ascii_digit() {
                    i = lex_number(b, i);
                    if i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'_') {
                        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                            i += 1;
                        }
                        diags.push(Diagnostic::error(
                            DiagnosticCode::Syntax,
                            span(start, i),
                            format!("invalid number literal `{}`", &src[start..i]),
                        ));
                    } else {
                        push(
                            &mut out,
                            TokenKind::Number(src[start..i].to_string()),
                            span(start, i),
                        );
                    }
                } else {
                    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                        i += 1;
                    }
                    push(
                        &mut out,
                        TokenKind::Ident(src[start..i].to_string()),
                        span(start, i),
                    );
                }
            }
            _ => {
                let (kind, len) = match (c, next) {
                    (b'<', Some(b'=')) => (Some(TokenKind::Le), 2),
                    (b'>', Some(b'=')) => (Some(TokenKind::Ge), 2),
                    (b'!', Some(b'=')) => (Some(TokenKind::Ne), 2),
                    (b'(', _) => (Some(TokenKind::LParen), 1),
                    (b')', _) => (Some(TokenKind::RParen), 1),
                    (b'[', _) => (Some(TokenKind::LBracket), 1),
                    (b']', _) => (Some(TokenKind::RBracket), 1),
                    (b'{', _) => (Some(TokenKind::LBrace), 1),
                    (b'}', _) => (Some(TokenKind::RBrace), 1),
                    (b',', _) => (Some(TokenKind::Comma), 1),
                    (b';', _) => (Some(TokenKind::Semi), 1),
                    (b'.', _) => (Some(TokenKind::Dot), 1),
                    (b'*', _) => (Some(TokenKind::Star), 1),
                    (b'+', _) => (Some(TokenKind::Plus), 1),
                    (b'-', _) => (Some(TokenKind::Minus), 1),
                    (b':', _) => (Some(TokenKind::Colon), 1),
                    (b'=', _) => (Some(TokenKind::Eq), 1),
                    (b'<', _) => (Some(TokenKind::Lt), 1),
                    (b'>', _) => (Some(TokenKind::Gt), 1),
                    _ => (None, src[i..].chars().next().map_or(1, char::len_utf8)),
                };
                i += len;
                match kind {
                    Some(kind) => push(&mut out, kind, span(start, i)),
                    None => diags.push(Diagnostic::error(
                        DiagnosticCode::Syntax,
                        span(start, i),
                        format!("unexpected character `{}`", &src[start..i]),
                    )),
                }
            }
        }
    }
    out.tokens.push(Token {
        kind: TokenKind::Eof,
        span: span(b.len(), b.len()),
    });
    (out, diags)
}

fn push(out: &mut Lexed, kind: TokenKind, span: Span) {
    out.tokens.push(Token { kind, span });
}

fn line_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i] != b'\n' {
        i += 1;
    }
    i
}

fn lex_number(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if b.get(i) == Some(&b'.') && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if b.get(j).is_some_and(u8::is_ascii_digit) {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    i
}

/// Length (36) if a `8-4-4-4-12` hex uuid starts at `i`.
fn is_uuid(b: &[u8], i: usize) -> Option<usize> {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let mut p = i;
    for (g, len) in GROUPS.iter().enumerate() {
        if g > 0 {
            if b.get(p) != Some(&b'-') {
                return None;
            }
            p += 1;
        }
        for _ in 0..*len {
            if !b.get(p).is_some_and(u8::is_ascii_hexdigit) {
                return None;
            }
            p += 1;
        }
    }
    if b.get(p)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    {
        return None;
    }
    Some(p - i)
}
