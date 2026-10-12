//! Code generators for cqlc. Consumes the typed IR from `cqlc-core`.

mod emit;
pub mod naming;
pub mod rust;

pub use rust::{Options, TimeLibrary, generate_rust};
