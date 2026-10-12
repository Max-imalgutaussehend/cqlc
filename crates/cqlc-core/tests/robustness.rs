//! The front end must never panic, whatever it is fed.

use cqlc_core::lexer::lex;
use cqlc_core::parser::{parse_queries, parse_schema};
use cqlc_core::span::FileId;
use proptest::prelude::*;

proptest! {
    #[test]
    fn arbitrary_text_never_panics(src in "\\PC{0,200}") {
        let _ = lex(FileId(0), &src);
        let _ = parse_schema(FileId(0), &src);
        let _ = parse_queries(FileId(0), &src);
    }

    #[test]
    fn cql_like_token_soup_never_panics(
        parts in proptest::collection::vec(
            prop_oneof![
                Just("SELECT"), Just("FROM"), Just("WHERE"), Just("INSERT INTO"), Just("VALUES"),
                Just("UPDATE"), Just("SET"), Just("DELETE"), Just("CREATE TABLE"), Just("PRIMARY KEY"),
                Just("("), Just(")"), Just(","), Just(";"), Just("?"), Just("="), Just("'a'"),
                Just("-- name: X :one\n"), Just("t"), Just("a"), Just("IN"), Just("AND"), Just("<"),
                Just("map<text,"), Just(">"), Just("{"), Just("}"), Just("$$"), Just("\"")
            ],
            0..40,
        )
    ) {
        let src = parts.join(" ");
        let _ = parse_schema(FileId(0), &src);
        let _ = parse_queries(FileId(0), &src);
    }
}

/// Found by fuzzing: a span starting inside a run of `\r` (which `line_text`
/// trims) used to make `render` panic in `clamp`.
#[test]
fn rendering_a_span_inside_trailing_carriage_returns() {
    use cqlc_core::diagnostic::{Diagnostic, DiagnosticCode};
    use cqlc_core::span::{SourceMap, Span};
    let mut map = SourceMap::default();
    let id = map.add("x.cql", "ab\r\r\r\ncd\r");
    for (start, end) in [(2, 5), (3, 4), (5, 5), (0, 99), (8, 9), (9, 9)] {
        let d = Diagnostic::error(DiagnosticCode::Syntax, Span::new(id, start, end), "x");
        let out = d.render(&map);
        assert!(out.contains("error[CQL001]"));
    }
}

#[test]
fn nul_and_control_bytes_do_not_break_the_pipeline() {
    use cqlc_core::{Input, compile};
    let junk = "UPDATE\0\0\0\0\u{1}\0SELUPDATE\r\r\r\r\r\u{f}\u{f}\r";
    let c = compile(
        &[Input::new("s.cql", "CREATE TABLE t (k int PRIMARY KEY);")],
        &[Input::new("q.cql", format!("-- name: Q :one\n{junk}"))],
    );
    for d in &c.diagnostics {
        let _ = d.render(&c.sources);
    }
}

fn compile_query(query: &str) -> cqlc_core::Compilation {
    use cqlc_core::{Input, compile};
    compile(
        &[Input::new(
            "s.cql",
            "CREATE TABLE t (k int PRIMARY KEY, v text, l list<int>);",
        )],
        &[Input::new("q.cql", format!("-- name: Q :exec\n{query}"))],
    )
}

/// Found by scanning the code: nesting used to be unbounded recursion and a
/// crafted file crashed the process with a stack overflow.
#[test]
fn deeply_nested_input_is_an_error_not_a_crash() {
    let n = 100_000;
    for (open, close) in [("[", "]"), ("(", ")"), ("{", "}"), ("{a:", "}")] {
        let q = format!(
            "INSERT INTO t (k, v) VALUES (1, {}{});",
            open.repeat(n),
            close.repeat(n)
        );
        let c = compile_query(&q);
        assert!(
            c.diagnostics
                .iter()
                .any(|d| d.message.contains("nested too deeply")),
            "{open}: {:?}",
            c.diagnostics.first()
        );
    }
    // The same holds for types in DDL.
    let ddl = format!(
        "CREATE TABLE u (k int PRIMARY KEY, v {}int{});",
        "list<".repeat(n),
        ">".repeat(n)
    );
    let c = cqlc_core::compile(&[cqlc_core::Input::new("s.cql", ddl)], &[]);
    assert!(
        c.diagnostics
            .iter()
            .any(|d| d.message.contains("nested too deeply"))
    );
}

#[test]
fn reasonable_nesting_is_not_limited() {
    // 30 levels is far more than any real query; only the types do not fit.
    let q = format!(
        "INSERT INTO t (k, l) VALUES (1, {}1{});",
        "[".repeat(30),
        "]".repeat(30)
    );
    let c = compile_query(&q);
    assert!(
        !c.diagnostics
            .iter()
            .any(|d| d.message.contains("nested too deeply"))
    );
    let q = "INSERT INTO t (k, l) VALUES (1, [1, 2, 3]);";
    assert!(!compile_query(q).has_errors());
}

#[test]
fn a_leading_byte_order_mark_is_ignored() {
    use cqlc_core::lexer::{TokenKind, lex};
    use cqlc_core::span::FileId;
    let (lexed, diags) = lex(FileId(0), "\u{feff}SELECT");
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(lexed.tokens[0].kind, TokenKind::Ident("SELECT".into()));
    // Spans still index the original text.
    assert_eq!(lexed.tokens[0].span.start, 3);
}

#[test]
fn diagnostics_on_huge_lines_stay_small() {
    let q = format!(
        "INSERT INTO t (k, v) VALUES (1, {}{});",
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    let c = compile_query(&q);
    let rendered = c.diagnostics[0].render(&c.sources);
    assert!(rendered.len() < 1_000, "{} bytes", rendered.len());
    assert!(rendered.contains('^') && rendered.contains('…'));
}
