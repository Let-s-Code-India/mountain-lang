//! `Atomic<T>` and `Ordering` (Document 14 §1, §2, §9.4; Phase 13 M1): the parts shared by the type
//! checker and code generation.

use crate::ast::Expr;
use crate::types::Ty;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ord5 {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}

impl Ord5 {
    pub fn name(self) -> &'static str {
        match self {
            Ord5::Relaxed => "Relaxed",
            Ord5::Acquire => "Acquire",
            Ord5::Release => "Release",
            Ord5::AcqRel => "AcqRel",
            Ord5::SeqCst => "SeqCst",
        }
    }

    /// The failure ordering of a compare-and-swap derived from its success ordering
    /// (Document 14 §9.4): `AcqRel` -> `Acquire`, `Release` -> `Relaxed`, the others unchanged.
    pub fn cas_failure(self) -> Ord5 {
        match self {
            Ord5::AcqRel => Ord5::Acquire,
            Ord5::Release => Ord5::Relaxed,
            o => o,
        }
    }
}

/// The ordering argument must be written as `Ordering::Name`: the generated instruction carries
/// exactly that ordering, so it has to be known at compile time.
pub fn parse_ordering(e: &Expr) -> Result<Ord5, String> {
    let mut e = e;
    while let Expr::Paren(i) = e {
        e = i;
    }
    if let Expr::Path(p) = e {
        if p.len() == 2 && p[0] == "Ordering" {
            return match p[1].as_str() {
                "Relaxed" => Ok(Ord5::Relaxed),
                "Acquire" => Ok(Ord5::Acquire),
                "Release" => Ok(Ord5::Release),
                "AcqRel" => Ok(Ord5::AcqRel),
                "SeqCst" => Ok(Ord5::SeqCst),
                other => Err(format!("`Ordering::{}` does not exist: the orderings are Relaxed, Acquire, Release, AcqRel, SeqCst (Document 14 §9.4)", other)),
            };
        }
    }
    Err("the memory ordering must be written out as `Ordering::Relaxed`, `Ordering::Acquire`, `Ordering::Release`, `Ordering::AcqRel` or `Ordering::SeqCst`: the instruction carries exactly that ordering, so it cannot be a variable (Document 14 §9.4)".into())
}

/// Element types `Atomic<T>` exists for (Document 14 §9.4): `bool`, `i8`..`i64`, `u8`..`u64`,
/// `isize`, `usize`, and `*mut T`.
pub fn elem_ok(t: &Ty) -> bool {
    matches!(t, Ty::Bool | Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 | Ty::Isize | Ty::Usize | Ty::RawPtr(true, _))
}

pub const ALLOWED: &str = "`bool`, `i8`..`i64`, `u8`..`u64`, `isize`, `usize` and `*mut T`";
