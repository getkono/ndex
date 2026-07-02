# 00 — System Architecture

**Owns:** the system-level view only — the two-binary split and its rationale, the nine-crate workspace dependency graph, the client/engine dependency-boundary rule and how it is enforced, the end-to-end data flow for `index` and `search`, and the v0.1 implemented-vs-stub map at crate granularity.

**Sources:** `Cargo.toml` (workspace `members`), `crates/*/Cargo.toml` (dependency sections), `crates/ndex/src/lib.rs`, `crates/ndex-reconcile/src/process.rs`, `docs/END_TO_END.md`, `README.md`, `PRD.md` §2–§3, §15.

---

## 1. Two-binary split

ndex ships as two binaries that speak length-prefixed MessagePack over stdin/stdout
(spec: [51-framing](50-protocol/51-framing.md)):

| Binary | Role | Crate |
|---|---|---|
| `ndex` | **Thin client.** CLI parsing, host resolution, SSH/subprocess transport, terminal rendering, progress display. Contains *no* extraction, embedding, or index logic. | `crates/ndex` |
| `ndex-remote` | **Fat server.** Reconciler, format extractors, ONNX embedder, tantivy FTS, usearch vectors, SQLite manifest/metadata. Spawned on demand (`ssh host "ndex-remote serve --root …"` or as a local subprocess); no daemon — it exits when the connection closes. Also usable **standalone** with a full local CLI, which is the only working entry point today (see §5). | `crates/ndex-remote` |

**Rationale** (PRD §3, §7.1): the client may sit on a bandwidth-constrained link; the
data and the heavy machinery (extractors, ONNX Runtime, index engines) live on the
archive host. The client never ships engine code to the server and the server never
ships raw content to the client — only wire messages. PRD sets binary-size targets
for both binaries (values owned by [73-release](70-operations/73-release.md); 📋
targets — neither size is measured or checked anywhere; see Divergences).

**Local mode:** with no `HOST:` prefix, `ndex` spawns `ndex-remote` as a local
subprocess over the same protocol — `ndex-remote` must therefore be installed locally
too. Spec: [62-client-transport](60-interfaces/62-client-transport.md).

## 2. Workspace crate graph

Nine crates (`Cargo.toml` `[workspace] members`). An arrow means "depends on".
Every crate depends on `ndex-core`; those edges all terminate at the bottom box.

```
              ┌──────────┐                       ┌──────────────┐
 binaries     │   ndex   │                       │  ndex-remote │
              └──┬────┬──┘                       └─┬────┬────┬──┘
                 │    │    ┌───────────────────────┘    │    └──────────┐
                 │    ▼    ▼                            ▼               ▼
                 │  ┌───────────────┐          ┌─────────────┐  ┌────────────────┐
 wire /          │  │ ndex-protocol │          │ ndex-search │  │ ndex-reconcile │
 composition     │  └──────┬────────┘          └──┬───────┬──┘  └──┬─────┬────┬──┘
                 │         │                      │       │       │     │    │
                 │         │                      ▼       ▼       ▼     ▼    │
                 │         │            ┌────────────┐ ┌────────────┐ ┌──────▼───────┐
 engines         │         │            │ ndex-store │ │ ndex-embed │ │ ndex-extract │
                 │         │            └──────┬─────┘ └─────┬──────┘ └──────┬───────┘
                 │         ▼                   ▼             ▼               ▼
                 │  ┌─────────────────────────────────────────────────────────┐
 foundation      └─►│                        ndex-core                        │
                    └─────────────────────────────────────────────────────────┘
```

Edges omitted from the drawing for readability: `ndex-remote` also depends
*directly* on `ndex-protocol`, `ndex-store`, `ndex-extract`, `ndex-embed`, and
`ndex-core` (it is the composition root and wires everything).

Authoritative edge list (from each `crates/*/Cargo.toml` `[dependencies]` section):

| Crate | Kind | Internal dependencies | Owning spec docs |
|---|---|---|---|
| `ndex-core` | lib | — (foundation) | [10-core/](10-core/11-data-model.md) |
| `ndex-protocol` | lib | core | [50-protocol/](50-protocol/51-framing.md) |
| `ndex-store` | lib | core | [20-store/](20-store/21-layout-and-locking.md) |
| `ndex-extract` | lib | core | [32-extraction](30-ingest/32-extraction.md), [33-chunking](30-ingest/33-chunking.md) |
| `ndex-embed` | lib | core | [34-embedding](30-ingest/34-embedding.md) |
| `ndex-search` | lib | core, store, embed | [41-search](40-search/41-search.md) |
| `ndex-reconcile` | lib | core, store, extract, embed | [31-reconcile](30-ingest/31-reconcile.md) |
| `ndex` | bin | **core, protocol only** | [61-client-cli](60-interfaces/61-client-cli.md), [62-client-transport](60-interfaces/62-client-transport.md) |
| `ndex-remote` | bin | core, protocol, store, extract, embed, search, reconcile (all seven) | [63-remote](60-interfaces/63-remote.md) |

Two structural properties worth naming:

- **Engines are protocol-agnostic.** No engine crate (store/extract/embed/search/
  reconcile) depends on `ndex-protocol`. Domain types live in `ndex-core`;
  `ndex-remote` alone maps them to/from wire payloads (`crates/ndex-remote/src/map.rs`,
  spec: [63-remote](60-interfaces/63-remote.md)).
- **The engine layer is flat.** Engine crates depend only on `ndex-core` and each
  other along the pipeline direction (search→store/embed, reconcile→store/extract/embed);
  there are no cycles and no engine→binary edges.

## 3. The dependency-boundary rule

**Invariant:** the `ndex` client crate depends on exactly two internal crates —
`ndex-core` and `ndex-protocol` — and must never reference an engine crate.

**Verified against code:** `crates/ndex/Cargo.toml` lists `ndex-core.workspace = true`
and `ndex-protocol.workspace = true` and no other internal crate; its external
dependencies are limited to CLI/terminal/serialization/tracing crates. The invariant
is stated in a comment in `crates/ndex/Cargo.toml` ("this crate depends ONLY on
ndex-core and ndex-protocol") and in `crates/ndex/src/lib.rs`.

**How it is enforced:** two layers. Structurally, by Cargo — an undeclared crate is
unresolvable, so client code *cannot* `use` an engine crate without first editing
`crates/ndex/Cargo.toml`. And by an automated regression gate: the `boundary` mise
task ([71-toolchain](70-operations/71-toolchain.md)) fails if
`cargo tree -p ndex -e normal` contains any engine crate
(`ndex-{store,extract,embed,search,reconcile,remote}`); it runs in CI
([72-ci](70-operations/72-ci.md)) and as part of `mise run ci`. A PR that adds
`ndex-store` to the client's dependencies now fails CI.

Consequence of the boundary: the client's transitive closure excludes tantivy,
usearch, rusqlite, ort, and all extractor libraries, which is what makes the
thin-client size target ([73-release](70-operations/73-release.md)) plausible.

## 4. End-to-end data flow

### 4.1 `index`

As designed (full v0.1 path; segments marked ⛔ are not yet wired — see §5):

```
ndex index host:/pool                                             [61-client-cli]
  → resolve host, spawn `ssh host "ndex-remote serve --root /pool"`  [62] ⛔
  → handshake / version negotiation                               [52-handshake] ⛔
  → IndexRequest frame (msgpack, length-prefixed)                 [51, 53] ⛔
  → serve loop dispatch                                           [63-remote] ⛔
  → open Store: flock + identity check                            [21-layout-and-locking]
  → Reconciler, three phases                                      [31-reconcile]
      1. parallel walk (ignore rules, .ndex/ excluded)
      2. metadata diff vs manifest → new/modified/unchanged/deleted
      3. per-file pipeline: extract [32] → chunk [33]
           → embed [34] ⛔ (explicitly skipped in v0.1:
             crates/ndex-reconcile/src/process.rs ignores its `Option<&dyn Embed>`)
           → write FTS [23-fts] + manifest/meta rows [22-manifest]
           → vectors [24-vectors] ⛔
  → Progress frames streamed back ⛔ → client renders bars        [15, 61]
  → IndexComplete summary
```

What actually runs today is the same engine path minus transport: the standalone
`ndex-remote index --root /pool` CLI ([63-remote](60-interfaces/63-remote.md)) calls
the reconciler directly and prints the summary itself.

### 4.2 `search`

```
ndex search host:/pool "query"                                    [61] ⛔ (transport)
  → SearchRequest frame → serve loop                              [51, 53, 63] ⛔
  → open store read-only; opportunistic stale-refresh             [21, 31]
  → mode resolution (auto → fts/semantic/hybrid heuristics)       [41-search]
  → FTS: tantivy BM25 query + snippets                            [23-fts]      ✅
  → semantic: embed query [34] ⛔ → usearch ANN [24] ⛔
  → hybrid: RRF fusion of both rank lists                         [41]
  → SearchResults frame → client renders (pretty/json/paths)      [53, 61]
```

Today: `ndex-remote search --root /pool "query"` exercises the FTS branch end to
end (mode resolution → tantivy → rendered results); the semantic and hybrid branches
are implemented at the `ndex-search` layer but call into ⛔ dependencies
(`ndex-embed`, `ndex-store` vectors).

## 5. v0.1 status map (crate granularity)

`todo!()` counts are from `crates/*/src` at the time of writing (grep-grounded;
counts include `todo!()` bodies inside `#[ignore]`d skeleton tests). Per-item detail
belongs to each crate's owning doc; test-level status to [80-testing](80-testing.md).

| Crate | Status | `todo!()` | Summary |
|---|---|---|---|
| `ndex-core` | ✅ | 0 | All foundation modules implemented. |
| `ndex-protocol` | ✅ | 0 | Framing, codec, handshake, messages implemented and unit-tested — but not yet exercised over a live client↔server session (both endpoints ⛔). |
| `ndex-store` | 🚧 | 10 | Lock, identity, SQLite manifest + meta, tantivy FTS implemented; `vector.rs` (usearch) is entirely ⛔ (all 10 todos). |
| `ndex-extract` | 🚧 | 7 | MIME routing, encoding, language detection, chunking, archive safety, and the text family (plaintext/markdown/csv/json/sql/log) implemented; `formats/{pdf,docx,html,image,code,archive}.rs` are ⛔ (6 todos; the 7th is an ignored skeleton test in `chunk.rs`). |
| `ndex-embed` | ⛔ | 9 | Entirely stub: model management, tokenizer, embedder all `todo!()`. |
| `ndex-search` | 🚧 | 1 | Mode heuristics, query building, FTS execution, fusion code-complete (the 1 todo is an ignored test); FTS path exercised end to end, semantic/hybrid blocked on `ndex-embed` ⛔ + store vectors ⛔. |
| `ndex-reconcile` | ✅ | 0 | Walk, diff, process, recovery, refresh implemented; the embed stage of phase 3 is deliberately skipped pending `ndex-embed` (`process.rs`). |
| `ndex-remote` | 🚧 | 7 | Standalone `init`/`index`/`search`/`info`/`stats`/`completions` and the v0.2 command stubs + `self-update` message work; ⛔: `serve` loop, progress frame emission, `model` subcommand dispatch, and `maintain.rs` (`verify`/`delete`/`config`/`checkpoint`). |
| `ndex` | ⛔ | 19 | Arg parsing and the binary shell compile; commands (9), rendering (5), hosts (1), session (2), and transport (2) bodies are `todo!()`. The thin client is not usable yet. |

Milestone context: PRD §15 scopes v0.1 as the full command set plus SSH remote,
auto-refresh, and auto model fetch. The repo is mid-v0.1: the local standalone
pipeline (init → index → FTS search) is the implemented core; embeddings, exotic
extractors, and the client/transport/serve path are the next increments
(`docs/END_TO_END.md`, "What works vs. deferred").

## Divergences & open questions

- **README's "the dependency graph *enforces* the boundary" is now true in the CI
  sense as well:** the `boundary` task (a `cargo tree -p ndex` assertion) gates CI.
  A cargo-deny `[bans]` entry remains an alternative belt-and-suspenders option
  ([71-toolchain](70-operations/71-toolchain.md)).
- **Binary-size claims are unverified.** The client/server size targets
  ([73-release](70-operations/73-release.md)) appear in
  PRD §3, README.md, and RELEASING.md but are measured nowhere. Moreover today's
  `ndex-remote` is *not* "statically linked incl. ONNX Runtime": `ort` is built with
  `load-dynamic` (dlopen at runtime) — the static build is a planned release-pipeline
  change ([73-release](70-operations/73-release.md)).
- **Stale module comment:** `crates/ndex-remote/src/commands/mod.rs` still says
  handler "bodies are `todo!()`" although most handlers are implemented.
- **PRD §15 v0.1 scope vs code:** `reindex`, `verify`, `delete`, `config`, SSH remote
  with version negotiation, auto-refresh-on-search wiring, auto model fetch, OSC 8
  hyperlinks and progress bars are v0.1 per PRD but currently ⛔/🚧 (tracked above and
  in the leaf docs). README.md and `docs/END_TO_END.md` disclose this; the PRD does not
  mark partial delivery.
- **README's crate-graph sketch** compresses edge direction (e.g. `core ← protocol`
  arrows mixed with `↖` chains); the table in §2 here is the authoritative edge list.
