# Mountain (`mtnc`)

Compiler for the Mountain programming language. See `PROGRESS.md` (repo
root) for build status against the 25-phase roadmap (Document 25 of the
spec series).

## Repository layout

```
.
├── PROGRESS.md                # phase-by-phase status, updated every phase
├── .github/workflows/ci.yml   # Phase 1 minimal CI (Ubuntu, build+test only)
└── mtnc/                      # the actual Rust cargo project
    ├── Cargo.toml
    ├── PROGRESS.md
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
