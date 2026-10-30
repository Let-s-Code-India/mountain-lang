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
use crate::types::{eval_const_int, garg_ty, resolve_type, GenericArg, Ty, TypeChecker};
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
        Type::Array(i, size) => {
            subst_type(i, m);
            if let Some(s) = size {
                subst_expr(s, m);
            }
        }
        Type::Assoc(b, _) => subst_type(b, m),
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
        // A const generic parameter used as a value (`N`, `[0; N]`) becomes its literal.
        Expr::Ident(n) => {
            if let Some(Type::ConstArg(c)) = m.get(n.as_str()) {
                *e = (**c).clone();
            }
            return;
        }
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

/// Everything the rewrite walk needs (all keyed by expression identity).
#[derive(Default)]
struct Rw {
    calls: HashMap<usize, String>,
    type_calls: HashMap<usize, String>,
    struct_lits: HashMap<usize, String>,
    /// match scrutinee identity -> (generic enum base, mangled instance)
    scrut: HashMap<usize, (String, String)>,
}

fn rewrite_block(b: &mut Block, calls: &Rw) {
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

fn rename_pattern(p: &mut Pattern, base: &str, mg: &str) {
    let fix = |n: &mut String| {
        if let Some(rest) = n.strip_prefix(&format!("{}::", base)) {
            *n = format!("{}::{}", mg, rest);
        }
    };
    match p {
        Pattern::TupleStruct(n, subs) => {
            fix(n);
            subs.iter_mut().for_each(|s| rename_pattern(s, base, mg));
        }
        Pattern::Ident(n) => fix(n),
        Pattern::Tuple(subs) | Pattern::Or(subs) => subs.iter_mut().for_each(|s| rename_pattern(s, base, mg)),
        _ => {}
    }
}

fn rewrite_expr(e: &mut Expr, calls: &Rw) {
    let self_key = e as *const Expr as usize;
    if let Expr::Path(p) = e {
        if p.len() == 2 {
            if let Some(mg) = calls.type_calls.get(&self_key) {
                p[0] = mg.clone();
            }
        }
    }
    if let Expr::Match(me) = e {
        let skey = &me.scrutinee as *const Expr as usize;
        if let Some((base, mg)) = calls.scrut.get(&skey) {
            for arm in me.arms.iter_mut() {
                arm.patterns.iter_mut().for_each(|p| rename_pattern(p, base, mg));
            }
        }
    }
    if let Expr::StructLit { name, .. } = e {
        if let Some(n) = calls.struct_lits.get(&self_key) {
            *name = n.clone();
        }
    }
    if let Expr::Call { callee, .. } = e {
        let key = callee.as_ref() as *const Expr as usize;
        if let Some(n) = calls.calls.get(&key) {
            **callee = Expr::Ident(n.clone());
        } else if let Some(tn) = calls.type_calls.get(&key) {
            let fname = match callee.as_ref() {
                Expr::Path(p) => p.last().cloned(),
                Expr::TurbofishPath { path, .. } => path.last().cloned(),
                _ => None,
            };
            if let Some(f) = fname {
                **callee = Expr::Path(vec![tn.clone(), f]);
            }
        }
    }
    for c in expr_children_mut(e) {
        match c {
            ChildMut::E(x) => rewrite_expr(x, calls),
            ChildMut::B(b) => rewrite_block(b, calls),
        }
    }
}

fn rewrite_items(items: &mut [Item], calls: &Rw) {
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


// ------------------------------------------------------------ type visiting

type TyFn<'a> = &'a mut dyn FnMut(&mut Type) -> bool;

/// Calls `f` on `t`; if it returns true, recurses into the children.
fn walk_type(t: &mut Type, f: TyFn) {
    if !f(t) {
        return;
    }
    match t {
        Type::Named(_, a) | Type::Tuple(a) | Type::Dyn(_, a) => a.iter_mut().for_each(|x| walk_type(x, f)),
        Type::Array(i, _) | Type::Option(i) => walk_type(i, f),
        Type::Assoc(b, _) => walk_type(b, f),
        Type::Ref { inner, .. } => walk_type(inner, f),
        Type::Fn(ps, r) => {
            ps.iter_mut().for_each(|x| walk_type(x, f));
            walk_type(r, f);
        }
        Type::Result(a, b) => {
            walk_type(a, f);
            walk_type(b, f);
        }
        _ => {}
    }
}

fn walk_block_types(b: &mut Block, f: TyFn) {
    for s in b.stmts.iter_mut() {
        match s {
            Stmt::Let { ty, value, .. } => {
                if let Some(t) = ty {
                    walk_type(t, f);
                }
                if let Some(v) = value {
                    walk_expr_types(v, f);
                }
            }
            Stmt::Expr(e) | Stmt::Yield(e) => walk_expr_types(e, f),
            Stmt::Return(Some(e)) => walk_expr_types(e, f),
            Stmt::Break { value: Some(e), .. } => walk_expr_types(e, f),
            Stmt::TargetBlock(_, blk) => walk_block_types(blk, f),
            _ => {}
        }
    }
    if let Some(t) = &mut b.tail {
        walk_expr_types(t, f);
    }
}

fn walk_expr_types(e: &mut Expr, f: TyFn) {
    match e {
        Expr::Cast { ty, .. } => walk_type(ty, f),
        Expr::Closure(c) => {
            for (_, t) in c.params.iter_mut() {
                if let Some(t) = t {
                    walk_type(t, f);
                }
            }
            if let Some(t) = &mut c.return_type {
                walk_type(t, f);
            }
        }
        Expr::TurbofishPath { gens, .. } => {
            for (_, args) in gens.iter_mut() {
                args.iter_mut().for_each(|t| walk_type(t, f));
            }
        }
        _ => {}
    }
    for c in expr_children_mut(e) {
        match c {
            ChildMut::E(x) => walk_expr_types(x, f),
            ChildMut::B(b) => walk_block_types(b, f),
        }
    }
}

fn walk_fn_types(fd: &mut FnDecl, f: TyFn) {
    if !fd.generics.0.is_empty() {
        return;
    }
    for p in fd.params.iter_mut() {
        walk_type(&mut p.ty, f);
    }
    if let Some(r) = &mut fd.return_type {
        walk_type(r, f);
    }
    if let Some(b) = &mut fd.body {
        walk_block_types(b, f);
    }
}

/// Visits every type written in the NON-generic items (generic declarations
/// are templates, not code).
fn walk_items_types(items: &mut [Item], f: TyFn) {
    for it in items.iter_mut() {
        match &mut it.kind {
            ItemKind::Fn(fd) => walk_fn_types(fd, f),
            ItemKind::Struct(s) if s.generics.0.is_empty() => match &mut s.body {
                StructBody::Named(fs) => fs.iter_mut().for_each(|x| walk_type(&mut x.ty, f)),
                StructBody::Tuple(ts) => ts.iter_mut().for_each(|x| walk_type(x, f)),
                StructBody::Unit => {}
            },
            ItemKind::Enum(en) if en.generics.0.is_empty() => {
                for v in en.variants.iter_mut() {
                    v.data.iter_mut().for_each(|x| walk_type(x, f));
                }
            }
            ItemKind::Impl(i) if i.generics.0.is_empty() => {
                i.target.args.iter_mut().for_each(|x| walk_type(x, f));
                if let Some(tr) = &mut i.trait_ref {
                    tr.args.iter_mut().for_each(|x| walk_type(x, f));
                }
                for ii in i.items.iter_mut() {
                    match ii {
                        ImplItem::Fn(fd) => walk_fn_types(fd, f),
                        ImplItem::AssocType(_, t) => walk_type(t, f),
                    }
                }
            }
            ItemKind::Const(c) => walk_type(&mut c.ty, f),
            ItemKind::Static(c) => walk_type(&mut c.ty, f),
            ItemKind::Mod(m) => walk_items_types(&mut m.items, f),
            _ => {}
        }
    }
}

// ------------------------------------------------------------ type discovery

const PRIMS: [&str; 22] = ["i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32", "f64", "bool", "char", "String", "str", "mut", "fn", "Option", "Result"];

fn ty_concrete(t: &Ty, known: &HashSet<String>) -> bool {
    if t.to_string().contains('?') {
        return false; // an unresolved inference variable: the final check reports it
    }
    t.to_string()
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))
        .filter(|tok| !tok.is_empty() && !tok.chars().next().unwrap().is_ascii_digit())
        .all(|tok| known.contains(tok) || PRIMS.contains(&tok))
}

fn collect_generic_tys(t: &Ty, bases: &HashSet<String>, out: &mut Vec<(String, Vec<Ty>)>) {
    match t {
        Ty::Generic(n, args) => {
            let mut tys = Vec::new();
            for a in args {
                match a {
                    GenericArg::Type(x) => {
                        collect_generic_tys(x, bases, out);
                        tys.push(x.clone());
                    }
                    GenericArg::Const(v) => tys.push(Ty::ConstVal(*v)),
                }
            }
            if bases.contains(n) {
                out.push((n.clone(), tys));
            }
        }
        Ty::Array(i) | Ty::Fixed(i, _) | Ty::Ref(_, i) | Ty::OptionTy(i) => collect_generic_tys(i, bases, out),
        Ty::Tuple(v) => v.iter().for_each(|x| collect_generic_tys(x, bases, out)),
        Ty::ResultTy(a, b) => {
            collect_generic_tys(a, bases, out);
            collect_generic_tys(b, bases, out);
        }
        _ => {}
    }
}

/// Names of ALL generic parameters (type and const), in declaration order.
fn generic_param_names(g: &GenericParams) -> Vec<String> {
    g.0.iter().map(|p| match p { GenericParam::Type { name, .. } | GenericParam::Const { name, .. } => name.clone() }).collect()
}

/// One monomorphization round. Returns `Ok(true)` if the program changed
/// (re-check and call again), `Ok(false)` at the fixpoint.
pub fn round(program: &mut Program, tc: &TypeChecker, made: &mut HashSet<String>) -> Result<bool, Vec<String>> {
    let mut changed = false;
    let mut rw = Rw::default();
    let mut new_items: Vec<Item> = Vec::new();

    // ---- generic declarations (templates)
    let mut generic_fns: HashMap<String, (FnDecl, Item)> = HashMap::new();
    let mut gstructs: HashMap<String, (StructDecl, Item)> = HashMap::new();
    let mut genums: HashMap<String, (EnumDecl, Item)> = HashMap::new();
    let mut gimpls: Vec<(ImplDecl, Item)> = Vec::new();
    let mut known: HashSet<String> = HashSet::new();
    for it in &program.items {
        match &it.kind {
            ItemKind::Fn(f) if !f.generics.0.is_empty() => {
                generic_fns.insert(f.name.clone(), (f.clone(), it.clone()));
            }
            ItemKind::Struct(s) => {
                known.insert(s.name.clone());
                if !s.generics.0.is_empty() {
                    gstructs.insert(s.name.clone(), (s.clone(), it.clone()));
                }
            }
            ItemKind::Enum(e) => {
                known.insert(e.name.clone());
                if !e.generics.0.is_empty() {
                    genums.insert(e.name.clone(), (e.clone(), it.clone()));
                }
            }
            ItemKind::Impl(i) => gimpls.push((i.clone(), it.clone())),
            _ => {}
        }
    }
    known.extend(made.iter().cloned());
    let bases: HashSet<String> = gstructs.keys().chain(genums.keys()).cloned().collect();
    // Template impls: own type parameters, or a specialized impl on a generic base (`impl C for Stack<i32>`).
    gimpls.retain(|(i, _)| !i.generics.0.is_empty() || (bases.contains(&i.target.name) && !i.target.args.is_empty()));

    // ---- generic function instances
    for (addr, (fname, targs)) in &tc.instances {
        let Some((decl, orig)) = generic_fns.get(fname) else { continue };
        let mangled = mangle(fname, targs);
        rw.calls.insert(*addr, mangled.clone());
        changed = true;
        if made.contains(&mangled) {
            continue;
        }
        if made.len() >= 4096 {
            return Err(vec![format!("error: too many generic instantiations while instantiating `{}` -- is a generic instantiating itself with ever-larger types? (Document 8)", fname)]);
        }
        made.insert(mangled.clone());
        let mut m: HashMap<String, Type> = HashMap::new();
        for (p, t) in generic_param_names(&decl.generics).iter().zip(targs.iter()) {
            match ty_to_type(t) {
                Some(ty) => {
                    m.insert(p.clone(), ty);
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

    // ---- generic type instances (struct / enum + every matching generic impl)
    if !bases.is_empty() {
        let mut wanted: Vec<(String, Vec<Ty>)> = Vec::new();
        for t in tc.expr_types.values().chain(tc.type_paths.values()) {
            collect_generic_tys(t, &bases, &mut wanted);
        }
        let mut ast_tys: Vec<Ty> = Vec::new();
        walk_items_types(&mut program.items, &mut |t: &mut Type| {
            if let Type::Named(n, args) = t {
                if !args.is_empty() && bases.contains(n) {
                    ast_tys.push(resolve_type(t));
                    return false;
                }
            }
            true
        });
        for t in &ast_tys {
            collect_generic_tys(t, &bases, &mut wanted);
        }
        for (base, tys) in wanted {
            if !tys.iter().all(|t| ty_concrete(t, &known)) {
                continue;
            }
            let mangled = mangle(&base, &tys);
            if made.contains(&mangled) {
                continue;
            }
            if made.len() >= 4096 {
                return Err(vec![format!("error: too many generic instantiations while instantiating `{}` (Document 8)", base)]);
            }
            let arg_types: Option<Vec<Type>> = tys.iter().map(ty_to_type).collect();
            let Some(arg_types) = arg_types else {
                return Err(vec![format!("error: cannot instantiate `{}` with type arguments ({}) (not yet supported by codegen -- Phase 11)", base, tys.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "))]);
            };
            made.insert(mangled.clone());
            changed = true;
            if let Some((sd, orig)) = gstructs.get(&base) {
                let m: HashMap<String, Type> = generic_param_names(&sd.generics).into_iter().zip(arg_types.iter().cloned()).collect();
                let mut inst = sd.clone();
                inst.name = mangled.clone();
                inst.generics = GenericParams(Vec::new());
                match &mut inst.body {
                    StructBody::Named(fs) => fs.iter_mut().for_each(|x| subst_type(&mut x.ty, &m)),
                    StructBody::Tuple(ts) => ts.iter_mut().for_each(|x| subst_type(x, &m)),
                    StructBody::Unit => {}
                }
                new_items.push(Item { kind: ItemKind::Struct(inst), ..orig.clone() });
            }
            if let Some((ed, orig)) = genums.get(&base) {
                let m: HashMap<String, Type> = generic_param_names(&ed.generics).into_iter().zip(arg_types.iter().cloned()).collect();
                let mut inst = ed.clone();
                inst.name = mangled.clone();
                inst.generics = GenericParams(Vec::new());
                for v in inst.variants.iter_mut() {
                    v.data.iter_mut().for_each(|x| subst_type(x, &m));
                }
                new_items.push(Item { kind: ItemKind::Enum(inst), ..orig.clone() });
            }
            for (imp, orig) in gimpls.iter().filter(|(i, _)| i.target.name == base) {
                let gnames = generic_param_names(&imp.generics);
                let mut m: HashMap<String, Type> = HashMap::new();
                let mut matches = true;
                for ((ta, at), ty) in imp.target.args.iter().zip(arg_types.iter()).zip(tys.iter()) {
                    match ta {
                        Type::Named(g, a) if a.is_empty() && gnames.contains(g) => {
                            m.insert(g.clone(), at.clone());
                        }
                        Type::ConstArg(ce) => {
                            // `impl C for Buf<i32, 4>`: applies only to the instance with that length.
                            let v = eval_const_int(ce, &HashMap::new());
                            if v.map(|v| Ty::ConstVal(v as i64)) != Some(ty.clone()) {
                                matches = false;
                            }
                        }
                        _ => {
                            // A concrete argument (`impl C for Stack<i32>`): applies only to that instance.
                            let rt = resolve_type(ta);
                            if gnames.iter().any(|g| rt.to_string().split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|tok| tok == g)) {
                                return Err(vec![format!("error: `impl` with a type parameter nested inside an argument of `{}<..>` is not yet supported by codegen -- Phase 11", base)]);
                            }
                            if rt != *ty {
                                matches = false;
                            }
                        }
                    }
                }
                if !matches {
                    continue;
                }
                // Doc 8 §4: a bounded `impl` only provides its methods when the
                // concrete arguments satisfy every bound; otherwise skip this copy.
                let mut bound_pairs: Vec<(String, String)> = Vec::new();
                for g in &imp.generics.0 {
                    if let GenericParam::Type { name, bounds } = g {
                        bounds.iter().for_each(|b| bound_pairs.push((name.clone(), b.name.clone())));
                    }
                }
                for wb in &imp.where_clause.0 {
                    wb.bounds.iter().for_each(|b| bound_pairs.push((wb.name.clone(), b.name.clone())));
                }
                let mut unsatisfied = false;
                for (p, tr) in &bound_pairs {
                    if let Some(pos) = imp.target.args.iter().position(|ta| matches!(ta, Type::Named(g, a) if a.is_empty() && g == p)) {
                        if !tc.satisfies_bound(&tys[pos], tr) {
                            unsatisfied = true;
                        }
                    }
                }
                if unsatisfied {
                    continue;
                }
                let mut inst = imp.clone();
                inst.generics = GenericParams(Vec::new());
                inst.where_clause = WhereClause::default();
                inst.target = TypeRef { name: mangled.clone(), args: Vec::new() };
                if let Some(tr) = &mut inst.trait_ref {
                    tr.args.iter_mut().for_each(|x| subst_type(x, &m));
                }
                for ii in inst.items.iter_mut() {
                    match ii {
                        ImplItem::Fn(f) => {
                            for p in f.params.iter_mut() {
                                subst_type(&mut p.ty, &m);
                            }
                            if let Some(r) = &mut f.return_type {
                                subst_type(r, &m);
                            }
                            if let Some(b) = &mut f.body {
                                subst_block(b, &m);
                            }
                        }
                        ImplItem::AssocType(_, t) => subst_type(t, &m),
                    }
                }
                new_items.push(Item { kind: ItemKind::Impl(inst), ..orig.clone() });
            }
        }
        // ---- expression-level renames (struct literals, `Type::f(..)` calls)
        for (addr, t) in tc.expr_types.iter() {
            if let Ty::Generic(n, args) = t {
                let tys: Vec<Ty> = args.iter().map(garg_ty).collect();
                if tys.len() == args.len() && bases.contains(n) {
                    let mg = mangle(n, &tys);
                    if made.contains(&mg) {
                        rw.scrut.insert(*addr, (n.clone(), mg.clone()));
                        rw.struct_lits.insert(*addr, mg);
                    }
                }
            }
        }
        for (addr, t) in tc.type_paths.iter() {
            if let Ty::Generic(n, args) = t {
                let tys: Vec<Ty> = args.iter().map(garg_ty).collect();
                let mg = mangle(n, &tys);
                if made.contains(&mg) {
                    rw.type_calls.insert(*addr, mg);
                    changed = true;
                }
            }
        }
    }

    for (addr, inst) in tc.inst_names.iter() {
        rw.struct_lits.insert(*addr, inst.clone());
        rw.type_calls.insert(*addr, inst.clone());
        changed = true;
    }
    rewrite_items(&mut program.items, &rw);
    program.items.extend(new_items);

    // ---- rewrite written types `Pair<i32, f64>` -> `Pair$i32_f64` (top-down; a
    // node whose instance does not exist yet is left for the next round)
    if !bases.is_empty() {
        let mut renamed = false;
        walk_items_types(&mut program.items, &mut |t: &mut Type| {
            if let Type::Named(n, args) = t {
                if !args.is_empty() && bases.contains(n) {
                    if let Ty::Generic(gn, gargs) = resolve_type(t) {
                        let tys: Vec<Ty> = gargs.iter().map(garg_ty).collect();
                        if tys.len() == gargs.len() {
                            let mg = mangle(&gn, &tys);
                            if made.contains(&mg) {
                                *t = Type::Named(mg, Vec::new());
                                renamed = true;
                            }
                        }
                    }
                    return false;
                }
            }
            true
        });
        if renamed {
            changed = true;
        }
    }
    Ok(changed)
}

/// Drop the generic declarations once the fixpoint is reached.
pub fn strip_generics(program: &mut Program) {
    let bases: HashSet<String> = program.items.iter().filter_map(|it| match &it.kind {
        ItemKind::Struct(s) if !s.generics.0.is_empty() => Some(s.name.clone()),
        ItemKind::Enum(e) if !e.generics.0.is_empty() => Some(e.name.clone()),
        _ => None,
    }).collect();
    program.items.retain(|it| match &it.kind {
        ItemKind::Fn(f) => f.generics.0.is_empty(),
        ItemKind::Struct(s) => s.generics.0.is_empty(),
        ItemKind::Enum(e) => e.generics.0.is_empty(),
        ItemKind::Impl(i) => i.generics.0.is_empty() && !(bases.contains(&i.target.name) && !i.target.args.is_empty()),
        _ => true,
    });
}
