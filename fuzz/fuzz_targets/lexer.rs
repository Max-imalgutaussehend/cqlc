#![no_main]
use cqlc_core::lexer::lex;
use cqlc_core::span::FileId;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|src: &str| {
    let (lexed, _) = lex(FileId(0), src);
    // Spans must stay inside the input and be ordered.
    let mut end = 0;
    for t in &lexed.tokens {
        assert!(t.span.start <= t.span.end && t.span.end as usize <= src.len());
        assert!(t.span.start >= end || t.span.start == t.span.end);
        end = end.max(t.span.end);
    }
});
