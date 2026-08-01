//! Minimal diagnostic type for Phase 1. The full teaching-quality
//! diagnostic system (error codes, `note:`/`help:` sections, `mtnc
//! explain`) is Document 22's scope and is deferred to Phase 23 of the
//! roadmap (Document 25 §2.3). For now this just needs to carry enough
//! information to prove the lexer's error-recovery requirement:
//! an invalid character produces a diagnostic and lexing continues,
//! it does not panic/abort (Document 17 §2, Document 25 Phase 1 exit
//! criteria).

use crate::token::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
}

impl Diagnostic {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Diagnostic { message: message.into(), span }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error: {} --> {}", self.message, self.span)
    }
}
