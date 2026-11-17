//! Checks for the compiler-known `Drop` trait (Document 6 §9.9, rules 1 and 5; Phase 13 M0).
//!
//! Runs on the parsed program BEFORE the prelude is added and before monomorphization, so it sees
//! the declarations exactly as the user wrote them:
//!  * a user trait named `Drop` is an error (the name is reserved);
//!  * `impl Drop for T` is allowed only for a struct or enum declared in this program (the orphan
//!    rule; `Drop` for `i32`, `String`, `Option<..>` and so on is rejected);
//!  * a generic impl must cover EVERY instantiation: `impl<T> Drop for W<T>` is fine, an impl
//!    specialised on concrete arguments (`impl Drop for W<i32>`) or carrying bounds / a `where`
//!    clause is an error;
//!  * a type that implements `Drop` cannot be `@copy`;
//!  * the method must be exactly `fn drop(borrow mut self)`.
//!
//! The remaining rules are checked where the information is: explicit `x.drop()` calls in the type
//! checker, "no field may be moved out of a `Drop` type" in code generation.

use crate::ast::*;
use std::collections::HashMap;

pub fn check(program: &Program) -> Vec<String> {
    let mut errors = Vec::new();
    let mut structs: HashMap<&str, (&StructDecl, bool)> = HashMap::new();
    let mut enums: HashMap<&str, &EnumDecl> = HashMap::new();
    collect(&program.items, &mut structs, &mut enums);
    check_items(&program.items, &structs, &enums, &mut errors);
    errors
}

fn collect<'a>(items: &'a [Item], structs: &mut HashMap<&'a str, (&'a StructDecl, bool)>, enums: &mut HashMap<&'a str, &'a EnumDecl>) {
    for it in items {
        match &it.kind {
            ItemKind::Struct(s) => {
                structs.insert(s.name.as_str(), (s, it.attrs.iter().any(|a| a.name == "copy")));
            }
            ItemKind::Enum(e) => {
                enums.insert(e.name.as_str(), e);
            }
            ItemKind::Mod(m) => collect(&m.items, structs, enums),
            ItemKind::TargetBlock(_, inner) => collect(inner, structs, enums),
            _ => {}
        }
    }
}

fn check_items(items: &[Item], structs: &HashMap<&str, (&StructDecl, bool)>, enums: &HashMap<&str, &EnumDecl>, errors: &mut Vec<String>) {
    for it in items {
        match &it.kind {
            ItemKind::Trait(t) if t.name == "Drop" => {
                errors.push(format!(
                    "error: the trait name `Drop` is reserved by the compiler; a user trait cannot be called `Drop` (Document 6 §9.9, rule 1) --> {}",
                    it.span
                ));
            }
            ItemKind::Impl(im) if im.trait_ref.as_ref().map(|t| t.name == "Drop").unwrap_or(false) => check_impl(it, im, structs, enums, errors),
            ItemKind::Mod(m) => check_items(&m.items, structs, enums, errors),
            ItemKind::TargetBlock(_, inner) => check_items(inner, structs, enums, errors),
            _ => {}
        }
    }
}

fn param_names(g: &GenericParams) -> Vec<String> {
    g.0.iter()
        .map(|p| match p {
            GenericParam::Type { name, .. } | GenericParam::Const { name, .. } => name.clone(),
        })
        .collect()
}

fn check_impl(it: &Item, im: &ImplDecl, structs: &HashMap<&str, (&StructDecl, bool)>, enums: &HashMap<&str, &EnumDecl>, errors: &mut Vec<String>) {
    let target = im.target.name.as_str();
    let at = format!("--> {}", it.span);
    let mut err = |m: String| errors.push(format!("error: {} (Document 6 §9.9) {}", m, at));

    if !im.trait_ref.as_ref().map(|t| t.args.is_empty()).unwrap_or(true) {
        err("`Drop` takes no type arguments".to_string());
    }
    let declared_generics: Option<&GenericParams> = if let Some((s, copy)) = structs.get(target) {
        if *copy {
            err(format!("`{}` is `@copy` and cannot implement `Drop`: a value that is duplicated bit for bit would be dropped twice (rule 5)", target));
        }
        Some(&s.generics)
    } else if let Some(e) = enums.get(target) {
        Some(&e.generics)
    } else {
        err(format!(
            "`impl Drop for {}` is not allowed: `Drop` can only be implemented for a struct or enum declared in the current package (rule 1)",
            target
        ));
        None
    };

    if let Some(decl_generics) = declared_generics {
        let own = param_names(&im.generics);
        let has_bounds = im.generics.0.iter().any(|p| matches!(p, GenericParam::Type { bounds, .. } if !bounds.is_empty())) || !im.where_clause.0.is_empty();
        if has_bounds {
            err(format!("`impl Drop for {}` carries extra bounds; the impl must cover every instantiation of `{}` (rule 1)", target, target));
        }
        // the impl's arguments must be exactly its own parameters, each used once
        let mut seen: Vec<&str> = Vec::new();
        let mut covers_all = im.target.args.len() == decl_generics.0.len();
        for a in &im.target.args {
            match a {
                Type::Named(n, args) if args.is_empty() && own.contains(n) && !seen.contains(&n.as_str()) => seen.push(n.as_str()),
                _ => covers_all = false,
            }
        }
        if !covers_all {
            if im.target.args.is_empty() && !decl_generics.0.is_empty() {
                err(format!("`{}` is generic: write `impl<..> Drop for {}<..>` so that the impl covers every instantiation (rule 1)", target, target));
            } else {
                err(format!(
                    "`impl Drop for {}<..>` is specialised on concrete or repeated arguments; the impl must cover every instantiation: write it as `impl<T> Drop for {}<T>` (rule 1)",
                    target, target
                ));
            }
        }
    }

    // exactly `fn drop(borrow mut self)`
    let mut found_drop = false;
    for ii in &im.items {
        let ImplItem::Fn(f) = ii else {
            err("`impl Drop` cannot declare associated types".to_string());
            continue;
        };
        if f.name != "drop" {
            err(format!("`impl Drop for {}` has no method `{}`: `Drop` has only `fn drop(borrow mut self)`", target, f.name));
            continue;
        }
        found_drop = true;
        let recv_ok = matches!(f.params.first(), Some(p) if p.name == "self" && p.ownership == OwnershipMod::BorrowMut);
        if !recv_ok || f.params.len() != 1 {
            err(format!("`drop` of `impl Drop for {}` must be written `fn drop(borrow mut self)`: it takes only the receiver, by `borrow mut` (rule 5)", target));
        }
        if f.return_type.is_some() || f.is_async || !f.generics.0.is_empty() {
            err(format!("`drop` of `impl Drop for {}` returns nothing and is neither `async` nor generic", target));
        }
    }
    if !found_drop {
        err(format!("`impl Drop for {}` is missing the required method `drop`", target));
    }
}
