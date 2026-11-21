//! Document 6 §9.9, rule 5, at check time (Phase 13 M0 follow-up): a type that implements `Drop`
//! cannot be taken apart. Two shapes are reported here, with the same words code generation uses,
//! so `mtnc check` (the editor tool) and `mtnc build` agree:
//!  * a field of an owned `Drop` value moved out (`let n = w.name;`, a by-value call argument, a
//!    struct-literal field, `return w.name`, an assignment's right-hand side, a block tail);
//!  * a pattern that binds an owning field of an owned `Drop` value (`let W(name) = w;`,
//!    `match w { E::A(name) => .. }`).
//! Borrowing the field, reading a `Copy` field and matching through a reference stay allowed.
//! Runs after the type checker, because it needs the type of every expression.

use crate::ast::*;
use crate::types::{Ty, TypeChecker};
use std::collections::HashSet;

pub fn check(program: &Program, tc: &TypeChecker) -> Vec<String> {
    let mut w = Walker { tc, errors: Vec::new(), fns: HashSet::new(), cur: String::new() };
    collect_fns(&program.items, &mut w.fns);
    w.items(&program.items);
    w.errors
}

fn collect_fns(items: &[Item], out: &mut HashSet<String>) {
    for it in items {
        match &it.kind {
            ItemKind::Fn(f) => {
                out.insert(f.name.clone());
            }
            ItemKind::Mod(m) => collect_fns(&m.items, out),
            ItemKind::TargetBlock(_, inner) => collect_fns(inner, out),
            _ => {}
        }
    }
}

struct Walker<'a> {
    tc: &'a TypeChecker,
    errors: Vec<String>,
    /// names of user-declared free functions: a bare field argument to one of them is a move
    fns: HashSet<String>,
    cur: String,
}

impl<'a> Walker<'a> {
    fn err(&mut self, msg: String) {
        self.errors.push(format!("type error in `{}`: {}", self.cur, msg));
    }

    fn items(&mut self, items: &[Item]) {
        for it in items {
            match &it.kind {
                ItemKind::Fn(f) => self.func(f),
                ItemKind::Impl(im) => {
                    for ii in &im.items {
                        if let ImplItem::Fn(f) = ii {
                            self.func(f);
                        }
                    }
                }
                ItemKind::Trait(t) => {
                    for ti in &t.items {
                        if let TraitItem::Fn(f) = ti {
                            self.func(f);
                        }
                    }
                }
                ItemKind::Mod(m) => self.items(&m.items),
                ItemKind::TargetBlock(_, inner) => self.items(inner),
                _ => {}
            }
        }
    }

    fn func(&mut self, f: &FnDecl) {
        if let Some(b) = &f.body {
            let saved = std::mem::replace(&mut self.cur, f.name.clone());
            self.block(b, true);
            self.cur = saved;
        }
    }

    fn block(&mut self, b: &Block, tail_moves: bool) {
        for s in &b.stmts {
            match s {
                Stmt::Let { pattern, value, .. } => {
                    if let Some(v) = value {
                        self.visit(v, true);
                        self.pattern_moves(v, std::slice::from_ref(pattern));
                    }
                }
                Stmt::Expr(e) | Stmt::Yield(e) => self.visit(e, false),
                Stmt::Return(Some(e)) => self.visit(e, true),
                Stmt::Break { value: Some(e), .. } => self.visit(e, true),
                Stmt::Item(it) => self.items(std::slice::from_ref(it.as_ref())),
                Stmt::TargetBlock(_, inner) => self.block(inner, false),
                _ => {}
            }
        }
        if let Some(t) = &b.tail {
            self.visit(t, tail_moves);
        }
    }

    /// `mv`: the value of `e` is moved somewhere (initializer, argument, field, return, tail).
    fn visit(&mut self, e: &Expr, mv: bool) {
        match e {
            Expr::Field { expr: base, .. } => {
                if mv {
                    self.field_move(e, base);
                }
                self.visit(base, false);
            }
            Expr::Paren(x) => self.visit(x, mv),
            Expr::Borrow { expr, .. } | Expr::Unary { expr, .. } | Expr::Cast { expr, .. } | Expr::Deref(expr) => self.visit(expr, false),
            Expr::Await(x) | Expr::Propagate(x) | Expr::Throw(x) | Expr::Yield(x) => self.visit(x, true),
            Expr::Return(x) => {
                if let Some(x) = x {
                    self.visit(x, true);
                }
            }
            Expr::Tuple(v) | Expr::Array(v) => v.iter().for_each(|x| self.visit(x, true)),
            Expr::ArrayRepeat(a, b) | Expr::Range { lo: a, hi: b, .. } | Expr::Index { expr: a, index: b } => {
                self.visit(a, false);
                self.visit(b, false);
            }
            Expr::Binary { lhs, rhs, .. } => {
                self.visit(lhs, false);
                self.visit(rhs, false);
            }
            Expr::Assign { lhs, rhs, .. } => {
                self.visit(lhs, false);
                self.visit(rhs, true);
            }
            Expr::StructLit { fields, spread, .. } => {
                fields.iter().for_each(|(_, x)| self.visit(x, true));
                if let Some(s) = spread {
                    self.visit(s, false);
                }
            }
            Expr::Call { callee, args } => {
                self.visit(callee, false);
                let user_fn = matches!(callee.as_ref(), Expr::Ident(n) if self.fns.contains(n));
                for a in args {
                    self.visit(&a.value, user_fn);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.visit(receiver, false);
                args.iter().for_each(|a| self.visit(&a.value, true));
            }
            Expr::If(i) => self.if_expr(i, mv),
            Expr::Match(m) => {
                self.visit(&m.scrutinee, false);
                for arm in &m.arms {
                    self.pattern_moves(&m.scrutinee, &arm.patterns);
                    if let Some(g) = &arm.guard {
                        self.visit(g, false);
                    }
                    match &arm.body {
                        MatchArmBody::Expr(x) => self.visit(x, mv),
                        MatchArmBody::Block(b) => self.block(b, mv),
                    }
                }
            }
            Expr::Loop(l) => match l.as_ref() {
                LoopExpr::Loop { body, .. } => self.block(body, false),
                LoopExpr::While { cond, body, .. } | LoopExpr::DoWhile { body, cond } => {
                    self.visit(cond, false);
                    self.block(body, false);
                }
                LoopExpr::For { iter, body, .. } => {
                    self.visit(iter, false);
                    self.block(body, false);
                }
            },
            Expr::Closure(c) => match &c.body {
                ClosureBody::Expr(x) => self.visit(x, false),
                ClosureBody::Block(b) => self.block(b, false),
            },
            Expr::Block(b) | Expr::Unsafe(b) => self.block(b, mv),
            Expr::Spawn(x) => self.visit(x, false),
            Expr::TryCatch { try_block, catch_block, .. } => {
                self.block(try_block, false);
                self.block(catch_block, false);
            }
            Expr::Select(arms) => {
                for a in arms {
                    self.visit(&a.channel_expr, false);
                    self.visit(&a.body, mv);
                }
            }
            _ => {}
        }
    }

    fn if_expr(&mut self, i: &IfExpr, mv: bool) {
        self.visit(&i.cond, false);
        self.block(&i.then_block, mv);
        match &i.else_branch {
            Some(ElseBranch::If(n)) => self.if_expr(n, mv),
            Some(ElseBranch::Block(b)) => self.block(b, mv),
            None => {}
        }
    }

    fn field_move(&mut self, e: &Expr, base: &Expr) {
        let (Some(bty), Some(fty)) = (self.tc.type_of(base), self.tc.type_of(e)) else { return };
        // through a reference the older "out of a borrowed value" rule applies
        let Ty::Named(bn) = bty else { return };
        if !self.tc.has_drop_impl_named(bn) || !self.tc.owns_heap(fty, 0) {
            return;
        }
        let name = if let Expr::Field { name, .. } = e { name.as_str() } else { "" };
        self.err(format!(
            "cannot move the field `{}` (type `{}`) out of `{}`: a type that implements `Drop` cannot be taken apart (Document 6 §9.9, rule 5); borrow the field instead",
            name, fty, bn
        ));
    }

    /// A pattern over an owned `Drop` value that binds an owning field.
    fn pattern_moves(&mut self, scrut: &Expr, pats: &[Pattern]) {
        if matches!(scrut, Expr::Borrow { .. }) {
            return;
        }
        let Some(Ty::Named(n)) = self.tc.type_of(scrut) else { return };
        if !self.tc.has_drop_impl_named(n) {
            return;
        }
        let n = n.clone();
        for p in pats {
            self.pattern(&n, p);
        }
    }

    fn pattern(&mut self, n: &str, p: &Pattern) {
        match p {
            Pattern::Or(alts) => alts.iter().for_each(|a| self.pattern(n, a)),
            Pattern::TupleStruct(name, subs) => {
                let ftys = self.tc.enum_variant_types(n, name).or_else(|| if name == n { self.tc.struct_field_types(n) } else { None });
                let Some(ftys) = ftys else { return };
                for (sub, ft) in subs.iter().zip(ftys.iter()) {
                    if !self.tc.owns_heap(ft, 0) {
                        continue;
                    }
                    let binds = match sub {
                        Pattern::Ident(b) => !b.contains("::") && self.tc.enum_variant_types(&ft.to_string(), b).is_none(),
                        Pattern::Mut(_) => true,
                        Pattern::Wildcard | Pattern::Literal(_) => false,
                        _ => true,
                    };
                    if binds {
                        self.err(format!(
                            "cannot move a field of type `{}` out of `{}` with a pattern: a type that implements `Drop` cannot be taken apart (Document 6 §9.9, rule 5); match on a reference instead",
                            ft, n
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}
