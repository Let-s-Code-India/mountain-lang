//! The compilation pipeline (Document 17 §1): lex -> parse -> type check
//! (incl. exhaustiveness) -> borrow check -> [desugar `try`/`catch`] -> LLVM
//! IR -> native executable. Used by the CLI and by the tests.

use crate::ast::Program;
use crate::borrow::BorrowChecker;
use crate::codegen::{Codegen, Options};
use crate::desugar;
use crate::lexer;
use crate::parser::parse_program;
use crate::types::TypeChecker;
use inkwell::context::Context;
use inkwell::targets::{CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine};
use inkwell::OptimizationLevel;
use std::path::Path;
use std::process::Command;

/// A program that passed every front-end check.
pub struct Checked {
    pub program: Program,
    pub tc: TypeChecker,
}

/// `mtnc check`: lexer, parser, type checker (incl. exhaustiveness), borrow
/// checker. Returns every diagnostic found, never stops at the first stage's
/// first error within a stage; later stages run only if earlier ones are clean
/// (their results would be meaningless on a broken tree).
pub fn check_source(src: &str) -> Result<Checked, Vec<String>> {
    let (tokens, lex_errs) = lexer::tokenize(src);
    if !lex_errs.is_empty() {
        return Err(lex_errs.iter().map(|d| d.to_string()).collect());
    }
    let (program, parse_errs) = parse_program(tokens);
    if !parse_errs.is_empty() {
        return Err(parse_errs.iter().map(|e| format!("error: {} --> {}", e.message, e.span)).collect());
    }
    let mut tc = TypeChecker::new();
    tc.check_program(&program);
    let mut errors: Vec<String> = tc.errors.iter().map(|e| e.to_string()).collect();
    let mut bc = BorrowChecker::new();
    bc.check_program(&program);
    errors.extend(bc.errors.iter().map(|e| e.to_string()));
    if errors.is_empty() {
        Ok(Checked { program, tc })
    } else {
        Err(errors)
    }
}

fn machine(release: bool) -> Result<TargetMachine, String> {
    Target::initialize_native(&InitializationConfig::default())?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
    target
        .create_target_machine(
            &triple,
            "generic",
            "",
            if release { OptimizationLevel::Aggressive } else { OptimizationLevel::None },
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| "could not create an LLVM target machine for this host".to_string())
}

fn recheck(program: &Program, what: &str) -> Result<TypeChecker, Vec<String>> {
    let mut tc2 = TypeChecker::new();
    tc2.check_program(program);
    if !tc2.errors.is_empty() {
        let mut e = vec![format!("internal error: the program failed to type-check after {}", what)];
        e.extend(tc2.errors.iter().map(|x| x.to_string()));
        return Err(e);
    }
    Ok(tc2)
}

/// Front end + desugaring + IR generation, then `f` is given the finished
/// (verified) module. Everything LLVM-related lives inside this call.
fn with_module<T>(src: &str, opts: &Options, f: impl FnOnce(&inkwell::module::Module, &TargetMachine) -> Result<T, Vec<String>>) -> Result<T, Vec<String>> {
    let Checked { mut program, tc } = check_source(src)?;
    let mut tc = tc;
    // Lowering passes that rewrite the AST; after each, the program is checked
    // again (the synthesized code is ordinary source, and the typed table is
    // keyed by node address, so it must be rebuilt for the new tree).
    if desugar::inline_default_methods(&mut program) {
        tc = recheck(&program, "inlining trait default methods")?;
    }
    if desugar::contains_try(&program) {
        desugar::desugar_try(&mut program, &tc)?;
        tc = recheck(&program, "desugaring `try`/`catch`")?;
    }
    let tm = machine(opts.release).map_err(|e| vec![e])?;
    let context = Context::create();
    let mut cg = Codegen::new(&context, "mountain", tm.get_target_data(), &tc.expr_types, opts.clone());
    cg.generate(&program)?;
    cg.module.set_triple(&TargetMachine::get_default_triple());
    cg.module.verify().map_err(|e| vec![format!("internal error: LLVM rejected the generated IR: {}", e.to_string())])?;
    f(&cg.module, &tm)
}

/// The textual LLVM IR of `src` (used by tests and `mtnc build --emit-ir`).
pub fn emit_ir(src: &str, opts: &Options) -> Result<String, Vec<String>> {
    with_module(src, opts, |m, _| Ok(m.print_to_string().to_string()))
}

/// Compiles `src` to a native executable at `out` (object file + system `cc`).
/// No file is written unless every stage succeeds.
pub fn build_executable(src: &str, out: &Path, opts: &Options) -> Result<(), Vec<String>> {
    let obj = out.with_extension("o");
    with_module(src, opts, |m, tm| {
        if opts.release {
            m.run_passes("default<O2>", tm, inkwell::passes::PassBuilderOptions::create())
                .map_err(|e| vec![format!("internal error: LLVM optimization failed: {}", e.to_string())])?;
        }
        tm.write_to_file(m, FileType::Object, &obj).map_err(|e| vec![format!("could not write the object file: {}", e.to_string())])
    })?;
    // Linking: system C compiler driver, no LTO (see PROGRESS.md).
    let status = Command::new("cc").arg(&obj).arg("-o").arg(out).status();
    let _ = std::fs::remove_file(&obj);
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(vec![format!("linking failed (`cc` exited with {})", s)]),
        Err(e) => Err(vec![format!("could not run the system linker `cc`: {}", e)]),
    }
}
