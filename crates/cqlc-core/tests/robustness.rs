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
