//! Channels (Phase 13 M3; Document 12 §6, §11.13; Document 14 §3, §9.6): the names and shapes of the built-in
//! `Sender<T>` and `Receiver<T>`, shared by the checker and the code generator.
//!
//! A handle is ONE POINTER to the runtime's channel object (`runtime/mtn_rt.c`); a moved-out handle slot is
//! null and its drop glue (internal, like a lock guard's) does nothing. Dropping a handle counts it out of the
//! channel (the last `Sender` closes the channel for receivers, the last `Receiver` for senders).
use crate::types::{GenericArg, Ty};

pub fn is_chan_generic(n: &str) -> bool {
    matches!(n, "Sender" | "Receiver")
}

pub fn is_sender(t: &Ty) -> bool {
    matches!(t, Ty::Generic(n, _) if n == "Sender")
}

pub fn is_chan(t: &Ty) -> bool {
    matches!(t, Ty::Generic(n, _) if is_chan_generic(n))
}

/// The message type of a `Sender<T>` / `Receiver<T>`.
pub fn elem_of(t: &Ty) -> Option<Ty> {
    match t {
        Ty::Generic(n, a) if is_chan_generic(n) => match a.first() {
            Some(GenericArg::Type(x)) => Some(x.clone()),
            _ => None,
        },
        _ => None,
    }
}

pub fn sender_of(elem: &Ty) -> Ty {
    Ty::Generic("Sender".into(), vec![GenericArg::Type(elem.clone())])
}

pub fn receiver_of(elem: &Ty) -> Ty {
    Ty::Generic("Receiver".into(), vec![GenericArg::Type(elem.clone())])
}
