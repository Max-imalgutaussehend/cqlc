//! Rust code generator for cqlc. Consumes the typed IR from `cqlc-core`.
//!
//! # Stability
//!
//! [`generate_rust`], [`Options`], [`TimeLibrary`] and [`OUTPUT_FORMAT`] are
//! the supported API. The layout of the generated code is versioned by
//! [`OUTPUT_FORMAT`]: it changes only in a new major version of cqlc.
//!
//! ```
//! use cqlc_codegen::{generate_rust, Options};
//! use cqlc_core::{compile, Input};
//!
//! let c = compile(
//!     &[Input::new("s.cql", "CREATE TABLE t (k uuid PRIMARY KEY, v text);")],
//!     &[Input::new("q.cql", "-- name: Get :one\nSELECT v FROM t WHERE k = ?;")],
//! );
//! let code = generate_rust(&c.ir, &Options::new()).unwrap();
//! assert!(code.contains("pub async fn get("));
//! ```

#![warn(missing_docs)]

mod emit;
#[doc(hidden)]
pub mod naming;
mod rust;

pub use rust::{OUTPUT_FORMAT, Options, TimeLibrary, generate_rust};
