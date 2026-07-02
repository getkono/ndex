# 71 — Toolchain, Dev Tooling & Workspace Policy

**Owns:** toolchain pinning (`rust-toolchain.toml`), the rustup-vs-mise ownership rule, mise tools and tasks, git hooks (`hk.pkl`), formatter/linter/supply-chain configs (`rustfmt.toml`, `taplo.toml`, `typos.toml`, `deny.toml`), `.gitignore` policy, and all workspace-level `Cargo.toml` settings (resolver, edition, `rust-version`, lints, dependency policy, notable pins, release profile).

**Sources:** `rust-toolchain.toml`, `mise.toml`, `hk.pkl`, `rustfmt.toml`, `taplo.toml`, `typos.toml`, `deny.toml`, `.gitignore`, `Cargo.toml` (workspace root), `CONTRIBUTING.md`.

---

## 1. Toolchain ownership rule

Two managers with a hard split, stated in `rust-toolchain.toml`, `mise.toml`,
`Cargo.toml`, and `CONTRIBUTING.md`:

- **Rust is owned by rustup.** `rust-toolchain.toml` is the single source of truth
  for the compiler. mise intentionally does **not** manage Rust.
- **Everything else is owned by mise.** Formatters, linters, spell checker, hook
  manager, test runner, supply-chain checker come from `mise.toml` `[tools]`.

Bootstrap sequence: `mise trust && mise install && mise run hooks-install`, then
`rustup show` (installs the pinned toolchain on first use).

## 2. `rust-toolchain.toml` ✅

| Key | Value | Note |
|---|---|---|
| `channel` | `"1.96.0"` | Exact pin (not a channel name). Comment: edition 2024 requires ≥ 1.85. |
| `components` | `rustfmt`, `clippy` | |
| `profile` | `minimal` | |

Note the three distinct version numbers in play: **1.96.0** = the pinned build
toolchain; **1.88** = the declared MSRV floor (`rust-version`, §8.1); **1.85** = the
edition-2024 minimum. CI installs 1.96.0 by its own hardcoded copy of the value
([72-ci](72-ci.md)).

## 3. `mise.toml` ✅

### 3.1 Tools

| Tool | Version | Purpose | Backend |
|---|---|---|---|
| `taplo` | latest | TOML formatter/linter | aqua registry shortname (prebuilt) |
| `typos` | latest | source spell checker | aqua |
| `actionlint` | latest | GitHub Actions workflow linter | aqua |
| `pkl` | latest | Apple Pkl CLI — evaluates `hk.pkl` | aqua |
| `hk` | latest | git hook manager | aqua |
| `cargo:cargo-nextest` | latest | test runner | cargo backend (cargo-binstall prebuilt when available) |
| `cargo:cargo-deny` | latest | supply-chain checks | cargo backend |

All tool versions are `latest` — dev tools are deliberately unpinned (contrast with
the exact Rust pin; see Divergences).

`[env]` sets `HK_MISE = "1"` so hk-invoked tools resolve through `mise x` even when
the shell has no mise activation.

### 3.2 Tasks (`mise run <task>`)

| Task | Runs |
|---|---|
| `build` | `cargo build --workspace --all-targets` |
| `test` | `cargo nextest run --workspace` |
| `check` | `cargo check --workspace --all-targets` |
| `clippy` | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| `fmt` | `cargo fmt --all` then `taplo fmt` |
| `fmt-check` | both of the above with `--check` (CI-safe, no writes) |
| `lint` | `typos`, `actionlint`, `cargo deny check` |
| `doc` | `cargo doc --workspace --no-deps` |
| `hooks-install` (alias `hooks`) | `hk install` |
| `ci` | depends on `fmt-check`, `lint`, `clippy`, `test` — the local gate |

Note: the `ci` task does **not** include `build` or `doc`; the GitHub pipeline runs
`build` as an explicit extra step ([72-ci](72-ci.md)).

## 4. Git hooks — `hk.pkl` ✅

Evaluated by the `pkl` CLI; config `amends` the **hk v1.48.0** package (the one
pinned version in the mise/hk stack, pinned via the Pkl package URL). `jobs = 0`
(parallelism auto-detected from CPU count). Install with `hk install`; validate with
`hk validate`.

A shared `linters` mapping — `cargo-fmt` (with `workspace_indicator = "Cargo.toml"`),
`taplo`, `typos`, `actionlint`, each from hk `Builtins` and each wrapped with
`prefix = "mise exec --"` — is splatted into every hook:

| Hook | Mode | Behavior |
|---|---|---|
| `pre-commit` | fix | Auto-formats and re-stages before commit: `fix = true`, `stash = "git"`, `stage = true`, `fail_fast = true`; runs the 4 linters. |
| `pre-push` | check | Read-only linters **plus** strict clippy (`glob **/*.rs`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`) **plus** the full test suite (`cargo nextest run --workspace`, `exclusive = true`). |
| `fix` (manual) | fix | Every fixer, `stash = "none"`. |
| `check` (manual) | check | Read-only; runs only the 4 linters (comment says "run everything" but clippy/test are not included — see Divergences). |

## 5. Formatter & linter configs

### 5.1 `rustfmt.toml` ✅

Stable-channel options only (no nightly-gated keys): `edition = "2024"`,
`newline_style = "Unix"`, `use_field_init_shorthand = true`,
`use_try_shorthand = true`.

### 5.2 `taplo.toml` ✅

`[formatting]`: `align_entries = false`, `column_width = 100`,
`indent_string = "    "` (4 spaces), `reorder_keys = false`.

### 5.3 `typos.toml` ✅

- Excluded from spell-checking: `tests/fixtures/**`, `*.onnx`, `Cargo.lock`.
- Domain vocabulary whitelisted via `extend-words`: `ndex`, `arctic` (the embedding
  model), `restat` (TOCTOU re-`stat()`), `flate` (flate2), `caf` (NFC test-data
  fragment of "café").
- Whitelisted identifiers: `blake3`, `usearch`, `tantivy`.

## 6. Supply chain — `deny.toml` ✅

Run as part of `mise run lint` (local + CI).

| Section | Policy |
|---|---|
| `[advisories]` | version 2; `yanked = "deny"`. |
| `[licenses]` | version 2; `confidence-threshold = 0.9`; allow-list: MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib, MPL-2.0, CC0-1.0. |
| `[bans]` | `multiple-versions = "warn"`, `wildcards = "deny"`. No named-crate bans (in particular, none guarding the client/engine boundary — [00-architecture](../00-architecture.md) §3). |
| `[sources]` | `unknown-registry = "deny"`, `unknown-git = "deny"`. |

## 7. `.gitignore` policy ✅

- Build output: `/target`, `**/*.rs.bk`.
- Index artifacts: `.ndex/`, `.ndex.old/` — never committed, always rebuildable
  ([21-layout-and-locking](../20-store/21-layout-and-locking.md)).
- Model weights: `/models/`, `*.onnx`.
- Per-developer overrides: `hk.local.pkl`, `.mise.local.toml`.
- OS cruft: `.DS_Store`.
- **`Cargo.lock` is committed** (explicit in-file note): the workspace ships
  binaries, so the lockfile is authoritative for reproducible builds.

## 8. Workspace `Cargo.toml`

### 8.1 `[workspace]` / `[workspace.package]` ✅

| Setting | Value | Rationale |
|---|---|---|
| `resolver` | `"3"` | Current feature resolver. |
| `edition` | `2024` | Requires Rust ≥ 1.85. |
| `rust-version` | `1.88` | **Dependency floor, not edition floor:** in-file comment states ort/tantivy/zip/pdf_oxide require ≥ 1.88. Declared but never validated by any build ([72-ci](72-ci.md) has no MSRV job). |
| `version` | `0.1.0` | Single workspace-wide version ([73-release](73-release.md)). |
| `license` | MIT | |
| `authors` | Justin Chung | |
| `repository` | `https://github.com/justy/ndex` | See Divergences — does not match the actual remote. |

### 8.2 Lints ✅

Every crate opts in via `[lints] workspace = true`.

- `[workspace.lints.rust]`: `unsafe_code = "warn"` (CONTRIBUTING.md: no `unsafe`
  without a `// SAFETY:` comment and a per-crate `#![allow(unsafe_code)]`);
  `dead_code = "allow"` and `unused_variables = "allow"` — **temporary skeleton
  allows**, marked `TODO(skeleton)`: they let real field/parameter names coexist
  with `todo!()` bodies under `clippy -D warnings`, and are to be removed once stub
  bodies are filled in (per-crate completion, per CONTRIBUTING.md).
- `[workspace.lints.clippy]`: `all = { level = "warn", priority = -1 }`; warnings are
  promoted to errors by the `-D warnings` flag in the clippy task/hook/CI, not here.

### 8.3 Workspace dependency policy ✅

All dependencies — internal path crates and external — are declared once in
`[workspace.dependencies]` with versions and features; member crates reference them
exclusively via `name.workspace = true` (optionally adding `optional = true`, as
`ndex-extract` does for `pdfium-render`). This gives one version per dependency
across the workspace. Notable entries:

- **`ort = { version = "=2.0.0-rc.12", features = ["load-dynamic"] }`** — the only
  `=`-pinned dependency. `load-dynamic` means ONNX Runtime is `dlopen`'d at runtime
  rather than linked at build time (a release-time switch to static/bundled is
  planned — [73-release](73-release.md)). In-file NOTE: **default features must stay
  on** — disabling them makes `ort-sys` emit a truncated `OrtApi` that fails to
  compile ort's execution-provider modules (observed on rc.12).
- **tree-sitter pinned to the 0.25.x line** with grammars trailing core
  (`tree-sitter-rust` 0.24, `-python`/`-javascript` 0.25). Comment marks the current
  set as a representative v0.1 subset with a `TODO(skeleton)` to add the rest of the
  PRD §4.4 grammar set (ts/tsx, c/cpp, go, java, ruby, …).
- **`pdfium-render`** — optional; gated behind the `pdfium` feature of
  `ndex-extract` (the fallback PDF path; `pdf_oxide` is primary,
  [32-extraction](../30-ingest/32-extraction.md)).
- **`liblzma`** with `features = ["static"]` — maintained successor to `xz2`.
- Engine crates whose builds need a C/C++ toolchain: `rusqlite` (`bundled`), `usearch`
  (C++/cmake), tree-sitter (cc) — this drives CI's apt package list ([72-ci](72-ci.md)).

### 8.4 `[profile.release]` ✅

`lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"`. In-file comment: thin
knobs for now, to be tuned for distribution later ([73-release](73-release.md)).

## Divergences & open questions

- **`repository` URL is wrong / inconsistent:** workspace `Cargo.toml` says
  `github.com/justy/ndex`; the actual remote is `getkono/ndex`; PRD §7.2 refers to
  `github.com/ndex-dev/ndex`. Three different owners for the same project.
- **MSRV asserted, never verified:** `rust-version = "1.88"` (and the claim that
  ort/tantivy/zip/pdf_oxide need ≥ 1.88) is not exercised anywhere — the only
  toolchain ever built with is 1.96.0.
- **Pinning is asymmetric:** Rust is pinned exactly (1.96.0) and ort is `=`-pinned,
  but every mise tool is `latest` — formatting, spell-check, and supply-chain gates
  can shift under contributors and CI without a repo change. Only hk itself is
  pinned (v1.48.0, via the Pkl package URL in `hk.pkl`).
- **hk `check` hook comment says "run everything"** but the step list contains only
  the four linters — clippy and tests run only in `pre-push`.
- **`dead_code`/`unused_variables` allows are workspace-global**, so they also mask
  genuine dead code in the crates that are already complete (e.g. `ndex-core`,
  `ndex-reconcile`); the removal TODO is all-or-nothing at the workspace level even
  though CONTRIBUTING.md describes per-crate removal.
- **`mise run ci` ≠ CI:** the local `ci` task omits `build` (and `doc`), while the
  GitHub pipeline runs `build` explicitly; CONTRIBUTING.md calls the task "the full
  local gate (mirrors GitHub Actions)". See [72-ci](72-ci.md).
