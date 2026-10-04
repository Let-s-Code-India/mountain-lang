//! `mtnc` CLI entry point (Document 17 §8).
//!
//! * `mtnc check [path]` — full front end (lexer, parser, type checker incl.
//!   exhaustiveness, borrow checker); prints every diagnostic; non-zero exit
//!   if any file has an error.
//! * `mtnc build <file.mtn|dir> [-o out] [--release] [--emit-ir]` — check, then
//!   LLVM IR -> native executable. No binary is produced if checking fails.
//! * `mtnc run <file.mtn|dir> [--release]` — build, then execute, forwarding
//!   the program's exit code.
//! * `test`/`bench`/`doc`/`fmt` — not implemented yet (later phases).

use mtnc::codegen::Options;
use mtnc::driver;
use mtnc::manifest::Manifest;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return ExitCode::FAILURE;
    }

    match args[1].as_str() {
        "check" => cmd_check(&args[2..]),
        "build" => cmd_build(&args[2..], false),
        "run" => cmd_build(&args[2..], true),
        "--version" | "-V" => {
            println!("mtnc 0.1.0 (Phase 10 — LLVM IR codegen + native backend)");
            ExitCode::SUCCESS
        }
        "test" | "bench" | "doc" | "fmt" => {
            eprintln!(
                "mtnc {}: not yet implemented — this subcommand depends on \
                 compiler stages introduced in later phases of the Document 25 \
                 roadmap (test/bench: Phase 16+, doc/fmt: Phase 23)",
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
    eprintln!("Usage: mtnc check [path]");
    eprintln!("       mtnc build <file.mtn | dir> [-o <out>] [--release] [--emit-ir]");
    eprintln!("       mtnc run   <file.mtn | dir> [--release]");
    eprintln!("       mtnc --version");
}

fn report(path: &Path, errors: &[String]) {
    for e in errors {
        eprintln!("{}", e);
    }
    eprintln!("  --> {}", path.display());
}

/// `mtnc check`: front end only.
fn cmd_check(args: &[String]) -> ExitCode {
    let root = match args.first() {
        Some(p) => PathBuf::from(p),
        None => env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    if !root.exists() {
        eprintln!("mtnc check: path does not exist: {}", root.display());
        return ExitCode::FAILURE;
    }
    let manifest_path = root.join("mountain.toml");
    if manifest_path.exists() {
        match fs::read_to_string(&manifest_path).map_err(|e| e.to_string()).and_then(|s| Manifest::parse(&s).map_err(|es| es.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; "))) {
            Ok(m) => match m.get_str("package", "name") {
                Some(name) => println!("mtnc check: package '{}'", name),
                None => println!("mtnc check: mountain.toml has no [package].name"),
            },
            Err(e) => {
                eprintln!("mtnc check: errors in mountain.toml: {}", e);
                return ExitCode::FAILURE;
            }
        }
    }
    let files = find_mtn_files(&root);
    if files.is_empty() {
        println!("mtnc check: no .mtn source files found");
        return ExitCode::SUCCESS;
    }
    let mut failed = 0usize;
    for path in &files {
        match fs::read_to_string(path) {
            Ok(src) => {
                if let Err(errors) = driver::check_source(&src) {
                    failed += 1;
                    report(path, &errors);
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("mtnc check: could not read {}: {}", path.display(), e);
            }
        }
    }
    if failed > 0 {
        eprintln!("mtnc check: {} of {} file(s) have errors", failed, files.len());
        ExitCode::FAILURE
    } else {
        println!("mtnc check: checked {} file(s), 0 errors", files.len());
        ExitCode::SUCCESS
    }
}

/// `mtnc build` / `mtnc run`.
fn cmd_build(args: &[String], run: bool) -> ExitCode {
    let name = if run { "run" } else { "build" };
    let mut input: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut opts = Options::default();
    let mut emit_ir = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--release" => opts.release = true,
            "--emit-ir" => emit_ir = true,
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(p) => out = Some(PathBuf::from(p)),
                    None => {
                        eprintln!("mtnc {}: `-o` needs a path", name);
                        return ExitCode::FAILURE;
                    }
                }
            }
            "--" => break,
            other if other.starts_with('-') => {
                eprintln!("mtnc {}: unknown option '{}'", name, other);
                return ExitCode::FAILURE;
            }
            other => input = Some(PathBuf::from(other)),
        }
        i += 1;
    }
    let input = input.unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let source_path = if input.is_dir() { input.join("main.mtn") } else { input.clone() };
    let src = match fs::read_to_string(&source_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("mtnc {}: could not read {}: {}", name, source_path.display(), e);
            return ExitCode::FAILURE;
        }
    };
    if emit_ir {
        return match driver::emit_ir(&src, &opts) {
            Ok(ir) => {
                println!("{}", ir);
                ExitCode::SUCCESS
            }
            Err(errors) => {
                report(&source_path, &errors);
                ExitCode::FAILURE
            }
        };
    }
    let exe = match (&out, run) {
        (Some(o), _) => o.clone(),
        (None, true) => env::temp_dir().join(format!("mtnc_run_{}", std::process::id())),
        (None, false) => PathBuf::from(source_path.file_stem().unwrap_or_default()),
    };
    if let Err(errors) = driver::build_executable(&src, &exe, &opts) {
        report(&source_path, &errors);
        eprintln!("mtnc {}: aborting, no executable produced", name);
        return ExitCode::FAILURE;
    }
    if !run {
        println!("mtnc build: wrote {}", exe.display());
        return ExitCode::SUCCESS;
    }
    let extra: Vec<&String> = match args.iter().position(|a| a == "--") {
        Some(p) => args[p + 1..].iter().collect(),
        None => Vec::new(),
    };
    let status = Command::new(&exe).args(extra).status();
    let _ = fs::remove_file(&exe);
    match status {
        Ok(s) => match s.code() {
            Some(c) => ExitCode::from((c & 0xff) as u8),
            None => {
                eprintln!("mtnc run: the program was terminated by a signal");
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("mtnc run: could not start the program: {}", e);
            ExitCode::FAILURE
        }
    }
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
