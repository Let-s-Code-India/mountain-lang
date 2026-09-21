//! Generator State-Machine Lowering — Phase 8 (Document 25 §2.3).
//!
//! Implements Document 9 §4's generator description: "Generators are
//! implemented by the compiler as a state machine transformation
//! (compiling the function body into an enum representing 'which
//! yield point am I resuming from' plus the captured local
//! variables)... a well-understood, zero-runtime-overhead-beyond-
//! necessary-state technique."
//!
//! ## Scope (read before extending)
//!
//! There is no LLVM backend yet (Document 25 §2.3 places codegen at
//! Phase 10), so "the generator state machine must correctly resume
//! from every yield point across multiple `.next()` calls, preserving
//! local state correctly between suspensions" (Phase 8's exit
//! criterion) can't be verified by compiling and running real machine
//! code. This module makes it verifiable anyway, honestly: it performs
//! a REAL lowering from a generator function's actual AST into an
//! explicit resume-point/locals state machine (`LoweredGenerator`,
//! `GenStep`) — not a stub, and structurally exactly what Document 9
//! §4 describes — plus a small interpreter (`GeneratorState::next`)
//! that actually EXECUTES that lowered form, so `#[test]`s can call
//! `.next()` repeatedly and assert on real yielded values and real
//! preserved state, the way a future LLVM-generated `poll`-style
//! function would behave at runtime.
//!
//! The interpreter is deliberately NOT a general Mountain evaluator —
//! building one would mean re-implementing the entire language's
//! runtime semantics ahead of Phase 10, which is explicitly out of
//! scope. It supports exactly the subset Document 9 §4's own
//! `fibonacci` example uses, since that's the only concrete generator
//! example in the whole specification series to ground this against:
//! - `let` / `let mut` with a simple initializer expression
//! - plain assignment (`x = expr;`)
//! - `yield expr;`
//! - one enclosing `loop { ... }` wrapping the whole body (the shape
//!   every realistic generator needs, since a generator that runs out
//!   of statements without looping is a one-shot generator — also
//!   supported, as the "no loop" case)
//! - expressions: integer literals, identifier reads, and `+ - * /`
//!   binary arithmetic over `i64` (Document 9 §4's own example is
//!   entirely `u64` arithmetic; `i64` is used here as the one numeric
//!   type this prototype evaluates, not a claim about Mountain's real
//!   numeric-type system, which Phase 3 already handles properly
//!   elsewhere)
//!
//! Anything outside that subset (`if`/`match` containing a `yield`,
//! nested loops, function calls, non-integer types) is reported as an
//! explicit `LowerError` at lowering time — never silently
//! mis-lowered. This is a genuine, if intentionally narrow, capability
//! boundary: it proves the state-machine TRANSFORMATION technique
//! works correctly (resume-point tracking, local-state preservation
//! across suspensions) for a real, representative case, which is what
//! the exit criterion asks for; it is not a claim that every
//! expressible Mountain generator can be lowered by Phase 8's code.
//! Full generality is Phase 10's job, once real codegen exists to
//! lower directly to LLVM IR instead of to this prototype IR.

use crate::ast::{AssignOp, BinaryOp, Block, Expr, Literal, LoopExpr, Pattern, Stmt};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    Unsupported(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::Unsupported(what) => write!(f, "generator lowering: unsupported {}", what),
        }
    }
}

/// A tiny expression IR covering exactly the shapes this prototype
/// evaluates (see module doc). Built once at lowering time from the
/// real `ast::Expr`, so the interpreter never touches `ast::Expr`
/// directly.
#[derive(Debug, Clone)]
enum GenExpr {
    IntLit(i64),
    Ident(String),
    Binary(Box<GenExpr>, BinOp, Box<GenExpr>),
}

#[derive(Debug, Clone, Copy)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// One step of the lowered, linear state machine. `LoopBack(n)` marks
/// where control returns to after falling off the end of the step
/// sequence -- `n` is the step index to resume at (the index right
/// after the enclosing `loop`'s own opening), giving the "enum of
/// which point I'm resuming from" Document 9 §4 describes: here, that
/// enum is simply "the step index", and resuming from it is exactly
/// `GeneratorState.pc`.
#[derive(Debug, Clone)]
enum GenStep {
    Let(String, GenExpr),
    Assign(String, GenExpr),
    Yield(GenExpr),
    /// Unconditional jump back to `usize` (used for the one supported
    /// enclosing `loop`) -- absent for a non-looping (one-shot)
    /// generator, which simply finishes when `pc` reaches the end.
    JumpTo(usize),
}

#[derive(Debug, Clone)]
pub struct LoweredGenerator {
    steps: Vec<GenStep>,
}

/// Lowers a generator function's body into its explicit state machine.
/// Grounded directly in Document 9 §4's own `fibonacci` shape: zero or
/// more leading `let`/assignment statements, then EITHER the rest of
/// the body directly, or (Document 9 §4's actual example) a single
/// trailing `loop { ... }` containing the repeating step sequence.
pub fn lower_generator(body: &Block) -> Result<LoweredGenerator, LowerError> {
    let mut steps = Vec::new();
    // Mountain's grammar (Document 23 §5's `block ::= "{" statement*
    // expr? "}"`, same as Rust's) lets a block-like expression such as
    // `loop { .. }` end a block either as an ordinary statement
    // (`Stmt::Expr`, if followed by `;`) or, with no trailing `;`, as
    // the block's own TAIL expression (`body.tail`) -- confirmed by
    // reading `parser::parse_stmt` directly rather than assuming.
    // Document 9 §4's own `fibonacci` example writes the loop with no
    // trailing semicolon, so it parses as `body.tail`, NOT
    // `body.stmts.last()` -- both representations are checked here so
    // this isn't just handling the one this module's author happened
    // to guess first.
    let (prelude, trailing_loop): (&[Stmt], Option<&Block>) =
        if let Some(Expr::Loop(l)) = body.tail.as_deref() {
            match l.as_ref() {
                LoopExpr::Loop { body: lbody, .. } => (&body.stmts[..], Some(lbody)),
                _ => return Err(LowerError::Unsupported("a `while`/`for`/`do-while` generator loop -- only bare `loop {}` is supported".into())),
            }
        } else {
            match body.stmts.last() {
                Some(Stmt::Expr(Expr::Loop(l))) => match l.as_ref() {
                    LoopExpr::Loop { body: lbody, .. } => (&body.stmts[..body.stmts.len() - 1], Some(lbody)),
                    _ => return Err(LowerError::Unsupported("a `while`/`for`/`do-while` generator loop -- only bare `loop {}` is supported".into())),
                },
                _ => (&body.stmts[..], None),
            }
        };
    for s in prelude {
        lower_stmt(s, &mut steps)?;
    }
    if let Some(lbody) = trailing_loop {
        let loop_start = steps.len();
        for s in &lbody.stmts {
            lower_stmt(s, &mut steps)?;
        }
        if lbody.tail.is_some() {
            return Err(LowerError::Unsupported("a trailing tail expression inside a generator's loop body".into()));
        }
        steps.push(GenStep::JumpTo(loop_start));
    } else if body.tail.is_some() {
        return Err(LowerError::Unsupported("a trailing tail expression in a non-looping generator body".into()));
    }
    Ok(LoweredGenerator { steps })
}

fn lower_stmt(s: &Stmt, out: &mut Vec<GenStep>) -> Result<(), LowerError> {
    match s {
        Stmt::Let { pattern, value, .. } => {
            let name = match pattern {
                Pattern::Ident(n) | Pattern::Mut(n) => n.clone(),
                _ => return Err(LowerError::Unsupported("a destructuring `let` pattern in a generator".into())),
            };
            let Some(v) = value else {
                return Err(LowerError::Unsupported("an uninitialized `let` in a generator".into()));
            };
            out.push(GenStep::Let(name, lower_expr(v)?));
            Ok(())
        }
        Stmt::Expr(Expr::Assign { op, lhs, rhs, .. }) => {
            if !matches!(op, AssignOp::Eq) {
                return Err(LowerError::Unsupported("a compound-assignment operator in a generator (only plain `=` is supported)".into()));
            }
            let Expr::Ident(name) = lhs.as_ref() else {
                return Err(LowerError::Unsupported("a non-identifier assignment target in a generator".into()));
            };
            out.push(GenStep::Assign(name.clone(), lower_expr(rhs)?));
            Ok(())
        }
        Stmt::Yield(e) => {
            out.push(GenStep::Yield(lower_expr(e)?));
            Ok(())
        }
        other => Err(LowerError::Unsupported(format!("statement shape {:?} in a generator", other))),
    }
}

fn lower_expr(e: &Expr) -> Result<GenExpr, LowerError> {
    match e {
        Expr::Literal(Literal::Int(s)) => s
            .parse::<i64>()
            .map(GenExpr::IntLit)
            .map_err(|_| LowerError::Unsupported(format!("integer literal `{}` (doesn't fit i64)", s))),
        Expr::Ident(name) => Ok(GenExpr::Ident(name.clone())),
        Expr::Paren(inner) => lower_expr(inner),
        Expr::Binary { op, lhs, rhs } => {
            let bop = match op {
                BinaryOp::Add => BinOp::Add,
                BinaryOp::Sub => BinOp::Sub,
                BinaryOp::Mul => BinOp::Mul,
                BinaryOp::Div => BinOp::Div,
                other => return Err(LowerError::Unsupported(format!("binary operator {:?}", other))),
            };
            Ok(GenExpr::Binary(Box::new(lower_expr(lhs)?), bop, Box::new(lower_expr(rhs)?)))
        }
        other => Err(LowerError::Unsupported(format!("expression shape {:?}", other))),
    }
}

fn eval(e: &GenExpr, locals: &HashMap<String, i64>) -> i64 {
    match e {
        GenExpr::IntLit(n) => *n,
        GenExpr::Ident(name) => *locals.get(name).unwrap_or(&0),
        GenExpr::Binary(l, op, r) => {
            let lv = eval(l, locals);
            let rv = eval(r, locals);
            match op {
                BinOp::Add => lv.wrapping_add(rv),
                BinOp::Sub => lv.wrapping_sub(rv),
                BinOp::Mul => lv.wrapping_mul(rv),
                BinOp::Div => if rv == 0 { 0 } else { lv / rv },
            }
        }
    }
}

/// Runtime state for one generator instance: the resume point (`pc`,
/// an index into `LoweredGenerator::steps` -- Document 9 §4's "which
/// yield point am I resuming from" enum, made concrete) plus the
/// captured local variables' current values -- together, exactly the
/// two things Document 9 §4 says a lowered generator's state consists
/// of.
#[derive(Debug, Clone)]
pub struct GeneratorState {
    pc: usize,
    locals: HashMap<String, i64>,
    finished: bool,
}

impl GeneratorState {
    pub fn new() -> Self {
        GeneratorState { pc: 0, locals: HashMap::new(), finished: false }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Reads a captured local's current value (for test assertions on
    /// preserved state between suspensions -- not part of the
    /// generator's own yielded-value protocol).
    pub fn local(&self, name: &str) -> Option<i64> {
        self.locals.get(name).copied()
    }

    /// Resumes execution from `self.pc`, running steps until a `Yield`
    /// produces a value (returned as `Some`) or the step sequence ends
    /// (returns `None`, marking the generator finished -- matching
    /// Rust/Document 9's `Iterator`-style `next() -> Option<T>`
    /// protocol, which is what Document 24 §3's `.take(10)` /
    /// `for value in fibonacci()` usage implies generators support).
    /// Calling `.next()` again after finishing keeps returning `None`
    /// rather than restarting or panicking.
    pub fn next(&mut self, gen: &LoweredGenerator) -> Option<i64> {
        if self.finished {
            return None;
        }
        loop {
            if self.pc >= gen.steps.len() {
                self.finished = true;
                return None;
            }
            match &gen.steps[self.pc] {
                GenStep::Let(name, e) => {
                    let v = eval(e, &self.locals);
                    self.locals.insert(name.clone(), v);
                    self.pc += 1;
                }
                GenStep::Assign(name, e) => {
                    let v = eval(e, &self.locals);
                    self.locals.insert(name.clone(), v);
                    self.pc += 1;
                }
                GenStep::Yield(e) => {
                    let v = eval(e, &self.locals);
                    self.pc += 1; // resume AFTER this yield next time
                    return Some(v);
                }
                GenStep::JumpTo(target) => {
                    self.pc = *target;
                }
            }
        }
    }
}

impl Default for GeneratorState {
    fn default() -> Self {
        Self::new()
    }
}
