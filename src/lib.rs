//! Convert [Pest](https://pest.rs/) and supported [PEG](https://en.wikipedia.org/wiki/Parsing_expression_grammar)
//! grammars into [Marser](https://crates.io/crates/marser) parser combinators.
//!
//! The CLI reads a grammar file and prints generated Rust source. The library API
//! exposes the same conversion pipeline for embedding in other tools.
//!
//! # Example
//!
//! ```
//! use grammar_to_marser::{convert_grammar_source, ConvertOptions};
//!
//! let rust = convert_grammar_source(
//!     r#"number = @{ ASCII_DIGIT+ }"#,
//!     &ConvertOptions {
//!         entry_rule: "number".to_string(),
//!         ..Default::default()
//!     },
//! )?;
//! assert!(rust.contains("pub fn grammar"));
//! # Ok::<(), Vec<grammar_to_marser::ConvertError>>(())
//! ```

mod ast;
mod codegen;
mod convert;
mod error;
mod export_templates;
mod expr;
mod grammar;
mod normalize;
mod output;
mod peg;
mod progress;
mod sample;
mod scc;
mod specialize;
mod syntax;
mod trivia;
mod validate;

pub use export_templates::{
    cargo_toml, default_sample_input, gitignore, lib_rs, main_rs, readme, rust_crate_ident,
    MARSER_VERSION,
};
pub use sample::{suggest_sample_from_table, suggest_sample_source};

pub use ast::*;
pub use convert::{
    convert_grammar, convert_grammar_source, convert_source, convert_with_warnings,
    list_grammar_rules, list_rules, ConvertOptions,
};
pub use error::{ConvertError, ConvertResult};
pub use expr::{Builtin, Expr, MatchingContext, SymKey};
pub use grammar::parse_pest_grammar;
pub use normalize::{RuleDef, RuleTable, build_rule_table, normalize_expression};
pub use peg::parse_peg_grammar;
pub use syntax::InputSyntax;
