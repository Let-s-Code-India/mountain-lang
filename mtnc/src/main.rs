//! `mtnc` CLI entry point.
//!
//! Phase 1 scope only implements `build` and `check` (Document 25 §2.3),
//! and both currently only run the pipeline through the Lexer stage
//! (Parser/Semantic Analysis/Codegen don't exist yet — later phases).
//! The full subcommand surface from Document 17 §8 (`run`, `test`,
//! `bench`, `doc`, `fmt`, etc.) is intentionally not implemented yet;
//! invoking them prints a clear "not yet implemented in Phase N" message
//! rather than silently doing nothing or pretending to succeed.

use mtnc::diagnostics::Diagnostic;
use mtnc::lexer;
use mtnc::manifest::Manifest;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return ExitCode::FAILURE;
    }

    match args[1].as_str() {
        "build" => run_lex_pipeline("build", args.get(2)),
        "check" => run_lex_pipeline("check", args.get(2)),
        "--version" | "-V" => {
            println!("mtnc 0.1.0 (Phase 1 — scaffold, manifest, lexer)");
            ExitCode::SUCCESS
        }
        "run" | "test" | "bench" | "doc" | "fmt" => {
            eprintln!(
                "mtnc {}: not yet implemented — this subcommand depends on \
                 compiler stages introduced in later phases of the Document 25 \
                 roadmap (Parser: Phase 2, Codegen: Phase 10, etc.)",
                args[1]
            );
            ExitCode::FAILURE
        }
        other => {
            eprintln!("mtnc: unrecognized subcommand '{}'", other);
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!("Usage: mtnc <build|check> [path]");
    eprintln!("       mtnc --version");
    eprintln!();
    eprintln!("Phase 1: 'build' and 'check' currently run source through the Lexer");
    eprintln!("stage only (tokenize + report diagnostics). Parsing/codegen land in");
    eprintln!("later phases per Document 25's roadmap.");
}

/// Locates `mountain.toml` (if present), discovers `.mtn` source files,
/// and lexes each one, reporting diagnostics. This is the full extent of
/// what "build"/"check" can do in Phase 1, since there is no parser yet.
fn run_lex_pipeline(subcommand: &str, path_arg: Option<&String>) -> ExitCode {
    let root = match path_arg {
        Some(p) => PathBuf::from(p),
        None => env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    if !root.exists() {
        eprintln!("mtnc {}: path does not exist: {}", subcommand, root.display());
        return ExitCode::FAILURE;
    }

    let manifest_path = root.join("mountain.toml");
    if manifest_path.exists() {
        match fs::read_to_string(&manifest_path) {
            Ok(src) => match Manifest::parse(&src) {
                Ok(m) => {
                    if let Some(name) = m.get_str("package", "name") {
                        println!("mtnc {}: package '{}'", subcommand, name);
                    } else {
                        println!("mtnc {}: mountain.toml has no [package].name", subcommand);
                    }
                }
                Err(errors) => {
                    eprintln!("mtnc {}: errors in mountain.toml:", subcommand);
                    for e in &errors {
                        eprintln!("  {}", e);
                    }
                    return ExitCode::FAILURE;
                }
            },
            Err(e) => {
                eprintln!("mtnc {}: could not read mountain.toml: {}", subcommand, e);
                return ExitCode::FAILURE;
            }
        }
    } else {
        println!("mtnc {}: no mountain.toml found in {}, lexing loose .mtn files", subcommand, root.display());
    }

    let mtn_files = find_mtn_files(&root);
    if mtn_files.is_empty() {
        println!("mtnc {}: no .mtn source files found", subcommand);
        return ExitCode::SUCCESS;
    }

    let mut had_errors = false;
    let mut total_tokens = 0usize;

    for path in &mtn_files {
        let src = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("mtnc {}: could not read {}: {}", subcommand, path.display(), e);
                had_errors = true;
                continue;
            }
        };
        let (tokens, diagnostics) = lexer::tokenize(&src);
        total_tokens += tokens.len();
        if !diagnostics.is_empty() {
            had_errors = true;
            for d in &diagnostics {
                report(path, d);
            }
        }
    }

    if had_errors {
        eprintln!("mtnc {}: lexical errors found, aborting", subcommand);
        ExitCode::FAILURE
    } else {
        println!(
            "mtnc {}: lexed {} file(s), {} token(s), 0 errors",
            subcommand,
            mtn_files.len(),
            total_tokens
        );
        ExitCode::SUCCESS
    }
}

fn report(path: &Path, d: &Diagnostic) {
    // Minimal form of Document 22's canonical format; the full
    // note:/help: teaching-diagnostic system is Phase 23 scope.
    eprintln!("error: {}", d.message);
    eprintln!("  --> {}:{}", path.display(), d.span);
}

fn find_mtn_files(root: &Path) -> Vec<PathBuf> {
    if root.is_file() {
        return if root.extension().and_then(|e| e.to_str()) == Some("mtn") {
            vec![root.to_path_buf()]
        } else {
            Vec::new()
        };
    }
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // don't descend into target/ or .git/ — no build artifacts
                // or VCS internals are ever .mtn sources
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name == "target" || name == ".git" {
                        continue;
                    }
                }
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("mtn") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
