#!/usr/bin/env sh
# install.sh — server-side installer for `ndex-remote` (STUB — implementation deferred to release ops).
#
# Intended behavior (see RELEASING.md / SECURITY.md / PRD §7):
#   1. Detect arch (uname -m: x86_64, aarch64) and OS (Linux, macOS, FreeBSD).
#   2. Download the release tarball from GitHub Releases.
#   3. VERIFY the GPG/cosign signature against a hardcoded public key (fail closed).
#   4. Extract `ndex-remote` to ~/.local/bin (or /usr/local/bin with --system).
#   5. Do NOT download the embedding model (auto-fetched on first use).
#
# Usage (planned):
#   curl -fsSL https://get.ndex.dev/install.sh | sh
#   curl -fsSL https://get.ndex.dev/install.sh | sh -s -- --version 0.1.0 --arch x86_64
set -eu

echo "ndex install.sh is not implemented in the v0.1 skeleton." >&2
echo "See RELEASING.md for the planned signed-release installation flow." >&2
exit 1
