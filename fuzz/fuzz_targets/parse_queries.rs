#![no_main]
use cqlc_core::parser::parse_queries;
use cqlc_core::span::FileId;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|src: &str| {
    let _ = parse_queries(FileId(0), src);
});
