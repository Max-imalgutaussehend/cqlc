#![no_main]
use cqlc_core::parser::parse_schema;
use cqlc_core::resolver::build_schema;
use cqlc_core::span::FileId;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|src: &str| {
    let (stmts, _) = parse_schema(FileId(0), src);
    let _ = build_schema(&stmts);
});
