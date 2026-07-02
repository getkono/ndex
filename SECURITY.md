# Security policy (outline — verification implementation deferred)

## Reporting

Report vulnerabilities privately to the maintainer. Do not open public issues for security bugs.

## Supply chain (planned — TODO)

`curl | sh` installation is a supply-chain vector. Mitigations the release pipeline will implement:

- The install script is hosted in this repository (not an opaque CDN) for auditability.
- Release artifacts are signed with **GPG or sigstore/cosign**. The install script verifies the
  signature against a hardcoded public key **before** executing — checksum verification alone is
  insufficient if the release host is compromised.
- The cosign public key and verification steps will be published here.

## Notes for operators

- `ndex` does not ship a built-in sensitive-file exclusion list. Use `.ndexignore` to exclude
  `.env`, `*.pem`, `*.key`, `**/credentials.json`, `**/secrets/**`, etc. (PRD §11.1).
- `ndex delete` removes files from FTS, manifest, and metadata immediately, but vector
  embeddings persist as tombstones until `ndex reindex --vectors` (or `ndex compact`, v0.2).
- `.ndex/` must reside on a **local** filesystem — `flock()` cannot guarantee exclusion on NFS.
