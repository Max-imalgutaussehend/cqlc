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
