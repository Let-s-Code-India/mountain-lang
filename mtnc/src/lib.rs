//! `mtnc` — the Mountain compiler.
//! Phase 8 scope (Document 25 §2.3) adds the exhaustiveness checker
//! (`exhaustive.rs`, wired into `types::TypeChecker::check_match`) and
//! full control-flow support (loop labels, generator lowering). See
//! each module's own doc comment for exactly what it checks.

pub mod ast;
pub mod borrow;
pub mod diagnostics;
pub mod exhaustive;
pub mod generator;
pub mod lexer;
pub mod manifest;
pub mod parser;
pub mod token;
pub mod types;
