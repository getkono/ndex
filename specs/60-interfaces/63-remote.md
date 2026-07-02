# 63 — `ndex-remote` server binary

**Owns:** the fat server's entire command surface — the `ndex-remote` clap definition, standalone-CLI dispatch, every command handler's observable behavior (exact stdout/stderr text), the serve-loop contract, the wire↔engine type mappings (`map.rs`), and the progress-event bridge (`progress.rs`).

**Sources:**
- `crates/ndex-remote/src/main.rs`
- `crates/ndex-remote/src/lib.rs`
- `crates/ndex-remote/src/cli.rs`
- `crates/ndex-remote/src/serve.rs`
- `crates/ndex-remote/src/map.rs`
- `crates/ndex-remote/src/progress.rs`
- `crates/ndex-remote/src/commands/{mod,completions,indexing,maintain,model,read}.rs`
- `crates/ndex-remote/Cargo.toml`
- Tests: `crates/ndex-remote/tests/{cli,characterization,integration}.rs`

Engine behavior lives elsewhere: store open/create in [21-layout-and-locking.md](../20-store/21-layout-and-locking.md), the reconcile pipeline in [31-reconcile.md](../30-ingest/31-reconcile.md), search execution in [41-search.md](../40-search/41-search.md), snippets in [23-fts.md](../20-store/23-fts.md), model registry in [34-embedding.md](../30-ingest/34-embedding.md). Wire framing/handshake/messages are the 50-series docs. This doc owns only what the binary does at its boundary.

---

## 1. Binary lifecycle ✅

`crates/ndex-remote/src/main.rs` mirrors the client exactly: `Cli::parse()` → `init_tracing(verbose, quiet)` → `run(cli)`; on `Err`, print `error: {err}` to **stderr** and exit `err.exit_code()` ([14-errors.md](../10-core/14-errors.md)). clap usage errors exit 2 (unknown subcommand rejection locked by `serve_and_unknown_subcommands`, `crates/ndex-remote/tests/characterization.rs`).

## 2. Logging 🚧

`ndex_remote::init_tracing` (`crates/ndex-remote/src/lib.rs`) is logic-identical to the client's ([61-client-cli.md §2](61-client-cli.md)): stderr writer, filter from `NDEX_LOG` else `-q`→`error` / none→`warn` / `-v`→`info` / `-vv`→`debug` / `-vvv`+→`trace`. Locked by `init_tracing_does_not_panic`. Logging to stderr (never stdout) matters more here: on a serve session, stdout is the msgpack channel.

**`--log-file <PATH>`** ⛔ — declared in `cli.rs` ("write logs to a file in addition to stderr, PRD §17") and `tracing-appender` is a declared dependency, but `main.rs` never passes it to `init_tracing`; the flag is parsed and dropped.

## 3. Argument surface (`crates/ndex-remote/src/cli.rs`)

Top level: `ndex-remote [OPTIONS] <COMMAND>`, `name = "ndex-remote"`, about = `ndex fat server (extraction, embedding, indexing, search)`, `-V/--version` from workspace version (stdout contains `ndex-remote` — locked by `prints_version`). Definition validated by `cli_definition_is_internally_consistent` (`Cli::command().debug_assert()`).

### 3.1 Global options

| Flag | Value | Default | Status |
|---|---|---|---|
| `-v`, `--verbose` | count, repeatable, global | 0 | ✅ |
| `-q`, `--quiet` | bool, global | false | ✅ |
| `--log-file <PATH>` | path, global | none | ⛔ (§2) |

Unlike the client, there are no `--color`/`--config`/SSH globals: the server has no terminal rendering of its own beyond plain `println!` and no outbound connections.

### 3.2 Command reference

`PATH` below is a local index-root directory (`PathBuf`) — the server never parses `host:` targets. Handler status refers to the modules under `crates/ndex-remote/src/commands/` (dispatch in §3.4).

| Command | Positionals | Args struct | Handler | Status |
|---|---|---|---|---|
| `serve` | — | `ServeArgs` | `serve::serve` | ⛔ |
| `init` | `PATH` | `InitArgs` | `indexing::init` | ✅ |
| `index` | `PATH` | `IndexArgs` | `indexing::index` | ✅ |
| `search` | `PATH QUERY` | `SearchArgs` | `read::search` | 🚧 |
| `info` | `PATH FILE` | `InfoArgs` | `read::info` | 🚧 |
| `stats` | `PATH` | `PathArg` | `read::stats` | 🚧 |
| `verify` | `PATH` | `VerifyArgs` | `maintain::verify` | ⛔ |
| `reindex` | `PATH` | `ReindexArgs` | `indexing::reindex` | 🚧 (deliberate error, §5.3) |
| `delete` | `PATH GLOB` | `DeleteArgs` | `maintain::delete` | ⛔ |
| `config` | `PATH [KEY]` | `ConfigArgs` | `maintain::config` | ⛔ |
| `checkpoint` | `PATH` | `PathArg` | `maintain::checkpoint` | ⛔ |
| `model <SUB>` | (per subcommand) | `ModelCommand` | `model::run` | ⛔ |
| `self-update` | — | `SelfUpdateArgs` | `model::self_update` | ✅ (notice stub) |
| `completions` | `SHELL` | `CompletionsArgs` | `completions::run` | ✅ |
| `tag`, `dedup`, `compact` | — | (unit variants) | `unavailable_v0_2` | ✅ v0.2 stubs (§3.3) |

`--help` listing `serve` and `model` is locked by `help_lists_serve_and_model`; parsing of the v0.2 unit variants and `serve --root /pool` by `v0_2_subcommands_parse_to_stub_variants` and `serve_and_unknown_subcommands`.

### 3.3 v0.2 stubs ✅

`tag`/`dedup`/`compact` → `commands::unavailable_v0_2(name)` (`crates/ndex-remote/src/commands/mod.rs`), a byte-for-byte copy of the client helper: `NdexError::Other("'ndex {command}' is planned for v0.2 and not yet available.")` → stderr + the general-error exit ([14-errors.md](../10-core/14-errors.md)). Locked by `v0_2_commands_are_unavailable_with_exit_1` (`tests/cli.rs`) and `unavailable_v0_2_is_a_clear_error` (`tests/characterization.rs`). Note the message says `ndex tag`, not `ndex-remote tag` (see Divergences).

### 3.4 Dispatch ✅ (`ndex_remote::run`, `crates/ndex-remote/src/lib.rs`)

Pure match; the table in §3.2 lists the exact handler per variant. Handlers are shared with (i.e. intended to be reused by) the serve loop's message dispatch per the `commands/mod.rs` module doc, but no serve-side dispatch exists yet.

### 3.5 Per-command arguments

**`serve`** (`ServeArgs`): `--root <PATH>` (required), `--read-only` (reject writes: index, delete, reindex), `--timeout <S>` (u64, default `0` = no timeout).

**`init`** (`InitArgs`): `--model <MODEL>` default `default` (documented `default` (arctic) | `none`), `--exclude <PAT>` repeatable (gitignore-style), `--no-fts`, `--no-meta`.

**`index`** (`IndexArgs`): `--full`, `--verify`, `--dry-run`, `--jobs <N>` (`Option<u32>`, default num_cpus per help text), `--batch-size <N>` (`Option<u32>`), `--no-vectors`, `--enable-ner` (accepted but ignored in v0.1 — help says so), `--max-file-size <S>` (`Option<String>`, human size), `--only-new`, `--status`.

**`search`** (`SearchArgs`): `-m/--mode` default `auto` (documented `auto | fts | semantic | hybrid`), `-f/--format` default `pretty` (documented `pretty | plain | json | jsonl | paths | csv`), `-n/--limit` u32 default `20`, `--offset` u32 default `0`, `--explain`. No filter flags (`--mime`, `--after`, …) exist on the server CLI — filters are a wire-protocol feature ([53-messages.md](../50-protocol/53-messages.md)).

**`info`** (`InfoArgs`): positional `file: PathBuf` (path within the index); `-f/--format` default `pretty`.

**`stats`** (`PathArg`): no flags — unlike the client's `stats`, there is no `--format`.

**`verify`** (`VerifyArgs`): `--sample <FRAC>` (`Option<f64>`, e.g. `0.01` = 1%), `--path-glob <GLOB>` (`Option<String>`, single — client uses repeatable `--path`), `--fail-fast`.

**`reindex`** (`ReindexArgs`): `--vectors`, `--fts`, `--all` (documented as the default), `--confirm` — four independent bools, no exclusion group.

**`delete`** (`DeleteArgs`): `--dry-run`, `--confirm`.

**`config`** (`ConfigArgs`): optional positional `key` (e.g. `auto_refresh.threshold`).

**`checkpoint`** (`PathArg`): no flags. Intent: `PRAGMA wal_checkpoint(TRUNCATE)` on both databases for safe backup (PRD §18.3).

**`model`** (`ModelCommand`, a nested subcommand):

| Subcommand | Arguments |
|---|---|
| `model list` | — |
| `model fetch [MODEL]` | positional default `arctic`; `--all` (download all models) |
| `model verify [MODEL]` | positional default `arctic` |
| `model delete [MODEL]` | positional default `arctic` |
| `model path [MODEL]` | positional default `arctic` |
| `model import <TARBALL>` | pre-staged tarball for air-gapped servers (PRD §7.4) |

**`self-update`** (`SelfUpdateArgs`): `--version <V>` (`Option<String>`), `--check`.

**`completions`** (`CompletionsArgs`): positional `shell: clap_complete::Shell` (`bash`, `elvish`, `fish`, `powershell`, `zsh`).

## 4. Serve loop ⛔ (`crates/ndex-remote/src/serve.rs`)

`serve(args: ServeArgs)` is `todo!()`. The contract, per its doc comment and PRD §13.11 (framing/handshake specifics owned by [51-framing.md](../50-protocol/51-framing.md) / [52-handshake.md](../50-protocol/52-handshake.md)):

1. Write the magic preamble to stdout immediately on startup, before any handshake.
2. Read the client `Handshake`, negotiate the protocol version, reply.
3. Dispatch each `ClientMessage` frame to a command handler, streaming `Progress` events (§7) and a terminal result or `Error` per request.
4. On stdin EOF, `EPIPE` on stdout, or `SIGHUP`: stop accepting frames, finish the in-flight extraction, flush WAL + tantivy, exit cleanly (crash recovery covers anything unflushed — [31-reconcile.md](../30-ingest/31-reconcile.md)).

`--read-only` rejects write operations; `CancelRequest` finishes the in-flight extraction and replies `IndexComplete { cancelled: true }` (PRD §16.2). `--timeout` (exit after N idle seconds) is parsed but its semantics exist only in the PRD. The end-to-end pin is the `#[ignore]`d `ssh_transport_roundtrip` test (`tests/integration.rs`).

## 5. Command handlers

All handlers print human-oriented text with plain `println!` — no color, no TTY detection, no `--format`-driven machine output except `search --format paths`.

### 5.1 `init` ✅ (`commands/indexing.rs`)

Builds the immutable index identity ([`IndexIdentity`](../10-core/11-data-model.md)) via `build_identity(model)`, then `Store::create(path, identity, Config::default())` ([21-layout-and-locking.md](../20-store/21-layout-and-locking.md)). Values chosen by `build_identity`:

| Field | `--model none` | `--model default` (or explicit name) |
|---|---|---|
| `model_name` | `"none"` | registry `full_name` — `default` aliases to `arctic`, then [`ndex_embed::lookup`](../30-ingest/34-embedding.md) |
| `model_hash` | `""` | registry `onnx_blake3` |
| `dimensions` / `mrl_dimensions` | `0` / `0` | from registry |
| `vector_scalar` | `"f16"` | `"f16"` |
| `hnsw_m` / `hnsw_ef_construction` | `32` / `200` | `32` / `200` |

Plus, always: `schema_version = SCHEMA_VERSION`, `created_by = "ndex-remote {CARGO_PKG_VERSION}"`, `created_at = jiff::Timestamp::now()` string, `hashing.algorithm = "blake3"`, `fts.tokenizer_version = 1`. An unknown model name → `NdexError::Config("unknown embedding model: {model}")` (configuration-error exit — [14-errors.md](../10-core/14-errors.md)).

Output (exact): `Initialized ndex index at {path}/.ndex (model: {model})` — prints the raw `--model` string (`default`, not the resolved `arctic`).

**Ignored flags:** `--exclude`, `--no-fts`, `--no-meta` parse but are never read by the handler — ⛔ (see Divergences).

### 5.2 `index` ✅ (`commands/indexing.rs`)

`Store::open(path)`, then either:

- **`--status`** — prints `{n} files indexed; last reconciled at {ns} ns` or `{n} files indexed; never reconciled` (from `manifest.live_files()` / `last_reconciliation_ns()`) and exits.
- Otherwise builds `ReconcileOptions` from the flags (`jobs`/`batch_size` cast u32→usize; `max_file_size` parsed as [`ByteSize`](../10-core/13-config.md) with **parse failures silently discarded** via `.ok()`), runs `Reconciler::new(&mut store, None).run(&options, &NullSink)` ([31-reconcile.md](../30-ingest/31-reconcile.md)) with no progress reporting, and prints the summary (exact):

```
{new} new, {modified} modified, {deleted} deleted, {unchanged} unchanged, {processed} processed, {failed} failed ({duration_ms} ms)
```

`skipped` and `timed_out` are computed by the engine but not printed. `--enable-ner` is parsed and never read — with **no warning**, contra PRD §13.3. Locked by `init_index_search_roundtrip` (`tests/integration.rs`): fresh index prints `2 new` / `2 processed` / `0 failed`; idempotent re-run prints `2 unchanged`.

### 5.3 `reindex` 🚧 (`commands/indexing.rs`)

Deliberately unimplemented (not `todo!()` — it returns a real error so it fails cleanly): `NdexError::Other` → the general-error exit ([14-errors.md](../10-core/14-errors.md)), message (exact):

```
`reindex` (atomic full rebuild) is planned for a follow-up; recreate with `init` + `index`
```

The PRD §13.6 `.ndex/` → `.ndex.old/` swap-and-rebuild flow is 📋. All four flags are parsed and ignored.

### 5.4 `search` 🚧 (`commands/read.rs`)

`Store::open(path)` → `ndex_search::run(&store, None, query, mode, &SearchFilters::default(), limit, offset)` ([41-search.md](../40-search/41-search.md)). Mode parsing (`parse_mode`): exact strings `fts` / `semantic` / `hybrid`; **anything else — including typos — silently becomes `Auto`**.

Output:

- No hits → `No results.` on **stderr**, exit **0** (locked by the no-match case of `init_index_search_roundtrip`).
- `--format paths` → one line per hit: the path only.
- Any other `--format` value (including `json`, `csv`, …) → the same numbered list, per hit:

  ```
  {rank:>3}. [{score:.3}] {path}
       {snippet}
  ```

  where `rank = offset + i + 1`, `path` is resolved from the manifest (`path_of(file_id)`, lossy display; fallback `file#{file_id}` if unmapped), and the snippet line (5-space indent) appears only when [`fts.snippet`](../20-store/23-fts.md) returns one.

`render_snippet` converts tantivy snippet HTML for the terminal, in this replacement order: `<b>`→`\x1b[1m`, `</b>`→`\x1b[0m`, then unescapes `&quot;` `&#x27;` `&lt;` `&gt;` and finally `&amp;`. Note the bold escapes are emitted unconditionally — even when stdout is piped.

`--explain` is parsed and never read ⛔; the hit-line format has no place for score components.

### 5.5 `info` 🚧 (`commands/read.rs`)

`Store::open` → `manifest.get_by_path(NdexPath::from_os_str(file))`; a miss → `NdexError::Other("not in index: {file}")` (general-error exit — [14-errors.md](../10-core/14-errors.md)). Output (exact, aligned):

```
path:    {path}
file_id: {file_id}
size:    {size} bytes
status:  {status:?}
mime:    {mime|-}
```

PRD §13.5 additionally specifies mtime, blake3, tags, doc/media metadata, chunk count, and index membership, plus directory/archive variants — 📋. `-f/--format` is parsed and **ignored** (`json` prints the same text) ⛔.

### 5.6 `stats` 🚧 (`commands/read.rs`)

Output (exact):

```
index:  {path}/.ndex
model:  {identity.embedding.model_name}
files:  {live_files().len()}
last reconcile: {ns} ns   (or: last reconcile: never)
```

The `files:` line is locked by `init_index_search_roundtrip`. PRD §13.5's fuller stats (per-status counts, per-index sizes, schema version) are 📋.

### 5.7 Maintenance ⛔ (`commands/maintain.rs`)

`verify`, `delete`, `config`, `checkpoint` are all `todo!()`. Documented intent: verify recomputes BLAKE3 against the manifest (PRD §13.5); delete removes matching files from all indices (PRD §13.8); config prints TOML or a single key (PRD §13.10); checkpoint runs `PRAGMA wal_checkpoint(TRUNCATE)` on both databases (PRD §18.3).

### 5.8 Model management (`commands/model.rs`)

- `model::run(command)` ⛔ — `todo!()`; intended dispatch to `ndex_embed::model::{list,fetch,verify,delete,model_path,import}` ([34-embedding.md](../30-ingest/34-embedding.md), PRD §7.4).
- `self_update` ✅ — v0.1 stub that **succeeds** (exit 0, unlike the v0.2 command stubs) printing one line:

  ```
  Self-update is planned for v0.2. Update manually via your package manager or: curl -fsSL https://get.ndex.dev/install.sh | sh
  ```

  Both flags are parsed and ignored. Locked by `self_update_prints_v0_2_notice` (`tests/cli.rs`).

### 5.9 `completions` ✅ (`commands/completions.rs`)

Identical mechanism to the client ([61-client-cli.md §4](61-client-cli.md)): `clap_complete::generate` against this crate's `Cli` to stdout. Locked by `generates_shell_completions` (`tests/cli.rs`, zsh).

## 6. Wire ↔ engine mapping ✅ (`crates/ndex-remote/src/map.rs`)

Keeps `ndex-reconcile` free of wire types; the server translates at the boundary. Type shapes are owned elsewhere ([53-messages.md](../50-protocol/53-messages.md) for `IndexOptions`/`IndexStats`; [31-reconcile.md](../30-ingest/31-reconcile.md) for `ReconcileOptions`/`ReconcileStats`); this crate owns the mapping:

- `to_reconcile_options(&IndexOptions) -> ReconcileOptions` — field-for-field; `jobs`/`batch_size` cast u32→usize; `full`, `verify`, `dry_run`, `no_vectors`, `max_file_size`, `only_new` pass through.
- `to_index_stats(&ReconcileStats) -> IndexStats` — identity mapping of all nine fields (`new`, `modified`, `deleted`, `unchanged`, `processed`, `failed`, `skipped`, `duration_ms`, `timed_out`).

Locked by `index_options_map_to_reconcile_options` and `reconcile_stats_map_to_index_stats` (`tests/characterization.rs`) plus in-module unit tests. Nothing calls these yet — the serve loop that would is ⛔.

## 7. Progress bridging (`crates/ndex-remote/src/progress.rs`)

Bridges core progress types ([15-search-and-progress-types.md](../10-core/15-search-and-progress-types.md)) to the wire `ProgressEvent` ([53-messages.md](../50-protocol/53-messages.md)).

- `phase_name(ProgressKind) -> &'static str` ✅ — the stable wire phase strings (PRD §13.7): `Walk`→`walk`, `Diff`→`diff`, `Extract`→`extract`, `Embed`→`embed`, `Fts`→`fts`, `Meta`→`meta`. Exhaustiveness locked by `phase_names_cover_every_kind`.
- `to_progress_event(&ProgressUpdate) -> ProgressEvent` ✅ — copies `current`/`total`/`message` and maps each child (`label`, `current`, `total`, `message`). Locked by `progress_update_maps_to_event_with_children` (and an in-module unit test).
- `WireProgressSink` ⛔ — the `ProgressSink` impl that should frame `ServerMessage::Progress` back to the client; struct is field-less, `emit` is `todo!()` (intended: shared `Mutex<FrameWriter>` handle, `to_vec_named` encoding).

## 8. Test-pinned end-to-end behavior

`init_index_search_roundtrip` (`crates/ndex-remote/tests/integration.rs`) is the only live end-to-end test and pins the standalone pipeline: init succeeds on a fresh dir; index reports `2 new`/`2 processed`/`0 failed`; FTS search finds a file by content; `--format paths` emits the bare path; a no-match query exits 0; re-index reports `2 unchanged`; stats prints `files:`. Four sibling tests are `#[ignore]`d pending features: all-formats coverage, crash recovery, sidecar repair, SSH roundtrip.

## Divergences & open questions

1. **`--log-file` is dead** — declared in `cli.rs` with a PRD §17 citation and `tracing-appender` in `Cargo.toml`, but `main.rs` never wires it. Either wire it or drop flag + dependency.
2. **`--enable-ner` silent** — PRD §13.3 requires the warning `"NER is not available in v0.1. Flag ignored."`; the handler reads nothing and warns nothing.
3. **`--max-file-size` swallows garbage** — `parse::<ByteSize>().ok()` means `--max-file-size banana` behaves as if the flag were absent, with no diagnostic.
4. **`init` ignores `--exclude`/`--no-fts`/`--no-meta`** — parsed, documented in help, never consumed; an index created with `--no-fts` still gets FTS.
5. **`search --format` mostly cosmetic** — only `paths` changes output; `json`/`jsonl`/`csv`/`plain` silently produce the default list, and ANSI bold from `render_snippet` is emitted even when piped. `--explain` is a no-op.
6. **`parse_mode` typo-forgiving** — `--mode semnatic` silently searches in `auto` mode; a `ValueEnum` or an error would surface the mistake.
7. **`reindex` contradicts PRD §13.6** — no `.ndex/`→`.ndex.old/` atomic rebuild; deliberate general-error failure instead (message in §5.3).
8. **`info`/`stats` far thinner than PRD §13.5** — missing fields listed in §5.5/§5.6; `info -f json` accepted but ignored; remote `stats` has no `--format` flag while the client's does.
9. **Stub message says `ndex`, not `ndex-remote`** — `unavailable_v0_2` was copy-pasted from the client, so `ndex-remote tag` reports `'ndex tag' is planned for v0.2…`.
10. **`verify` flag naming split** — server `--path-glob` (single) vs client `--path` (repeatable) vs PRD `--path <GLOB>`; the wire type takes `Option<Vec<…>>` ([53-messages.md](../50-protocol/53-messages.md)). Three surfaces, three shapes.
11. **`init` echoes the alias** — prints `model: default` rather than the resolved registry name stored in `index.toml`.
12. **`model fetch arctic --all`** — a positional default plus `--all` can both apply; precedence unspecified.
13. **Unused dependencies** — `rustix`, `rayon`, `crossbeam-channel`, `rmp-serde` are declared but unreferenced in `src/` (scaffolding for the serve loop and signal handling).
14. **No results ⇒ stderr** — `No results.` goes to stderr while all other search output goes to stdout; fine for piping but undocumented, and the server CLI has no analog to the client's `--fail-no-results` flag (`NdexError::NoResults` — [14-errors.md](../10-core/14-errors.md)).
