//! Monomorphization (Phase 11b-2; Document 17 §5, Document 8 §1).
//!
//! The type checker records, for every call of a generic function whose type
//! arguments were fully inferred (or given by turbofish), the callee
//! expression's identity and the concrete type arguments. One round of this
//! pass then (a) rewrites each such call to a mangled, non-generic name and
//! (b) adds one specialized copy of the generic declaration per DISTINCT
//! instantiation (de-duplicated by mangled name across the whole program).
//! The driver re-checks the new program and repeats until a round adds
//! nothing (a generic that calls another generic with concrete types is only
//! discovered after substitution). A depth limit turns infinite
//! instantiation into a compile error. Code generation never sees generics.

use crate::ast::*;
use crate::desugar::{expr_children_mut, ty_to_type, ChildMut};
use crate::types::{Ty, TypeChecker};
use std::collections::{HashMap, HashSet};

/// Maximum number of instantiation rounds (nesting depth of
/// generic-calls-generic chains) before reporting infinite instantiation.
pub const MAX_DEPTH: usize = 32;

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

pub fn mangle(name: &str, args: &[Ty]) -> String {
    let parts: Vec<String> = args.iter().map(|a| sanitize(&a.to_string())).collect();
    format!("{}${}", name, parts.join("_"))
}

fn subst_type(t: &mut Type, m: &HashMap<String, Type>) {
    match t {
        Type::Named(n, args) => {
            if args.is_empty() {
                if let Some(r) = m.get(n.as_str()) {
                    *t = r.clone();
                    return;
                }
            }
            for a in args.iter_mut() {
                subst_type(a, m);
            }
        }
        Type::Array(i, _) => subst_type(i, m),
        Type::Tuple(v) => v.iter_mut().for_each(|x| subst_type(x, m)),
        Type::Ref { inner, .. } => subst_type(inner, m),
        Type::Dyn(_, v) => v.iter_mut().for_each(|x| subst_type(x, m)),
        Type::Fn(ps, r) => {
            ps.iter_mut().for_each(|x| subst_type(x, m));
            subst_type(r, m);
        }
        Type::Option(i) => subst_type(i, m),
        Type::Result(a, b) => {
            subst_type(a, m);
            subst_type(b, m);
        }
        _ => {}
    }
}

fn subst_block(b: &mut Block, m: &HashMap<String, Type>) {
    for s in b.stmts.iter_mut() {
        match s {
            Stmt::Let { ty, value, .. } => {
                if let Some(t) = ty {
                    subst_type(t, m);
                }
                if let Some(v) = value {
                    subst_expr(v, m);
                }
            }
            Stmt::Expr(e) | Stmt::Yield(e) => subst_expr(e, m),
            Stmt::Return(Some(e)) => subst_expr(e, m),
            Stmt::Break { value: Some(e), .. } => subst_expr(e, m),
            Stmt::TargetBlock(_, blk) => subst_block(blk, m),
            _ => {}
        }
    }
    if let Some(t) = &mut b.tail {
        subst_expr(t, m);
    }
}

fn subst_expr(e: &mut Expr, m: &HashMap<String, Type>) {
    match e {
        Expr::Cast { ty, .. } => subst_type(ty, m),
        Expr::Closure(c) => {
            for (_, t) in c.params.iter_mut() {
                if let Some(t) = t {
                    subst_type(t, m);
                }
            }
            if let Some(t) = &mut c.return_type {
                subst_type(t, m);
            }
        }
        Expr::TurbofishPath { gens, .. } => {
            for (_, args) in gens.iter_mut() {
                args.iter_mut().for_each(|t| subst_type(t, m));
            }
        }
        _ => {}
    }
    for c in expr_children_mut(e) {
        match c {
            ChildMut::E(x) => subst_expr(x, m),
            ChildMut::B(b) => subst_block(b, m),
        }
    }
}

fn rewrite_block(b: &mut Block, calls: &HashMap<usize, String>) {
    for s in b.stmts.iter_mut() {
        match s {
            Stmt::Let { value: Some(v), .. } => rewrite_expr(v, calls),
            Stmt::Expr(e) | Stmt::Yield(e) => rewrite_expr(e, calls),
            Stmt::Return(Some(e)) => rewrite_expr(e, calls),
            Stmt::Break { value: Some(e), .. } => rewrite_expr(e, calls),
            Stmt::TargetBlock(_, blk) => rewrite_block(blk, calls),
            _ => {}
        }
    }
    if let Some(t) = &mut b.tail {
        rewrite_expr(t, calls);
    }
}

fn rewrite_expr(e: &mut Expr, calls: &HashMap<usize, String>) {
    if let Expr::Call { callee, .. } = e {
        let key = callee.as_ref() as *const Expr as usize;
        if let Some(n) = calls.get(&key) {
            **callee = Expr::Ident(n.clone());
        }
    }
    for c in expr_children_mut(e) {
        match c {
            ChildMut::E(x) => rewrite_expr(x, calls),
            ChildMut::B(b) => rewrite_block(b, calls),
        }
    }
}

fn rewrite_items(items: &mut [Item], calls: &HashMap<usize, String>) {
    for it in items.iter_mut() {
        match &mut it.kind {
            ItemKind::Fn(f) => {
                if let Some(b) = &mut f.body {
                    rewrite_block(b, calls);
                }
            }
            ItemKind::Impl(i) => {
                for ii in i.items.iter_mut() {
                    if let ImplItem::Fn(f) = ii {
                        if let Some(b) = &mut f.body {
                            rewrite_block(b, calls);
                        }
                    }
                }
            }
            ItemKind::Mod(m) => rewrite_items(&mut m.items, calls),
            _ => {}
        }
    }
}

/// One monomorphization round. Returns `Ok(true)` if the program changed
/// (re-check and call again), `Ok(false)` at the fixpoint.
pub fn round(program: &mut Program, tc: &TypeChecker, made: &mut HashSet<String>) -> Result<bool, Vec<String>> {
    if tc.instances.is_empty() {
        return Ok(false);
    }
    let mut generic_fns: HashMap<String, (FnDecl, Item)> = HashMap::new();
    for it in &program.items {
        if let ItemKind::Fn(f) = &it.kind {
            if !f.generics.0.is_empty() {
                generic_fns.insert(f.name.clone(), (f.clone(), it.clone()));
            }
        }
    }
    let mut calls: HashMap<usize, String> = HashMap::new();
    let mut new_items: Vec<Item> = Vec::new();
    for (addr, (fname, targs)) in &tc.instances {
        let Some((decl, orig)) = generic_fns.get(fname) else { continue };
        let mangled = mangle(fname, targs);
        calls.insert(*addr, mangled.clone());
        if made.contains(&mangled) {
            continue;
        }
        if made.len() >= 4096 {
            return Err(vec![format!("error: too many generic instantiations while instantiating `{}` -- is a generic function instantiating itself with ever-larger types? (Document 8)", fname)]);
        }
        made.insert(mangled.clone());
        let type_params: Vec<&String> = decl
            .generics
            .0
            .iter()
            .filter_map(|g| if let GenericParam::Type { name, .. } = g { Some(name) } else { None })
            .collect();
        let mut m: HashMap<String, Type> = HashMap::new();
        for (p, t) in type_params.iter().zip(targs.iter()) {
            match ty_to_type(t) {
                Some(ty) => {
                    m.insert((*p).clone(), ty);
                }
                None => return Err(vec![format!("error: cannot instantiate `{}` with type `{}` (not yet supported by codegen -- Phase 11)", fname, t)]),
            }
        }
        let mut inst = decl.clone();
        inst.name = mangled;
        inst.generics = GenericParams(Vec::new());
        inst.where_clause = WhereClause::default();
        for p in inst.params.iter_mut() {
            subst_type(&mut p.ty, &m);
        }
        if let Some(r) = &mut inst.return_type {
            subst_type(r, &m);
        }
        if let Some(b) = &mut inst.body {
            subst_block(b, &m);
        }
        new_items.push(Item { kind: ItemKind::Fn(inst), ..orig.clone() });
    }
    rewrite_items(&mut program.items, &calls);
    let changed = !new_items.is_empty() || !calls.is_empty();
    program.items.extend(new_items);
    Ok(changed)
}

/// Drop the generic declarations once the fixpoint is reached.
pub fn strip_generics(program: &mut Program) {
    program.items.retain(|it| !matches!(&it.kind, ItemKind::Fn(f) if !f.generics.0.is_empty()));
}
