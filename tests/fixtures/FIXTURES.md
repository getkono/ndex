# Test fixtures

Representative files for each v0.1 format plus the edge cases from PRD §18.2. The binary fixtures
(PDF/DOCX/images/archives) are added alongside the extractor implementations; this file is the
checklist. The directory is excluded from spell-checking (see `typos.toml`).

## Present (seed)

- `text/hello.txt` — plaintext
- `text/doc.md` — Markdown (heading + paragraph)
- `edge/zero-byte.dat` — zero-byte file (expect `status=1`, BLAKE3 of empty input)
- `edge/.ndexignore` — ignore-file semantics

## Required (TODO — add with the relevant extractor)

| Fixture | Exercises (PRD) |
|---|---|
| `doc/report.pdf` | PDF text + `doc_meta` from Info dict (§4.4) |
| `doc/scanned.pdf` | image-only PDF → `status=4 [DEFERRED]` (§4.8) |
| `doc/encrypted.pdf` | encrypted PDF → `status=4 [UNSUPPORTED]` (§4.8) |
| `doc/letter.docx` | DOCX headings + `core.xml` metadata (§4.4) |
| `web/page.html` | HTML title/headings (§4.5) |
| `code/sample.rs` | tree-sitter declaration boundaries (§4.5) |
| `img/photo.jpg` | JPEG EXIF (camera, GPS, taken_at) (§4.8) |
| `img/plain.png` | PNG width/height only (§4.8) |
| `arc/bundle.tar.gz` | tar.gz member extraction (§4.6) |
| `arc/traversal.zip` | member path `../../etc/passwd` → skipped (§4.9) |
| `arc/bomb.gz` | compression ratio > 200:1 → skipped (§4.9) |
| `arc/deep.zip` | nesting depth > 3 → skipped (§4.9) |
| `enc/utf16.txt` | BOM-prefixed UTF-16 (§4.8) |
| `enc/<latin1-bytes>` | non-UTF-8 filename (§8) |
| `fs/hardlink-{a,b}` | two paths, one inode (§11.1) |
| `fs/cycle-{a,b}` | symlink cycle A→B→A (§11.4) |
