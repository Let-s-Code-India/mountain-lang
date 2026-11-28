# Mountain (`mtnc`)

Compiler for the Mountain programming language. See `PROGRESS.md` (repo
root) for build status against the 25-phase roadmap (Document 25 of the
spec series).

## Status

Phases 1–12 are done and confirmed by CI. **Phase 13 (atomics, locks, channels, select) is implemented and
awaiting the manager's confirmation of a real CI log: `Drop`, raw-pointer methods, `dyn Trait<T>`,
`Rc<dyn T>`/`Arc<dyn T>`, `Atomic<T>` with memory orderings, `Spinlock`/`Mutex`/`RwLock`, `channel`,
`select`, `volatile`, and the lock-free stack and queue examples** (`mtnc --version` →
`mtnc 1.0.0-dev (Phase 13 — atomics, locks, channels, select)`). Runnable samples are in
`mtnc/examples/concurrency/` and `mtnc/examples/memory/`; `PROGRESS.md` has the full record,
`mtnc/runtime/mtn_rt.c` is the embedded C runtime that is linked into programs that use tasks, futures,
actors, locks or channels.
Not yet available (scheduled): modules (Phase 14), `wasm32` (15), the standard library and prelude (16),
and the rest of the roadmap (Document 25 §2).

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
