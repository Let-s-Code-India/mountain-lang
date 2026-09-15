//! `mtnc` — the Mountain compiler.
//! Phase 6 scope (Document 25 §2.3) adds the Borrow Checker
//! (`borrow.rs`), implementing Document 6's ownership/borrowing/
//! lifetime rules as a standalone pass. See `borrow.rs`'s module doc
//! for exactly what it checks and its documented scope limits.

pub mod ast;
pub mod borrow;
pub mod diagnostics;
pub mod lexer;
pub mod manifest;
pub mod parser;
pub mod token;
pub mod types;
