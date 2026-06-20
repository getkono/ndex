# Releasing ndex (outline — implementation deferred)

> Runtime/release ops are **out of scope for the v0.1 skeleton**. This document is the
> intended shape; `scripts/install.sh` and `.github/workflows/release.yml` are stubs.

## Artifacts

- `ndex` — thin client (~2–3 MB).
- `ndex-remote` — fat server (~80–100 MB statically linked, incl. ONNX Runtime).
- `snowflake-arctic-embed-m-v2.0` ONNX model (~297 MB) — published alongside releases,
  auto-fetched by the server on first use (never shipped to the client).

## Planned pipeline (TODO)

1. Cross-compile both binaries for `{x86_64,aarch64}-{linux,macos}` (cargo-dist style).
2. Bundle `pdfium` native lib into the `ndex-remote` tarball (when the `pdfium` feature is on).
3. Switch `ort` from `load-dynamic` to a static/bundled feature for distribution.
4. Export + INT8-quantize the embedding model with `optimum-cli`; verify dims + MRL truncation.
5. **Sign every artifact** (GPG or sigstore/cosign). The install script verifies the
   signature against a hardcoded public key before executing (see `SECURITY.md`).
6. Publish to GitHub Releases; update `get.ndex.dev/install.sh`.

## Versioning

- `index.toml` schema version bumps ⇒ **no migration**; full rebuild (PRD §5).
- Protocol version bumps are rare (years apart) and gated by handshake negotiation (PRD §12.3).
