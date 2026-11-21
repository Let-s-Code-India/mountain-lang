//! Locks (Phase 13 M2; Document 14 §6, §7, §9.1, §9.5; Document 12 §7): the names and shapes of the
//! built-in `Spinlock`, `Mutex<T>`, `RwLock<T>` and their guards, shared by the checker and the code generator.
//!
//! Guards are compiler-known types with an INTERNAL drop glue (the glue calls the runtime's unlock); they
//! are not user `Drop` impls, because they carry no hidden "live" byte: a guard is one pointer to its
//! lock, and a moved-out guard slot is null, which the glue skips.
use crate::types::{GenericArg, Ty};

/// The three guard types that give access to a protected value (`Spinlock` protects nothing).
pub const VALUE_GUARDS: [&str; 3] = ["MutexGuard", "RwLockReadGuard", "RwLockWriteGuard"];

pub fn is_lock_generic(n: &str) -> bool {
    matches!(n, "Mutex" | "RwLock")
}

pub fn is_guard_generic(n: &str) -> bool {
    VALUE_GUARDS.contains(&n)
}

/// `true` for any guard type (`SpinlockGuard` included).
pub fn is_guard(t: &Ty) -> bool {
    match t {
        Ty::Generic(n, _) => is_guard_generic(n),
        Ty::Named(n) => n == "SpinlockGuard",
        _ => false,
    }
}

/// A guard that only gives shared access (`RwLock::read`).
pub fn is_read_guard(t: &Ty) -> bool {
    matches!(t, Ty::Generic(n, _) if n == "RwLockReadGuard")
}

/// `true` when `t` is (or contains, through `Option`/`Result`/tuples/arrays/references/generics) a guard.
pub fn mentions_guard(t: &Ty) -> bool {
    match t {
        _ if is_guard(t) => true,
        Ty::Generic(_, args) => args.iter().any(|a| matches!(a, GenericArg::Type(x) if mentions_guard(x))),
        Ty::Ref(_, i) | Ty::RawPtr(_, i) | Ty::Array(i) | Ty::Fixed(i, _) | Ty::OptionTy(i) => mentions_guard(i),
        Ty::Tuple(v) => v.iter().any(mentions_guard),
        Ty::ResultTy(a, b) => mentions_guard(a) || mentions_guard(b),
        _ => false,
    }
}

/// The guard type `lock`/`read`/`write` returns for a lock of the given kind and element.
pub fn guard_of(lock: &str, elem: &Ty) -> Ty {
    let g = match lock {
        "Mutex" => "MutexGuard",
        _ => "RwLockReadGuard",
    };
    Ty::Generic(g.into(), vec![GenericArg::Type(elem.clone())])
}

pub fn write_guard_of(elem: &Ty) -> Ty {
    Ty::Generic("RwLockWriteGuard".into(), vec![GenericArg::Type(elem.clone())])
}

pub fn spin_guard() -> Ty {
    Ty::Named("SpinlockGuard".into())
}

/// Element type of a guard or a lock.
pub fn elem_of(t: &Ty) -> Option<Ty> {
    match t {
        Ty::Generic(n, a) if is_guard_generic(n) || is_lock_generic(n) => match a.first() {
            Some(GenericArg::Type(x)) => Some(x.clone()),
            _ => None,
        },
        _ => None,
    }
}
