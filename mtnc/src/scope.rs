//! Scoping of items declared inside function bodies (Phase 11b-1; Doc 25 §2.5
//! "nested items inside functions", follow-up to Phase 11a).
//!
//! Nested items are hoisted into one global namespace by the checker and by
//! codegen (`desugar::all_items`). Two functions that each declare a nested
//! `helper` would therefore collide. This pass renames a nested item **only if
//! its name would collide** (it is declared more than once in the program,
//! counting top-level items) to `<enclosing function path>$<name>` and rewrites
//! every reference to it inside the enclosing function (including inside other
//! nested items). Names that do not collide are left untouched, so ordinary
//! programs are not rewritten at all.
//!
//! A nested item is treated as visible in the whole body of its enclosing
//! function (not just the block it is written in); a local variable of the
//! same name shadows it. Two same-named items in ONE function body remain a
//! "defined more than once" error.

use crate::ast::*;
use crate::desugar::{expr_children_mut, ChildMut};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Mutable traversal helpers
// ---------------------------------------------------------------------------

fn expr_items<'a>(e: &'a mut Expr, out: &mut Vec<&'a mut Item>) {
    for c in expr_children_mut(e) {
        match c {
            ChildMut::E(x) => expr_items(x, out),
            ChildMut::B(b) => block_items(b, out),
        }
    }
}

/// The items declared directly in `b` (at any block depth), not those declared
/// inside other items' bodies.
fn block_items<'a>(b: &'a mut Block, out: &mut Vec<&'a mut Item>) {
    for s in b.stmts.iter_mut() {
        match s {
            Stmt::Item(it) => out.push(&mut **it),
            Stmt::Let { value, .. } => {
                if let Some(v) = value {
                    expr_items(v, out);
                }
            }
            Stmt::Expr(e) | Stmt::Yield(e) => expr_items(e, out),
            Stmt::Return(v) => {
                if let Some(v) = v {
                    expr_items(v, out);
                }
            }
            Stmt::Break { value, .. } => {
                if let Some(v) = value {
                    expr_items(v, out);
                }
            }
            Stmt::Continue { .. } => {}
            Stmt::TargetBlock(_, blk) => block_items(blk, out),
        }
    }
    if let Some(t) = &mut b.tail {
        expr_items(t, out);
    }
}

/// Calls `cb` for every item of the program: top-level and nested, recursively
/// (an item is visited before the items nested in it).
pub fn for_each_item_mut(items: &mut [Item], cb: &mut dyn FnMut(&mut Item)) {
    for it in items.iter_mut() {
        cb(it);
        let mut nested: Vec<&mut Item> = Vec::new();
        match &mut it.kind {
            ItemKind::Fn(f) => {
                if let Some(b) = &mut f.body {
                    block_items(b, &mut nested);
                }
            }
            ItemKind::Impl(im) => {
                for ii in im.items.iter_mut() {
                    if let ImplItem::Fn(f) = ii {
                        if let Some(b) = &mut f.body {
                            block_items(b, &mut nested);
                        }
                    }
                }
            }
            _ => {}
        }
        for n in nested {
            for_each_item_mut(std::slice::from_mut(n), cb);
        }
    }
}

fn item_name(it: &Item) -> Option<&str> {
    match &it.kind {
        ItemKind::Fn(f) => Some(&f.name),
        ItemKind::Struct(s) => Some(&s.name),
        ItemKind::Enum(e) => Some(&e.name),
        ItemKind::Const(c) => Some(&c.name),
        ItemKind::Static(c) => Some(&c.name),
        ItemKind::TypeAlias(a) => Some(&a.name),
        ItemKind::Trait(t) => Some(&t.name),
        _ => None,
    }
}

fn set_item_name(it: &mut Item, new: String) {
    match &mut it.kind {
        ItemKind::Fn(f) => f.name = new,
        ItemKind::Struct(s) => s.name = new,
        ItemKind::Enum(e) => e.name = new,
        ItemKind::Const(c) => c.name = new,
        ItemKind::Static(c) => c.name = new,
        ItemKind::TypeAlias(a) => a.name = new,
        ItemKind::Trait(t) => t.name = new,
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Reference renaming
// ---------------------------------------------------------------------------

struct Renamer<'m> {
    map: &'m HashMap<String, String>,
    scopes: Vec<HashSet<String>>,
}

fn pattern_names(p: &Pattern, out: &mut Vec<String>) {
    match p {
        Pattern::Ident(n) if !n.contains("::") => out.push(n.clone()),
        Pattern::Mut(n) => out.push(n.clone()),
        Pattern::TupleStruct(_, subs) | Pattern::Tuple(subs) | Pattern::Or(subs) => {
            for s in subs {
                pattern_names(s, out);
            }
        }
        Pattern::Array(subs, rest) => {
            for s in subs {
                pattern_names(s, out);
            }
            if let Some(Some(n)) = rest {
                out.push(n.clone());
            }
        }
        _ => {}
    }
}

impl Renamer<'_> {
    /// `a::b` -> `mapped(a)::b` when the first segment is renamed.
    fn rn(&self, n: &str) -> Option<String> {
        let (first, rest) = match n.split_once("::") {
            Some((f, r)) => (f, Some(r)),
            None => (n, None),
        };
        let new = self.map.get(first)?;
        Some(match rest {
            Some(r) => format!("{}::{}", new, r),
            None => new.clone(),
        })
    }

    fn shadowed(&self, n: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(n))
    }

    fn declare(&mut self, p: &Pattern) {
        let mut names = Vec::new();
        pattern_names(p, &mut names);
        let top = self.scopes.last_mut().unwrap();
        for n in names {
            top.insert(n);
        }
    }

    fn ty(&mut self, t: &mut Type) {
        match t {
            Type::Named(n, args) => {
                if let Some(new) = self.rn(n) {
                    *n = new;
                }
                for a in args {
                    self.ty(a);
                }
            }
            Type::Array(i, size) => {
                self.ty(i);
                if let Some(s) = size {
                    self.expr(s);
                }
            }
            Type::Tuple(ts) => {
                for x in ts {
                    self.ty(x);
                }
            }
            Type::Ref { inner, .. } => self.ty(inner),
            Type::Dyn(n, args) => {
                if let Some(new) = self.rn(n) {
                    *n = new;
                }
                for a in args {
                    self.ty(a);
                }
            }
            Type::Fn(ps, r) => {
                for x in ps {
                    self.ty(x);
                }
                self.ty(r);
            }
            Type::Option(i) => self.ty(i),
            Type::Result(o, e) => {
                self.ty(o);
                self.ty(e);
            }
            Type::ConstArg(e) => self.expr(e),
            Type::Primitive(_) | Type::Unit | Type::Never => {}
        }
    }

    fn pat(&mut self, p: &mut Pattern) {
        match p {
            Pattern::TupleStruct(n, subs) => {
                if let Some(new) = self.rn(n) {
                    *n = new;
                }
                for s in subs {
                    self.pat(s);
                }
            }
            Pattern::Ident(n) if n.contains("::") => {
                if let Some(new) = self.rn(n) {
                    *n = new;
                }
            }
            Pattern::Tuple(subs) | Pattern::Or(subs) => {
                for s in subs {
                    self.pat(s);
                }
            }
            Pattern::Array(subs, _) => {
                for s in subs {
                    self.pat(s);
                }
            }
            _ => {}
        }
    }

    fn block(&mut self, b: &mut Block) {
        self.scopes.push(HashSet::new());
        for s in b.stmts.iter_mut() {
            match s {
                Stmt::Let { pattern, ty, value, .. } => {
                    if let Some(t) = ty {
                        self.ty(t);
                    }
                    if let Some(v) = value {
                        self.expr(v);
                    }
                    self.pat(pattern);
                    self.declare(pattern);
                }
                Stmt::Expr(e) | Stmt::Yield(e) => self.expr(e),
                Stmt::Return(v) => {
                    if let Some(v) = v {
                        self.expr(v);
                    }
                }
                Stmt::Break { value, .. } => {
                    if let Some(v) = value {
                        self.expr(v);
                    }
                }
                Stmt::Continue { .. } => {}
                Stmt::Item(it) => self.item(it),
                Stmt::TargetBlock(_, blk) => self.block(blk),
            }
        }
        if let Some(t) = &mut b.tail {
            self.expr(t);
        }
        self.scopes.pop();
    }

    fn func(&mut self, f: &mut FnDecl, with_self: bool) {
        // A nested function cannot see the enclosing function's locals.
        let saved = std::mem::take(&mut self.scopes);
        let mut top = HashSet::new();
        for p in f.params.iter_mut() {
            self.ty(&mut p.ty);
            if let Some(d) = &mut p.default {
                self.expr(d);
            }
            top.insert(p.name.clone());
        }
        if with_self {
            top.insert("self".to_string());
        }
        if let Some(r) = &mut f.return_type {
            self.ty(r);
        }
        self.scopes = vec![top];
        if let Some(b) = &mut f.body {
            self.block(b);
        }
        self.scopes = saved;
    }

    fn item(&mut self, it: &mut Item) {
        match &mut it.kind {
            ItemKind::Fn(f) => self.func(f, false),
            ItemKind::Struct(s) => match &mut s.body {
                StructBody::Named(fs) => {
                    for f in fs {
                        self.ty(&mut f.ty);
                    }
                }
                StructBody::Tuple(ts) => {
                    for t in ts {
                        self.ty(t);
                    }
                }
                StructBody::Unit => {}
            },
            ItemKind::Enum(e) => {
                for v in e.variants.iter_mut() {
                    for t in v.data.iter_mut() {
                        self.ty(t);
                    }
                }
            }
            ItemKind::Const(c) => {
                self.ty(&mut c.ty);
                self.expr(&mut c.value);
            }
            ItemKind::Static(c) => {
                self.ty(&mut c.ty);
                self.expr(&mut c.value);
            }
            ItemKind::TypeAlias(a) => self.ty(&mut a.ty),
            ItemKind::Trait(t) => {
                for ti in t.items.iter_mut() {
                    if let TraitItem::Fn(f) = ti {
                        self.func(f, true);
                    }
                }
            }
            ItemKind::Impl(im) => {
                if let Some(new) = self.rn(&im.target.name) {
                    im.target.name = new;
                }
                for a in im.target.args.iter_mut() {
                    self.ty(a);
                }
                if let Some(tr) = &mut im.trait_ref {
                    if let Some(new) = self.rn(&tr.name) {
                        tr.name = new;
                    }
                    for a in tr.args.iter_mut() {
                        self.ty(a);
                    }
                }
                for ii in im.items.iter_mut() {
                    match ii {
                        ImplItem::Fn(f) => self.func(f, true),
                        ImplItem::AssocType(_, t) => self.ty(t),
                    }
                }
            }
            _ => {}
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Ident(n) => {
                if !self.shadowed(n) {
                    if let Some(new) = self.rn(n) {
                        *n = new;
                    }
                }
            }
            Expr::Path(segs) => {
                if let Some(first) = segs.first_mut() {
                    if let Some(new) = self.map.get(first.as_str()) {
                        *first = new.clone();
                    }
                }
            }
            Expr::StructLit { name, fields, spread } => {
                if let Some(new) = self.rn(name) {
                    *name = new;
                }
                for (_, x) in fields.iter_mut() {
                    self.expr(x);
                }
                if let Some(s) = spread {
                    self.expr(s);
                }
            }
            Expr::Cast { expr, ty } => {
                self.ty(ty);
                self.expr(expr);
            }
            Expr::Match(m) => {
                self.expr(&mut m.scrutinee);
                for arm in m.arms.iter_mut() {
                    self.scopes.push(HashSet::new());
                    for p in arm.patterns.iter_mut() {
                        self.pat(p);
                    }
                    if let Some(first) = arm.patterns.first() {
                        let first = first.clone();
                        self.declare(&first);
                    }
                    if let Some(g) = &mut arm.guard {
                        self.expr(g);
                    }
                    match &mut arm.body {
                        MatchArmBody::Expr(x) => self.expr(x),
                        MatchArmBody::Block(b) => self.block(b),
                    }
                    self.scopes.pop();
                }
            }
            Expr::Loop(l) => match l.as_mut() {
                LoopExpr::For { pattern, iter, body, .. } => {
                    self.expr(iter);
                    self.scopes.push(HashSet::new());
                    self.pat(pattern);
                    let p = pattern.clone();
                    self.declare(&p);
                    self.block(body);
                    self.scopes.pop();
                }
                LoopExpr::Loop { body, .. } => self.block(body),
                LoopExpr::While { cond, body, .. } => {
                    self.expr(cond);
                    self.block(body);
                }
                LoopExpr::DoWhile { body, cond } => {
                    self.block(body);
                    self.expr(cond);
                }
            },
            Expr::TryCatch { try_block, catch_var, catch_block } => {
                self.block(try_block);
                self.scopes.push(HashSet::new());
                self.scopes.last_mut().unwrap().insert(catch_var.clone());
                self.block(catch_block);
                self.scopes.pop();
            }
            Expr::Block(b) | Expr::Unsafe(b) => self.block(b),
            other => {
                for c in expr_children_mut(other) {
                    match c {
                        ChildMut::E(x) => self.expr(x),
                        ChildMut::B(b) => self.block(b),
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The pass
// ---------------------------------------------------------------------------

fn process_fn(f: &mut FnDecl, owner: &str, with_self: bool, colliding: &HashSet<String>) {
    let Some(body) = &mut f.body else { return };
    // 1. Which nested items need a new name?
    let mut map: HashMap<String, String> = HashMap::new();
    {
        let mut nested: Vec<&mut Item> = Vec::new();
        block_items(body, &mut nested);
        for it in nested.iter_mut() {
            if let Some(n) = item_name(it).map(|s| s.to_string()) {
                if colliding.contains(&n) {
                    let new = format!("{}${}", owner, n);
                    map.insert(n, new.clone());
                    set_item_name(it, new);
                }
            }
        }
    }
    // 2. Rewrite references inside this function's body (and the nested items).
    if !map.is_empty() {
        let mut r = Renamer { map: &map, scopes: Vec::new() };
        let mut top = HashSet::new();
        for p in &f.params {
            top.insert(p.name.clone());
        }
        if with_self {
            top.insert("self".to_string());
        }
        r.scopes = vec![top];
        if let Some(b) = &mut f.body {
            r.block(b);
        }
    }
    // 3. Items nested in nested items.
    if let Some(body) = &mut f.body {
        let mut nested: Vec<&mut Item> = Vec::new();
        block_items(body, &mut nested);
        for it in nested {
            let name = item_name(it).unwrap_or("impl").to_string();
            match &mut it.kind {
                ItemKind::Fn(g) => process_fn(g, &format!("{}${}", owner, name), false, colliding),
                ItemKind::Impl(im) => {
                    let tn = im.target.name.clone();
                    for ii in im.items.iter_mut() {
                        if let ImplItem::Fn(g) = ii {
                            let o = format!("{}${}${}", owner, tn, g.name);
                            process_fn(g, &o, true, colliding);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Renames colliding nested items (see the module docs). Run before type checking.
pub fn scope_nested_items(program: &mut Program) {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for it in crate::desugar::all_items(program) {
        if let Some(n) = item_name(it) {
            *counts.entry(n.to_string()).or_insert(0) += 1;
        }
    }
    let colliding: HashSet<String> = counts.into_iter().filter(|(_, c)| *c > 1).map(|(n, _)| n).collect();
    if colliding.is_empty() {
        return;
    }
    let mut counter_guard = 0usize;
    for it in program.items.iter_mut() {
        counter_guard += 1;
        let _ = counter_guard;
        match &mut it.kind {
            ItemKind::Fn(f) => {
                let owner = f.name.clone();
                process_fn(f, &owner, false, &colliding);
            }
            ItemKind::Impl(im) => {
                let tn = im.target.name.clone();
                for ii in im.items.iter_mut() {
                    if let ImplItem::Fn(g) = ii {
                        let o = format!("{}${}", tn, g.name);
                        process_fn(g, &o, true, &colliding);
                    }
                }
            }
            _ => {}
        }
    }
}
