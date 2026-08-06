//! `mtnc` — the Mountain compiler.
//! Phase 1 scope (Document 25 §2.3): project scaffold, `mountain.toml`
//! parsing, CLI skeleton (`mtnc build`/`mtnc check`), and the full Lexer.

pub mod ast;
pub mod diagnostics;
pub mod lexer;
pub mod manifest;
pub mod parser;
pub mod token;
