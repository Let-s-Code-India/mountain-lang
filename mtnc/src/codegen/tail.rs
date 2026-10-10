//! Tail-position analysis (Document 10 §6): which call expressions are the last
//! action of their function. A self-recursive call among them is compiled as a
//! jump to the top of the function (both debug and release builds).

use crate::ast::*;
use crate::desugar::{block_children, expr_children, Child};
use std::collections::HashSet;

fn mark_block(b: &Block, unit: bool, out: &mut HashSet<usize>) {
    match &b.tail {
        Some(t) => mark_expr(t, unit, out),
        None => {
            // `f(x);` as the last statement of a unit function is a tail call too.
            if unit {
                if let Some(Stmt::Expr(e)) = b.stmts.last() {
                    mark_expr(e, unit, out);
                }
            }
        }
    }
}

fn mark_expr(e: &Expr, unit: bool, out: &mut HashSet<usize>) {
    match e {
        Expr::Paren(i) => mark_expr(i, unit, out),
        Expr::Call { .. } | Expr::MethodCall { .. } => {
            out.insert(e as *const Expr as usize);
        }
        Expr::Block(b) | Expr::Unsafe(b) => mark_block(b, unit, out),
        Expr::If(i) => mark_if(i, unit, out),
        Expr::Match(m) => {
            for arm in &m.arms {
                match &arm.body {
                    MatchArmBody::Expr(x) => mark_expr(x, unit, out),
                    MatchArmBody::Block(b) => mark_block(b, unit, out),
                }
            }
        }
        Expr::Return(Some(x)) => mark_expr(x, unit, out),
        _ => {}
    }
}

fn mark_if(i: &IfExpr, unit: bool, out: &mut HashSet<usize>) {
    mark_block(&i.then_block, unit, out);
    match &i.else_branch {
        Some(ElseBranch::Block(b)) => mark_block(b, unit, out),
        Some(ElseBranch::If(inner)) => mark_if(inner, unit, out),
        None => {}
    }
}

fn returns_in_block(b: &Block, unit: bool, out: &mut HashSet<usize>) {
    for s in &b.stmts {
        if let Stmt::Return(Some(e)) = s {
            mark_expr(e, unit, out);
        }
    }
    for c in block_children(b) {
        match c {
            Child::E(x) => returns_in_expr(x, unit, out),
            Child::B(bb) => returns_in_block(bb, unit, out),
        }
    }
}

fn returns_in_expr(e: &Expr, unit: bool, out: &mut HashSet<usize>) {
    if let Expr::Return(Some(x)) = e {
        mark_expr(x, unit, out);
    }
    for c in expr_children(e) {
        match c {
            Child::E(x) => returns_in_expr(x, unit, out),
            Child::B(bb) => returns_in_block(bb, unit, out),
        }
    }
}

/// The addresses of all call expressions in tail position of `body`
/// (`unit`: the function returns `()`).
pub fn tail_calls_of(body: &Block, unit: bool) -> HashSet<usize> {
    let mut out = HashSet::new();
    mark_block(body, unit, &mut out);
    returns_in_block(body, unit, &mut out);
    out
}
