# Contributing to ndex

## Setup

```sh
mise trust && mise install        # taplo, typos, actionlint, pkl, hk, nextest, deny (NOT rust)
rustup show                       # rustup installs the toolchain pinned in rust-toolchain.toml
mise run hooks-install            # install hk git hooks
```

One-liner: `mise trust && mise install && mise run hooks-install && mise run ci`

## Toolchain ownership

- **Rust** is owned by **rustup** via `rust-toolchain.toml`. mise does **not** manage Rust.
- **Everything else** (formatters, linters, test runner, hooks) is owned by **mise** (`mise.toml`).

## Tasks (run via `mise run <task>`)

| Task | What it does |
|---|---|
| `build` | `cargo build --workspace --all-targets` |
| `test` | `cargo nextest run --workspace` |
| `check` | `cargo check --workspace --all-targets` |
| `clippy` | `cargo clippy … -- -D warnings` |
| `fmt` / `fmt-check` | format (write / verify) Rust + TOML |
| `lint` | typos + actionlint + cargo-deny |
| `doc` | build API docs |
| `ci` | the full local gate (mirrors GitHub Actions) |

## Git hooks (hk)

- **pre-commit** — auto-formats (rustfmt, taplo), spell-checks, lints workflows; re-stages fixes.
- **pre-push** — strict clippy (`-D warnings`) + the full `nextest` suite.

## Conventions

- This is a **Unix** tool (Linux, macOS, FreeBSD). Windows is out of scope (see PRD §8).
- Prefer `tracing` over `log`. All logs go to **stderr**; stdout is reserved for results.
- No `unsafe` without a `// SAFETY:` justification and a per-crate `#![allow(unsafe_code)]`.
- During the skeleton phase, product-logic bodies are `todo!()`; tests for them are stubbed
  with a doc comment stating the intended assertion. Fill them in alongside the implementation,
  and remove the workspace `dead_code` / `unused_variables` allows once a crate is complete.
