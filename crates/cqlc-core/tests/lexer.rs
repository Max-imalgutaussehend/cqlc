use cqlc_core::lexer::{TokenKind, lex};
use cqlc_core::span::{FileId, SourceFile};

fn kinds(src: &str) -> Vec<TokenKind> {
    let (lexed, diags) = lex(FileId(0), src);
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    lexed.tokens.into_iter().map(|t| t.kind).collect()
}

fn count_binds(src: &str) -> usize {
    kinds(src).iter().filter(|k| **k == TokenKind::Bind).count()
}

#[test]
fn question_mark_in_string_is_not_a_bind_marker() {
    let src = "SELECT username FROM users WHERE username = 'What?' AND id = ?";
    assert_eq!(count_binds(src), 1);
}

#[test]
fn question_mark_in_comments_is_ignored() {
    let src = "-- why?\nSELECT /* ? */ a FROM t // ?\n WHERE b = ?";
    assert_eq!(count_binds(src), 1);
    let (lexed, _) = lex(FileId(0), src);
    assert_eq!(lexed.comments.len(), 3);
}

#[test]
fn escaped_quotes_stay_inside_the_string() {
    assert_eq!(
        kinds("'It''s ?'")[0],
        TokenKind::String("It's ?".to_string())
    );
    assert_eq!(
        kinds("\"Odd \"\"Name\"\"\"")[0],
        TokenKind::QuotedIdent("Odd \"Name\"".to_string())
    );
}

#[test]
fn dollar_quoted_strings() {
    assert_eq!(kinds("$$a ? 'b'$$")[0], TokenKind::String("a ? 'b'".into()));
}

#[test]
fn uuid_numbers_and_blobs() {
    let k = kinds("123e4567-e89b-12d3-a456-426614174000 42 3.14 1e5 0xCAFE");
    assert_eq!(
        k[0],
        TokenKind::Uuid("123e4567-e89b-12d3-a456-426614174000".into())
    );
    assert_eq!(k[1], TokenKind::Number("42".into()));
    assert_eq!(k[2], TokenKind::Number("3.14".into()));
    assert_eq!(k[3], TokenKind::Number("1e5".into()));
    assert_eq!(k[4], TokenKind::Blob("0xCAFE".into()));
}

#[test]
fn operators() {
    let k = kinds("<= >= != < > =");
    assert_eq!(
        &k[..6],
        [
            TokenKind::Le,
            TokenKind::Ge,
            TokenKind::Ne,
            TokenKind::Lt,
            TokenKind::Gt,
            TokenKind::Eq
        ]
    );
}

#[test]
fn problems_become_diagnostics_and_lexing_continues() {
    for src in ["'open", "/* open", "$$open", "\"open", "a # b", "12abc"] {
        let (lexed, diags) = lex(FileId(0), src);
        assert_eq!(diags.len(), 1, "{src}");
        assert_eq!(diags[0].code.as_str(), "CQL001");
        assert_eq!(lexed.tokens.last().unwrap().kind, TokenKind::Eof);
    }
}

#[test]
fn line_and_column_are_one_based_and_count_characters() {
    let f = SourceFile::new("x.cql", "SELECT a\nFROM 'äö' t");
    assert_eq!(f.line_col(0), (1, 1));
    assert_eq!(f.line_col(9), (2, 1));
    // `t` follows a two-character (four-byte) string.
    let t = f.text.find(" t").unwrap() + 1;
    assert_eq!(f.line_col(t), (2, 11));
}
