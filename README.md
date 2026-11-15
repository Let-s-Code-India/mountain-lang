# Mountain (`mtnc`)

Compiler for the Mountain programming language. See `PROGRESS.md` (repo
root) for build status against the 25-phase roadmap (Document 25 of the
spec series).

## Status

Phases 1–11 are done and confirmed by CI. **Phase 12 (concurrency) is implemented and
awaiting the manager's confirmation of a real CI log: closures, generators, `spawn`/`join`,
OS threads, `Send`/`Sync` checking, `async`/`await`, and actors** (`mtnc --version` →
`mtnc 1.0.0-dev (Phase 12 — closures, generators, async, concurrency)`). Runnable samples are in
`mtnc/examples/concurrency/`; `PROGRESS.md` has the full record, `mtnc/runtime/mtn_rt.c` is the
embedded C runtime that is linked into programs that use tasks, futures or actors.
Not yet available (scheduled): `Mutex`/`Atomic`/`channel` (Phase 13), modules (14), `wasm32` (15),
the standard library and prelude (16), and the rest of the roadmap (Document 25 §2).

## Repository layout

```
.
├── PROGRESS.md                # phase-by-phase status, updated every phase
├── .github/workflows/ci.yml   # Phase 1 minimal CI (Ubuntu, build+test only)
└── mtnc/                      # the actual Rust cargo project
    ├── Cargo.toml
    ├── src/
    │   ├── lib.rs
    │   ├── main.rs             # CLI: mtnc build / mtnc check
    │   ├── token.rs
    │   ├── lexer.rs
    │   ├── manifest.rs
    │   └── diagnostics.rs
    ├── tests/
    │   └── integration.rs
    └── examples/
        └── hello.mtn           # smoke-test source used by CI
```

## Building locally (once you have Rust installed)

```bash
cd mtnc
cargo build
cargo test
cargo run -- check examples
```
