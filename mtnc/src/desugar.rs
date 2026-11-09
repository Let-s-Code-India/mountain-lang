//! `try`/`catch` desugaring (Phase 10; Document 11 §3, §3.1, §8).
//!
//! Document 11 §3 defines `try { B } catch (e) { C }` as a *source-to-source
//! transformation* into ordinary `Result`-returning code:
//!
//! ```text
//! fn __try_block_N(<captured locals>) -> Result<V, E> { B' ; return Ok(<tail or ()>); }
//! match __try_block_N(<captured locals>) { Ok(_) => {}, Err(e) => { C } }
//! ```
//!
//! where `B'` is `B` with every `throw x` rewritten to `return Err(x)`
//! (Document 11 §3.1). This module performs exactly that rewrite on the AST,
//! *before* any IR is generated, so there is one error-propagation path in
//! the compiled output (Document 11 §3, Pillar I).
//!
//! Where the document's wrapper is a free function, a block that reads local
//! variables of the enclosing function needs them handed over: the wrapper
//! takes each such variable as a by-value parameter. Document 11 does not
//! discuss captures at all (its example only calls free functions), so this
//! is a flagged interpretation, see PROGRESS.md Phase 10.
//!
//! NORMALIZATION RULE (used by the IR-identity test, `tests/e2e.rs`): the
//! synthesized wrapper of the N-th `try` expression of the program (counting
//! in source order, pre-order, from 0) is named exactly `__try_block_N`, and is
//! inserted immediately before the item that contains the `try`. A hand-written
//! program that spells out the same wrapper under that name, in that position,
//! produces byte-identical LLVM IR.
//!
//! A local the block assigns to or mutably borrows is passed as a `borrow mut`
//! parameter (Document 11 §8.9, Phase 11b-1). Limit (reported as a "not yet
//! supported by codegen" error, never miscompiled): `break`/`continue` that
//! leave the `try` block (they cannot cross the wrapper function).

use crate::ast::*;
use crate::types::{Ty, TypeChecker};
use std::collections::HashSet;

pub(crate) enum ChildMut<'a> {
    E(&'a mut Expr),
    B(&'a mut Block),
}

pub(crate) enum Child<'a> {
    E(&'a Expr),
    B(&'a Block),
}

fn if_children_mut<'a>(i: &'a mut IfExpr, out: &mut Vec<ChildMut<'a>>) {
    out.push(ChildMut::E(&mut i.cond));
    out.push(ChildMut::B(&mut i.then_block));
    match &mut i.else_branch {
        Some(ElseBranch::If(inner)) => if_children_mut(inner, out),
        Some(ElseBranch::Block(b)) => out.push(ChildMut::B(b)),
        None => {}
    }
}

fn if_children<'a>(i: &'a IfExpr, out: &mut Vec<Child<'a>>) {
    out.push(Child::E(&i.cond));
    out.push(Child::B(&i.then_block));
    match &i.else_branch {
        Some(ElseBranch::If(inner)) => if_children(inner, out),
        Some(ElseBranch::Block(b)) => out.push(Child::B(b)),
        None => {}
    }
}

fn block_children_mut(b: &mut Block) -> Vec<ChildMut<'_>> {
    let mut out = Vec::new();
    for s in b.stmts.iter_mut() {
        match s {
            Stmt::Let { value, .. } => {
                if let Some(v) = value {
                    out.push(ChildMut::E(v));
                }
            }
            Stmt::Expr(e) | Stmt::Yield(e) => out.push(ChildMut::E(e)),
            Stmt::Return(v) => {
                if let Some(v) = v {
                    out.push(ChildMut::E(v));
                }
            }
            Stmt::Break { value, .. } => {
                if let Some(v) = value {
                    out.push(ChildMut::E(v));
                }
            }
            Stmt::Continue { .. } | Stmt::Item(_) => {}
            Stmt::TargetBlock(_, blk) => out.push(ChildMut::B(blk)),
        }
    }
    if let Some(t) = &mut b.tail {
        out.push(ChildMut::E(t));
    }
    out
}

pub(crate) fn block_children(b: &Block) -> Vec<Child<'_>> {
    let mut out = Vec::new();
    for s in b.stmts.iter() {
        match s {
            Stmt::Let { value, .. } => {
                if let Some(v) = value {
                    out.push(Child::E(v));
                }
            }
            Stmt::Expr(e) | Stmt::Yield(e) => out.push(Child::E(e)),
            Stmt::Return(v) => {
                if let Some(v) = v {
                    out.push(Child::E(v));
                }
            }
            Stmt::Break { value, .. } => {
                if let Some(v) = value {
                    out.push(Child::E(v));
                }
            }
            Stmt::Continue { .. } | Stmt::Item(_) => {}
            Stmt::TargetBlock(_, blk) => out.push(Child::B(blk)),
        }
    }
    if let Some(t) = &b.tail {
        out.push(Child::E(t));
    }
    out
}

/// Direct sub-expressions / sub-blocks of `e`. Constructs the compiler cannot
/// lower yet (closures, spawn, ui, ...) report no children: codegen rejects
/// them with its own error, so nothing inside them needs rewriting.
pub(crate) fn expr_children_mut(e: &mut Expr) -> Vec<ChildMut<'_>> {
    let mut out = Vec::new();
    match e {
        Expr::Paren(x) | Expr::Await(x) | Expr::Propagate(x) | Expr::Throw(x) | Expr::Yield(x) | Expr::Deref(x) => out.push(ChildMut::E(x)),
        Expr::Tuple(v) | Expr::Array(v) => {
            for x in v.iter_mut() {
                out.push(ChildMut::E(x));
            }
        }
        Expr::ArrayRepeat(x, n) => {
            out.push(ChildMut::E(x));
            out.push(ChildMut::E(n));
        }
        Expr::StructLit { fields, spread, .. } => {
            for (_, x) in fields.iter_mut() {
                out.push(ChildMut::E(x));
            }
            if let Some(s) = spread {
                out.push(ChildMut::E(s));
            }
        }
        Expr::If(i) => if_children_mut(i, &mut out),
        Expr::Match(m) => {
            out.push(ChildMut::E(&mut m.scrutinee));
            for arm in m.arms.iter_mut() {
                if let Some(g) = &mut arm.guard {
                    out.push(ChildMut::E(g));
                }
                match &mut arm.body {
                    MatchArmBody::Expr(x) => out.push(ChildMut::E(x)),
                    MatchArmBody::Block(b) => out.push(ChildMut::B(b)),
                }
            }
        }
        Expr::Loop(l) => match l.as_mut() {
            LoopExpr::Loop { body, .. } => out.push(ChildMut::B(body)),
            LoopExpr::While { cond, body, .. } => {
                out.push(ChildMut::E(cond));
                out.push(ChildMut::B(body));
            }
            LoopExpr::For { iter, body, .. } => {
                out.push(ChildMut::E(iter));
                out.push(ChildMut::B(body));
            }
            LoopExpr::DoWhile { body, cond } => {
                out.push(ChildMut::B(body));
                out.push(ChildMut::E(cond));
            }
        },
        Expr::Block(b) | Expr::Unsafe(b) => out.push(ChildMut::B(b)),
        Expr::Borrow { expr, .. } | Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { expr, .. } => out.push(ChildMut::E(expr)),
        Expr::Binary { lhs, rhs, .. } | Expr::Assign { lhs, rhs, .. } => {
            out.push(ChildMut::E(lhs));
            out.push(ChildMut::E(rhs));
        }
        Expr::Range { lo, hi, .. } => {
            out.push(ChildMut::E(lo));
            out.push(ChildMut::E(hi));
        }
        Expr::Index { expr, index } => {
            out.push(ChildMut::E(expr));
            out.push(ChildMut::E(index));
        }
        Expr::Call { callee, args } => {
            out.push(ChildMut::E(callee));
            for a in args.iter_mut() {
                out.push(ChildMut::E(&mut a.value));
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            out.push(ChildMut::E(receiver));
            for a in args.iter_mut() {
                out.push(ChildMut::E(&mut a.value));
            }
        }
        Expr::TryCatch { try_block, catch_block, .. } => {
            out.push(ChildMut::B(try_block));
            out.push(ChildMut::B(catch_block));
        }
        Expr::Return(v) => {
            if let Some(v) = v {
                out.push(ChildMut::E(v));
            }
        }
        _ => {}
    }
    out
}

pub(crate) fn expr_children(e: &Expr) -> Vec<Child<'_>> {
    let mut out = Vec::new();
    match e {
        Expr::Paren(x) | Expr::Await(x) | Expr::Propagate(x) | Expr::Throw(x) | Expr::Yield(x) | Expr::Deref(x) => out.push(Child::E(x)),
        Expr::Tuple(v) | Expr::Array(v) => {
            for x in v.iter() {
                out.push(Child::E(x));
            }
        }
        Expr::ArrayRepeat(x, n) => {
            out.push(Child::E(x));
            out.push(Child::E(n));
        }
        Expr::StructLit { fields, spread, .. } => {
            for (_, x) in fields.iter() {
                out.push(Child::E(x));
            }
            if let Some(s) = spread {
                out.push(Child::E(s));
            }
        }
        Expr::If(i) => if_children(i, &mut out),
        Expr::Match(m) => {
            out.push(Child::E(&m.scrutinee));
            for arm in m.arms.iter() {
                if let Some(g) = &arm.guard {
                    out.push(Child::E(g));
                }
                match &arm.body {
                    MatchArmBody::Expr(x) => out.push(Child::E(x)),
                    MatchArmBody::Block(b) => out.push(Child::B(b)),
                }
            }
        }
        Expr::Loop(l) => match l.as_ref() {
            LoopExpr::Loop { body, .. } => out.push(Child::B(body)),
            LoopExpr::While { cond, body, .. } => {
                out.push(Child::E(cond));
                out.push(Child::B(body));
            }
            LoopExpr::For { iter, body, .. } => {
                out.push(Child::E(iter));
                out.push(Child::B(body));
            }
            LoopExpr::DoWhile { body, cond } => {
                out.push(Child::B(body));
                out.push(Child::E(cond));
            }
        },
        Expr::Block(b) | Expr::Unsafe(b) => out.push(Child::B(b)),
        Expr::Borrow { expr, .. } | Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Field { expr, .. } => out.push(Child::E(expr)),
        Expr::Binary { lhs, rhs, .. } | Expr::Assign { lhs, rhs, .. } => {
            out.push(Child::E(lhs));
            out.push(Child::E(rhs));
        }
        Expr::Range { lo, hi, .. } => {
            out.push(Child::E(lo));
            out.push(Child::E(hi));
        }
        Expr::Index { expr, index } => {
            out.push(Child::E(expr));
            out.push(Child::E(index));
        }
        Expr::Call { callee, args } => {
            out.push(Child::E(callee));
            for a in args.iter() {
                out.push(Child::E(&a.value));
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            out.push(Child::E(receiver));
            for a in args.iter() {
                out.push(Child::E(&a.value));
            }
        }
        Expr::TryCatch { try_block, catch_block, .. } => {
            out.push(Child::B(try_block));
            out.push(Child::B(catch_block));
        }
        Expr::Return(v) => {
            if let Some(v) = v {
                out.push(Child::E(v));
            }
        }
        _ => {}
    }
    out
}

/// True if any `try`/`catch` expression occurs anywhere in `program`'s
/// function bodies (so the driver can skip the pass and the second check).
pub fn contains_try(program: &Program) -> bool {
    fn in_expr(e: &Expr) -> bool {
        if matches!(e, Expr::TryCatch { .. }) {
            return true;
        }
        expr_children(e).into_iter().any(|c| match c {
            Child::E(x) => in_expr(x),
            Child::B(b) => in_block(b),
        })
    }
    fn in_block(b: &Block) -> bool {
        block_children(b).into_iter().any(|c| match c {
            Child::E(x) => in_expr(x),
            Child::B(b) => in_block(b),
        })
    }
    program.items.iter().any(|it| match &it.kind {
        ItemKind::Fn(f) => f.body.as_ref().map(in_block).unwrap_or(false),
        _ => false,
    })
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

/// Free-variable analysis of one `try` block (scoped: a `let` shadows only
/// after its initializer, match-arm / `for` bindings only inside their body).
struct Free {
    scopes: Vec<HashSet<String>>,
    /// (name, address of the first `Expr::Ident` use)
    captured: Vec<(String, *const Expr)>,
    assigned: HashSet<String>,
    loop_labels: Vec<Option<String>>,
    errors: Vec<String>,
    /// jumps (is_break, label) that leave the block (Doc 12 §11.6)
    escapes: Vec<(bool, Option<String>)>,
}

fn jump_targets_inner_loop(labels: &[Option<String>], label: &Option<String>) -> bool {
    match label {
        None => !labels.is_empty(),
        Some(l) => labels.iter().any(|x| x.as_deref() == Some(l.as_str())),
    }
}

impl Free {
    fn bound(&self, n: &str) -> bool {
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

    fn root_ident(e: &Expr) -> Option<&str> {
        match e {
            Expr::Ident(n) => Some(n.as_str()),
            Expr::Field { expr, .. } | Expr::Index { expr, .. } | Expr::Paren(expr) => Self::root_ident(expr),
            _ => None,
        }
    }

    fn block(&mut self, b: &Block) {
        self.scopes.push(HashSet::new());
        for s in &b.stmts {
            match s {
                Stmt::Let { pattern, value, .. } => {
                    if let Some(v) = value {
                        self.expr(v);
                    }
                    self.declare(pattern);
                }
                Stmt::Expr(e) | Stmt::Yield(e) => self.expr(e),
                Stmt::Return(v) => {
                    if let Some(v) = v {
                        self.expr(v);
                    }
                }
                Stmt::Break { label, value } => {
                    if let Some(v) = value {
                        self.expr(v);
                    }
                    self.check_jump(label, "break");
                }
                Stmt::Continue { label } => self.check_jump(label, "continue"),
                Stmt::TargetBlock(_, blk) => self.block(blk),
                Stmt::Item(_) => {}
            }
        }
        if let Some(t) = &b.tail {
            self.expr(t);
        }
        self.scopes.pop();
    }

    fn check_jump(&mut self, label: &Option<String>, what: &str) {
        // A jump that leaves the `try` block (Doc 12 §11.6) is recorded; `lower_try` turns it
        // into a control signal returned by the wrapper.
        if !jump_targets_inner_loop(&self.loop_labels, label) {
            self.escapes.push((what == "break", label.clone()));
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Ident(n) => {
                if n.as_str() != "None" && !self.bound(n) && !self.captured.iter().any(|(c, _)| c == n) {
                    self.captured.push((n.clone(), e as *const Expr));
                }
            }
            Expr::Call { callee, args } => {
                // A bare-identifier callee names a function, not a local.
                if !matches!(callee.as_ref(), Expr::Ident(_)) {
                    self.expr(callee);
                }
                for a in args {
                    self.expr(&a.value);
                }
            }
            Expr::Assign { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
                if let Some(r) = Self::root_ident(lhs) {
                    if !self.bound(r) {
                        self.assigned.insert(r.to_string());
                    }
                }
            }
            Expr::Borrow { mutable: true, expr } => {
                self.expr(expr);
                if let Some(r) = Self::root_ident(expr) {
                    if !self.bound(r) {
                        self.assigned.insert(r.to_string());
                    }
                }
            }
            Expr::Match(m) => {
                self.expr(&m.scrutinee);
                for arm in &m.arms {
                    self.scopes.push(HashSet::new());
                    for p in &arm.patterns {
                        self.declare(p);
                    }
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    match &arm.body {
                        MatchArmBody::Expr(x) => self.expr(x),
                        MatchArmBody::Block(b) => self.block(b),
                    }
                    self.scopes.pop();
                }
            }
            Expr::Loop(l) => match l.as_ref() {
                LoopExpr::Loop { label, body } => {
                    self.loop_labels.push(label.clone());
                    self.block(body);
                    self.loop_labels.pop();
                }
                LoopExpr::While { label, cond, body } => {
                    self.expr(cond);
                    self.loop_labels.push(label.clone());
                    self.block(body);
                    self.loop_labels.pop();
                }
                LoopExpr::For { label, pattern, iter, body } => {
                    self.expr(iter);
                    self.scopes.push(HashSet::new());
                    self.declare(pattern);
                    self.loop_labels.push(label.clone());
                    self.block(body);
                    self.loop_labels.pop();
                    self.scopes.pop();
                }
                LoopExpr::DoWhile { body, cond } => {
                    self.loop_labels.push(None);
                    self.block(body);
                    self.loop_labels.pop();
                    self.expr(cond);
                }
            },
            Expr::TryCatch { try_block, catch_var, catch_block } => {
                // An inner `try` is its own wrapper later; for the OUTER
                // wrapper its variables are still free uses of this block.
                // A jump inside it that leaves it may also leave the outer block (the inner
                // `try` is lowered first into a block holding such a jump).
                self.block(try_block);
                self.scopes.push(HashSet::new());
                self.scopes.last_mut().unwrap().insert(catch_var.clone());
                self.block(catch_block);
                self.scopes.pop();
            }
            other => {
                for c in expr_children(other) {
                    match c {
                        Child::E(x) => self.expr(x),
                        Child::B(b) => self.block(b),
                    }
                }
            }
        }
    }
}

/// `Ty` -> syntactic `Type`, for the synthesized wrapper's signature.
pub fn ty_to_type(t: &Ty) -> Option<Type> {
    Some(match t {
        Ty::I8 => Type::Primitive("i8".into()),
        Ty::I16 => Type::Primitive("i16".into()),
        Ty::I32 => Type::Primitive("i32".into()),
        Ty::I64 => Type::Primitive("i64".into()),
        Ty::I128 => Type::Primitive("i128".into()),
        Ty::Isize => Type::Primitive("isize".into()),
        Ty::U8 => Type::Primitive("u8".into()),
        Ty::U16 => Type::Primitive("u16".into()),
        Ty::U32 => Type::Primitive("u32".into()),
        Ty::U64 => Type::Primitive("u64".into()),
        Ty::U128 => Type::Primitive("u128".into()),
        Ty::Usize => Type::Primitive("usize".into()),
        Ty::F32 => Type::Primitive("f32".into()),
        Ty::F64 => Type::Primitive("f64".into()),
        Ty::Bool => Type::Primitive("bool".into()),
        Ty::Char => Type::Primitive("char".into()),
        Ty::StringTy => Type::Primitive("String".into()),
        Ty::Str => Type::Primitive("str".into()),
        Ty::Unit => Type::Unit,
        Ty::Never => Type::Never,
        Ty::Named(n) => Type::Named(n.clone(), Vec::new()),
        Ty::Assoc(b, n) => Type::Assoc(Box::new(ty_to_type(b)?), n.clone()),
        Ty::Generic(n, args) => {
            let mut v = Vec::new();
            for a in args {
                match a {
                    crate::types::GenericArg::Type(t) => v.push(ty_to_type(t)?),
                    crate::types::GenericArg::Const(c) => v.push(Type::ConstArg(Box::new(Expr::Literal(Literal::Int(c.to_string()))))),
                }
            }
            Type::Named(n.clone(), v)
        }
        Ty::Tuple(ts) => Type::Tuple(ts.iter().map(ty_to_type).collect::<Option<Vec<_>>>()?),
        Ty::OptionTy(i) => Type::Option(Box::new(ty_to_type(i)?)),
        Ty::ResultTy(o, e) => Type::Result(Box::new(ty_to_type(o)?), Box::new(ty_to_type(e)?)),
        Ty::Ref(m, i) => Type::Ref { lifetime: None, mutable: *m, inner: Box::new(ty_to_type(i)?) },
        Ty::RawPtr(m, i) => Type::RawPtr { mutable: *m, inner: Box::new(ty_to_type(i)?) },
        Ty::Array(i) => Type::Array(Box::new(ty_to_type(i)?), None),
        Ty::Fixed(i, n) => Type::Array(Box::new(ty_to_type(i)?), Some(Box::new(Expr::Literal(Literal::Int(n.to_string()))))),
        Ty::ConstVal(n) => Type::ConstArg(Box::new(Expr::Literal(Literal::Int(n.to_string())))),
        _ => return None,
    })
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::Ident(name.to_string())),
        args: args.into_iter().map(|value| Arg { name: None, value }).collect(),
    }
}

struct Ctx<'a> {
    tc: &'a TypeChecker,
    counter: usize,
    new_fns: Vec<Item>,
    errors: Vec<String>,
}

impl Ctx<'_> {
    /// `throw x` -> `return Err(x)` (Document 11 §3.1), not descending into a
    /// nested `try`'s try-block (its `throw`s belong to that inner wrapper),
    /// but descending into its `catch` block (which runs in THIS context).
    fn rewrite_throws(e: &mut Expr) {
        match e {
            Expr::Throw(_) => {
                let inner = match std::mem::replace(e, Expr::Tuple(Vec::new())) {
                    Expr::Throw(x) => *x,
                    _ => unreachable!(),
                };
                let mut inner = inner;
                Self::rewrite_throws(&mut inner);
                *e = Expr::Return(Some(Box::new(call("Err", vec![inner]))));
            }
            Expr::TryCatch { catch_block, .. } => Self::rewrite_throws_block(catch_block),
            other => {
                for c in expr_children_mut(other) {
                    match c {
                        ChildMut::E(x) => Self::rewrite_throws(x),
                        ChildMut::B(b) => Self::rewrite_throws_block(b),
                    }
                }
            }
        }
    }

    fn rewrite_throws_block(b: &mut Block) {
        for c in block_children_mut(b) {
            match c {
                ChildMut::E(x) => Self::rewrite_throws(x),
                ChildMut::B(b) => Self::rewrite_throws_block(b),
            }
        }
    }

    fn block(&mut self, b: &mut Block) {
        for c in block_children_mut(b) {
            match c {
                ChildMut::E(x) => self.expr(x),
                ChildMut::B(b) => self.block(b),
            }
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        if matches!(e, Expr::TryCatch { .. }) {
            self.lower_try(e);
            return;
        }
        for c in expr_children_mut(e) {
            match c {
                ChildMut::E(x) => self.expr(x),
                ChildMut::B(b) => self.block(b),
            }
        }
    }

    fn lower_try(&mut self, e: &mut Expr) {
        let key = e as *const Expr as usize;
        let n = self.counter;
        self.counter += 1;
        let Some((ok_ty, err_ty)) = self.tc.try_info.get(&key).cloned() else {
            self.errors.push("internal: no resolved wrapper type recorded for a `try` expression".into());
            return;
        };

        // 1. Captured locals of the enclosing function.
        let mut free = Free { scopes: vec![HashSet::new()], captured: Vec::new(), assigned: HashSet::new(), loop_labels: Vec::new(), errors: Vec::new(), escapes: Vec::new() };
        if let Expr::TryCatch { try_block, .. } = &*e {
            free.block(try_block);
        }
        self.errors.append(&mut free.errors);
        let mut params = Vec::new();
        let mut cap_names: Vec<(String, bool)> = Vec::new();
        for (name, ptr) in &free.captured {
            // A local the block assigns or mutably borrows is passed as a `borrow mut`
            // parameter (Document 11 §8.9); a local it only reads is passed by value.
            let by_ref = free.assigned.contains(name);
            // SAFETY-free lookup: the address is only used as a map key.
            let ty = self.tc.expr_types.get(&(*ptr as usize)).cloned();
            let ast_ty = ty.as_ref().and_then(ty_to_type);
            match ast_ty {
                Some(t) => {
                    let ownership = if by_ref { OwnershipMod::BorrowMut } else { OwnershipMod::None };
                    params.push(Param { ownership, name: name.clone(), is_variadic: false, ty: t, default: None, is_mut: false });
                    cap_names.push((name.clone(), by_ref));
                }
                None => self.errors.push(format!("cannot pass `{}` into the desugared `try` wrapper: its type is not supported by codegen yet", name)),
            }
        }

        let (ok_t, err_t) = match (ty_to_type(&ok_ty), ty_to_type(&err_ty)) {
            (Some(o), Some(r)) => (o, r),
            _ => {
                self.errors.push(format!("the `try` block's types `{}`/`{}` are not supported by codegen yet", ok_ty, err_ty));
                return;
            }
        };

        // 2. Take the pieces out of the node.
        let placeholder = Expr::Tuple(Vec::new());
        let (mut try_block, catch_var, mut catch_block) = match std::mem::replace(e, placeholder) {
            Expr::TryCatch { try_block, catch_var, catch_block } => (try_block, catch_var, catch_block),
            _ => unreachable!(),
        };
        let unit_valued = ok_ty == Ty::Unit;
        let never_valued = {
            // A try block that cannot fall through recorded `()` as its value
            // type; detect it again so the Ok arm can be marked unreachable.
            try_block.tail.is_none() && matches!(try_block.stmts.last(), Some(Stmt::Return(_)) | Some(Stmt::Break { .. }) | Some(Stmt::Continue { .. }))
        };

        // 3. Build the wrapper: B' ; return Ok(tail or ()).
        // Nested `try`s are lowered FIRST (numbered after this one), while
        // every node still sits at the address the type checker saw it at
        // (moving the tail expression below would change its address).
        self.block(&mut try_block);
        Self::rewrite_throws_block(&mut try_block);
        // Doc 12 §11.6: `break`/`continue` that leave the block become "set the control variable,
        // return Ok(None)"; the caller acts on the signal after the call. Only then is the
        // wrapper's value wrapped in an `Option`, so ordinary `try` blocks lower exactly as before.
        let ctl_name = format!("__try_ctl_{}", n);
        let mut jumps: Vec<(bool, Option<String>)> = Vec::new();
        let has_escape = {
            let mut labels: Vec<Option<String>> = Vec::new();
            let mut errs = Vec::new();
            rewrite_jumps_block(&mut try_block, &mut labels, &ctl_name, &mut jumps, &mut errs);
            self.errors.append(&mut errs);
            !jumps.is_empty()
        };
        let tail = try_block.tail.take();
        let ok_value = match tail {
            Some(t) => *t,
            None => Expr::Tuple(Vec::new()),
        };
        let ok_value = if has_escape { call("Some", vec![ok_value]) } else { ok_value };
        try_block.stmts.push(Stmt::Return(Some(call("Ok", vec![ok_value]))));
        let ok_t = if has_escape { Type::Option(Box::new(ok_t)) } else { ok_t };
        let mut params = params;
        if has_escape {
            params.push(Param { ownership: OwnershipMod::BorrowMut, name: ctl_name.clone(), is_variadic: false, ty: Type::Primitive("i32".into()), default: None, is_mut: false });
        }
        let name = format!("__try_block_{}", n);
        self.new_fns.push(Item {
            attrs: Vec::new(),
            visibility: Visibility::Private,
            kind: ItemKind::Fn(FnDecl {
                is_async: false,
                name: name.clone(),
                generics: GenericParams::default(),
                params,
                return_type: Some(Type::Result(Box::new(ok_t), Box::new(err_t))),
                where_clause: WhereClause::default(),
                body: Some(try_block),
            }),
            span: crate::token::Span { line: 0, col: 0 },
        });

        // 4. The replacement `match` (Document 11 §3's shape).
        self.block(&mut catch_block);
        let ok_arm_body = if never_valued {
            MatchArmBody::Block(Block { stmts: vec![Stmt::Expr(call("panic", vec![Expr::Literal(Literal::Str("\"unreachable: `try` block cannot complete\"".into()))]))], tail: None })
        } else if unit_valued {
            MatchArmBody::Block(Block { stmts: Vec::new(), tail: None })
        } else {
            MatchArmBody::Expr(Box::new(Expr::Ident("__v".into())))
        };
        let ok_pat = if unit_valued || never_valued { Pattern::Wildcard } else { Pattern::Ident("__v".into()) };
        let mut call_args: Vec<Expr> = cap_names
            .iter()
            .map(|(c, by_ref)| if *by_ref { Expr::Borrow { mutable: true, expr: Box::new(Expr::Ident(c.clone())) } } else { Expr::Ident(c.clone()) })
            .collect();
        if has_escape {
            call_args.push(Expr::Borrow { mutable: true, expr: Box::new(Expr::Ident(ctl_name.clone())) });
        }
        let ok_pattern = if has_escape { Pattern::TupleStruct("Ok".into(), vec![Pattern::TupleStruct("Some".into(), vec![ok_pat])]) } else { Pattern::TupleStruct("Ok".into(), vec![ok_pat]) };
        let mut arms = vec![MatchArm { patterns: vec![ok_pattern], guard: None, body: ok_arm_body }];
        if has_escape {
            // Ok(None): act on the recorded jump.
            let mut stmts = Vec::new();
            for (i, (is_break, label)) in jumps.iter().enumerate() {
                let jump = if *is_break { Stmt::Break { label: label.clone(), value: None } } else { Stmt::Continue { label: label.clone() } };
                stmts.push(Stmt::Expr(Expr::If(Box::new(IfExpr {
                    cond: Expr::Binary { op: BinaryOp::EqEq, lhs: Box::new(Expr::Ident(ctl_name.clone())), rhs: Box::new(Expr::Literal(Literal::Int((i + 1).to_string()))) },
                    then_block: Block { stmts: vec![jump], tail: None },
                    else_branch: None,
                }))));
            }
            let dispatch = Block { stmts, tail: Some(Box::new(call("panic", vec![Expr::Literal(Literal::Str("\"unreachable: unknown `try` jump\"".into()))]))) };
            arms.push(MatchArm { patterns: vec![Pattern::TupleStruct("Ok".into(), vec![Pattern::Ident("None".into())])], guard: None, body: MatchArmBody::Block(dispatch) });
        }
        arms.push(MatchArm { patterns: vec![Pattern::TupleStruct("Err".into(), vec![Pattern::Ident(catch_var)])], guard: None, body: MatchArmBody::Block(catch_block) });
        let m = Expr::Match(Box::new(MatchExpr { scrutinee: call(&name, call_args), arms }));
        *e = if has_escape {
            Expr::Block(Box::new(Block {
                stmts: vec![Stmt::Let { mutable: true, pattern: Pattern::Ident(ctl_name.clone()), ty: Some(Type::Primitive("i32".into())), value: Some(Expr::Literal(Literal::Int("0".into()))) }],
                tail: Some(Box::new(m)),
            }))
        } else {
            m
        };
    }
}

/// Replaces the `break`/`continue` statements of a lowered `try` block that leave the block
/// by `{ ctl = k; return Ok(None); }` (Doc 12 §11.6); `jumps` lists the distinct targets,
/// `k` is the 1-based index.
fn rewrite_jumps_block(b: &mut Block, labels: &mut Vec<Option<String>>, ctl: &str, jumps: &mut Vec<(bool, Option<String>)>, errs: &mut Vec<String>) {
    for c in block_children_mut(b) {
        match c {
            ChildMut::E(x) => rewrite_jumps_expr(x, labels, ctl, jumps, errs),
            ChildMut::B(bb) => rewrite_jumps_block(bb, labels, ctl, jumps, errs),
        }
    }
    let old = std::mem::take(&mut b.stmts);
    for s in old {
        let (is_break, label) = match &s {
            Stmt::Break { label, value } if !jump_targets_inner_loop(labels, label) => {
                if value.is_some() {
                    errs.push("`break` with a value cannot leave a `try` block (Document 12 §11.6)".into());
                }
                (true, label.clone())
            }
            Stmt::Continue { label } if !jump_targets_inner_loop(labels, label) => (false, label.clone()),
            _ => {
                b.stmts.push(s);
                continue;
            }
        };
        let k = match jumps.iter().position(|j| *j == (is_break, label.clone())) {
            Some(i) => i + 1,
            None => {
                jumps.push((is_break, label.clone()));
                jumps.len()
            }
        };
        b.stmts.push(Stmt::Expr(Expr::Assign {
            op: AssignOp::Eq,
            lhs: Box::new(Expr::Ident(ctl.to_string())),
            rhs: Box::new(Expr::Literal(Literal::Int(k.to_string()))),
        }));
        b.stmts.push(Stmt::Return(Some(call("Ok", vec![Expr::Ident("None".into())]))));
    }
}

fn rewrite_jumps_expr(e: &mut Expr, labels: &mut Vec<Option<String>>, ctl: &str, jumps: &mut Vec<(bool, Option<String>)>, errs: &mut Vec<String>) {
    let pushed = if let Expr::Loop(l) = e {
        let label = match l.as_ref() {
            LoopExpr::Loop { label, .. } | LoopExpr::While { label, .. } | LoopExpr::For { label, .. } => label.clone(),
            LoopExpr::DoWhile { .. } => None,
        };
        labels.push(label);
        true
    } else {
        false
    };
    for c in expr_children_mut(e) {
        match c {
            ChildMut::E(x) => rewrite_jumps_expr(x, labels, ctl, jumps, errs),
            ChildMut::B(bb) => rewrite_jumps_block(bb, labels, ctl, jumps, errs),
        }
    }
    if pushed {
        labels.pop();
    }
}

/// Rewrites every `try`/`catch` expression of `program` in place. The type
/// checker results in `tc` must be for this exact `program` (they are keyed by
/// node address). The caller must re-run the type checker on the result: the
/// synthesized wrapper functions are ordinary source and take the ordinary path.
pub fn desugar_try(program: &mut Program, tc: &TypeChecker) -> Result<(), Vec<String>> {
    let mut ctx = Ctx { tc, counter: 0, new_fns: Vec::new(), errors: Vec::new() };
    let mut out: Vec<Item> = Vec::with_capacity(program.items.len());
    for mut item in std::mem::take(&mut program.items) {
        if let ItemKind::Fn(f) = &mut item.kind {
            if let Some(body) = &mut f.body {
                ctx.block(body);
            }
        }
        out.append(&mut ctx.new_fns);
        out.push(item);
    }
    program.items = out;
    if ctx.errors.is_empty() {
        Ok(())
    } else {
        Err(ctx.errors)
    }
}

/// Every item of `program`: the top-level ones and (hoisted, recursively) the
/// items declared inside function bodies (Phase 11a, Doc 25 §2.5 "nested
/// items inside functions"). Order: an item, then the items nested in its
/// bodies, then the next item.
pub fn all_items(program: &Program) -> Vec<&Item> {
    fn in_block<'a>(b: &'a Block, out: &mut Vec<&'a Item>) {
        for s in &b.stmts {
            match s {
                Stmt::Item(i) => push_item(i, out),
                Stmt::TargetBlock(_, blk) => in_block(blk, out),
                _ => {}
            }
        }
        for c in block_children(b) {
            match c {
                Child::E(e) => in_expr(e, out),
                Child::B(bb) => in_block(bb, out),
            }
        }
    }
    fn in_expr<'a>(e: &'a Expr, out: &mut Vec<&'a Item>) {
        for c in expr_children(e) {
            match c {
                Child::E(x) => in_expr(x, out),
                Child::B(bb) => in_block(bb, out),
            }
        }
    }
    fn push_item<'a>(it: &'a Item, out: &mut Vec<&'a Item>) {
        out.push(it);
        match &it.kind {
            ItemKind::Fn(f) => {
                if let Some(b) = &f.body {
                    in_block(b, out);
                }
            }
            ItemKind::Impl(im) => {
                for ii in &im.items {
                    if let ImplItem::Fn(f) = ii {
                        if let Some(b) = &f.body {
                            in_block(b, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for it in &program.items {
        push_item(it, &mut out);
    }
    out
}

/// Copies every trait default method (Document 7 §4.3) that an `impl Trait for
/// Type` does not override into that impl, so each implementing type gets its own
/// statically-dispatched copy, type-checked with `Self` = that type (Phase 11a;
/// the checker does not check trait default bodies themselves). Returns whether
/// anything changed (the caller then re-checks the program).
pub fn inline_default_methods(program: &mut Program) -> bool {
    inline_defaults(program, false)
}

/// Phase 11c C1b: trait default methods that declare their own type parameters are
/// inlined BEFORE the first check, so their calls are instantiated by `mono` like
/// those of any other generic method.
pub fn inline_generic_default_methods(program: &mut Program) -> bool {
    inline_defaults(program, true)
}

fn inline_defaults(program: &mut Program, only_generic: bool) -> bool {
    use std::collections::HashMap;
    // Traits and impls may be nested in function bodies (Phase 11b-1), so
    // both are collected / patched over the whole item tree.
    let mut defaults: HashMap<String, (Vec<String>, Vec<FnDecl>)> = HashMap::new();
    crate::scope::for_each_item_mut(&mut program.items, &mut |it| {
        if let ItemKind::Trait(t) = &it.kind {
            let fs: Vec<FnDecl> = t
                .items
                .iter()
                .filter_map(|ti| match ti {
                    TraitItem::Fn(f) if f.body.is_some() && (!only_generic || !f.generics.0.is_empty()) => Some(f.clone()),
                    _ => None,
                })
                .collect();
            let gn: Vec<String> = t.generics.0.iter().filter_map(|g| if let GenericParam::Type { name, .. } = g { Some(name.clone()) } else { None }).collect();
            defaults.insert(t.name.clone(), (gn, fs));
        }
    });
    let mut changed = false;
    crate::scope::for_each_item_mut(&mut program.items, &mut |it| {
        if let ItemKind::Impl(im) = &mut it.kind {
            let Some(tr) = &im.trait_ref else { return };
            let Some((gn, fs)) = defaults.get(&tr.name) else { return };
            // a default body of a generic trait mentions the trait's parameters; the impl's
            // trait arguments take their place
            let m: HashMap<String, Type> = gn.iter().cloned().zip(tr.args.iter().cloned()).collect();
            for f in fs {
                let overridden = im.items.iter().any(|ii| matches!(ii, ImplItem::Fn(g) if g.name == f.name));
                if !overridden {
                    let mut f = f.clone();
                    if !m.is_empty() {
                        for p in f.params.iter_mut() {
                            crate::mono::subst_type(&mut p.ty, &m);
                        }
                        if let Some(r) = &mut f.return_type {
                            crate::mono::subst_type(r, &m);
                        }
                        if let Some(b) = &mut f.body {
                            crate::mono::subst_block(b, &m);
                        }
                    }
                    im.items.push(ImplItem::Fn(f));
                    changed = true;
                }
            }
        }
    });
    changed
}
