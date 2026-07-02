# 73 — Release, Installation & Versioning

**Owns:** the release process as specified by `RELEASING.md`, the installer contract of `scripts/install.sh`, and the versioning policy (workspace, index schema, protocol, model).

**Sources:** `RELEASING.md`, `scripts/install.sh`, `SECURITY.md`, `Cargo.toml` (workspace `version`), `PRD.md` §5, §7, §12.3.

---

## 1. Status: the entire release process is deferred ⛔/📋

Runtime/release ops are explicitly out of scope for the v0.1 skeleton
(`RELEASING.md` header). Concretely:

- `scripts/install.sh` is a **fail-fast stub** ⛔: it prints "not implemented in the
  v0.1 skeleton", points at `RELEASING.md`, and `exit 1`s. Everything below in §4 is
  its *documented contract* (header comments + PRD §7), not behavior.
- `RELEASING.md` refers to `.github/workflows/release.yml` as a stub, but **no such
  file exists** — `ci.yml` is the only workflow ([72-ci](72-ci.md)).
- No artifact has ever been signed or published; no tags exist.

## 2. Release artifacts 📋

Per `RELEASING.md`:

| Artifact | Size claim | Notes |
|---|---|---|
| `ndex` | ~2–3 MB | Thin client ([00-architecture](../00-architecture.md) §1). |
| `ndex-remote` | ~80–100 MB | "Statically linked, incl. ONNX Runtime" — true only after pipeline step 3 below; today ort is `load-dynamic` ([71-toolchain](71-toolchain.md) §8.3). |
| The default embedding model's ONNX artifact (see [embedding](../30-ingest/34-embedding.md)) | registry fact — [34-embedding](../30-ingest/34-embedding.md) §2.2 | Published alongside releases; auto-fetched by the **server** on first use, never shipped to the client (PRD §7.4). |

All size figures are estimates; none is measured or enforced anywhere.

## 3. Planned release pipeline 📋

From `RELEASING.md` (verbatim intent, all TODO):

1. Cross-compile both binaries for `{x86_64,aarch64}-{linux,macos}` (cargo-dist
   style). Note FreeBSD is a supported OS (§4.1) but absent from this target list.
2. Bundle the `pdfium` native library into the `ndex-remote` tarball when the
   `pdfium` feature is enabled ([32-extraction](../30-ingest/32-extraction.md)).
3. Switch `ort` from `load-dynamic` to a static/bundled feature **for distribution
   only** — development builds keep `load-dynamic` ([71-toolchain](71-toolchain.md) §8.3).
4. Export + INT8-quantize the embedding model with `optimum-cli`; verify dimensions
   and MRL truncation ([34-embedding](../30-ingest/34-embedding.md)).
5. **Sign every artifact** (GPG or sigstore/cosign). Signature verification, not
   checksums, is the trust anchor: per SECURITY.md, if the release host is
   compromised, both binary and checksum are attacker-controlled, so the installer
   verifies a signature against a hardcoded public key before executing. The cosign
   public key and verification steps are to be published in SECURITY.md (currently a
   placeholder).
6. Publish to GitHub Releases; update `get.ndex.dev/install.sh`.

`[profile.release]` build knobs are owned by [71-toolchain](71-toolchain.md) §8.4
(comment there: "tune for distribution later").

## 4. Installer contract — `scripts/install.sh` ⛔

Server-side installer for `ndex-remote` only (the client is never installed by this
script, and the client never pushes the binary to the server — PRD §7.1: the server
self-installs over its own network link).

### 4.1 Planned behavior (header comments; PRD §7.2)

1. **Detect platform:** arch via `uname -m` — `x86_64`, `aarch64`; OS — Linux,
   macOS, FreeBSD.
2. **Download** the release tarball from GitHub Releases.
3. **Verify** the GPG/cosign signature against a hardcoded public key — **fail
   closed** (no verification, no install).
4. **Extract** `ndex-remote` to `~/.local/bin` by default, or `/usr/local/bin` with
   `--system`.
5. **Never download the embedding model** — the server auto-fetches it on first use
   (PRD §7.4; decision on interrupted downloads: PRD §16.1 — `.tmp` file, delete on
   failure, restart from scratch, never partially installed).

### 4.2 Planned invocation

```sh
curl -fsSL https://get.ndex.dev/install.sh | sh
curl -fsSL https://get.ndex.dev/install.sh | sh -s -- --version 0.1.0 --arch x86_64
```

Flags: `--version <V>`, `--arch <ARCH>`, `--system`. The script is hosted in this
repository (not an opaque CDN) for auditability (SECURITY.md).

### 4.3 Current behavior ✅ (of the stub)

`set -eu`; unconditionally writes two explanatory lines to stderr and exits 1. Safe
to pipe to `sh` today — it cannot do anything.

### 4.4 Alternative install methods 📋 (PRD §7.2)

Homebrew (`brew install ndex`), `cargo install ndex-remote`, Nix
(`nixpkgs#ndex-remote`), AUR (`ndex-remote-bin`), and a manual tarball with
out-of-band GPG verification. None exist yet.

### 4.5 Self-update

`ndex-remote self-update` is a v0.1 stub **by design** (PRD §7.3): it prints the
"planned for v0.2, update manually" message — and this is one of the few release
touchpoints already implemented ✅ (`crates/ndex-remote/src/commands/model.rs`,
spec: [63-remote](../60-interfaces/63-remote.md)). The v0.2 mechanism (manifest
fetch → download → signature verify → atomic `rename()`) is 📋. The client may warn
about an outdated remote but never initiates the update.

## 5. Versioning policy

Three independently versioned surfaces plus the model:

| Surface | Value today | Policy |
|---|---|---|
| Binaries / crates | `0.1.0` — single `[workspace.package] version` for all nine crates ([71-toolchain](71-toolchain.md) §8.1) | Released as a matched pair; the client detects an older remote via handshake and suggests upgrading (PRD §7.3). |
| Index schema (`index.toml`) | owned by [11-data-model](../10-core/11-data-model.md) / [21-layout-and-locking](../20-store/21-layout-and-locking.md) | **No migrations, ever** (PRD §5): any schema-version bump ⇒ refuse to open ⇒ full rebuild. `RELEASING.md` restates this as release policy. |
| Wire protocol | owned by [52-handshake](../50-protocol/52-handshake.md) | Bumps are rare ("years apart") and gated by handshake version negotiation (PRD §12.3); a protocol bump is *not* implied by a binary release. |
| Embedding model | the default embedding model (see [34-embedding](../30-ingest/34-embedding.md)) | Identity recorded in `index.toml`; changing models ⇒ re-embed via reindex (PRD §16.6). |

## Divergences & open questions

- **`RELEASING.md` cites `.github/workflows/release.yml` as a stub; the file does
  not exist at all.** The only workflow is `ci.yml`.
- **Static-linking contradiction (acknowledged):** the artifact table claims
  `ndex-remote` is statically linked including ONNX Runtime, while the workspace
  builds ort with `load-dynamic`; pipeline step 3 is the reconciliation, but until
  then the size/link claims describe a build configuration that doesn't exist.
- **FreeBSD gap:** installer contract and CONTRIBUTING.md/PRD §8 name FreeBSD as a
  supported OS, but the cross-compile matrix in `RELEASING.md` covers only
  linux/macos — no FreeBSD artifact for the installer to download.
- **Hostname/repo identity is unsettled:** `get.ndex.dev` (RELEASING.md, PRD §7),
  `github.com/ndex-dev/ndex` (PRD §7.2 SECURITY link), `github.com/justy/ndex`
  (Cargo.toml `repository`), and the actual `getkono/ndex` remote all disagree. The
  hardcoded-public-key trust model in §4.1 needs a settled distribution origin
  before it means anything.
- **Model size drift:** RELEASING.md and PRD §3 give slightly different model-size
  estimates (the registry fact is owned by [34-embedding](../30-ingest/34-embedding.md))
  — consistent in spirit, but neither is verified against a published artifact
  (none exists).
- **Versioning policy below 1.0 is unstated:** nothing defines what a 0.x bump means
  (semver-ish? lockstep protocol/schema?), when `version` is bumped, or how tags are
  cut — the "Planned pipeline" starts at cross-compilation and skips version/tag
  mechanics entirely.
