# 61 — `ndex` client CLI

**Owns:** the thin client's entire command surface — every subcommand, flag, and default in the `ndex` binary; its dispatch table; its process lifecycle (logging, error printing, exit behavior); and terminal rendering (color palette, OSC 8 links, capability detection, per-format renderers).

**Sources:**
- `crates/ndex/src/main.rs`
- `crates/ndex/src/lib.rs`
- `crates/ndex/src/args.rs`
- `crates/ndex/src/commands.rs`
- `crates/ndex/src/render/mod.rs`
- `crates/ndex/src/render/format.rs`
- `crates/ndex/Cargo.toml`
- Tests: `crates/ndex/tests/cli.rs`, `crates/ndex/tests/characterization.rs`

Target parsing (`[HOST:]PATH`), host aliases, transport, and sessions are owned by [62-client-transport.md](62-client-transport.md). The server's own CLI is owned by [63-remote.md](63-remote.md). Engine behavior invoked by these commands is owned by the 30/40-series docs.

---

## 1. Binary lifecycle ✅

`crates/ndex/src/main.rs`:

1. `Cli::parse()` — clap parses argv; usage errors are reported by clap itself with exit code 2 (locked by `unknown_command_is_a_usage_error` in `crates/ndex/tests/cli.rs`).
2. `ndex::init_tracing(verbose, quiet)` (§2).
3. `ndex::run(cli)` (§4). On `Err`, prints `error: {err}` to **stderr** and exits with `err.exit_code()` — the variant→code mapping is owned by [14-errors.md](../10-core/14-errors.md).

There is no signal handling; Ctrl-C uses the default process behavior (the PRD's interruption exit code — `NdexError::Interrupted`, mapping owned by [14-errors.md](../10-core/14-errors.md) — is produced only if a handler maps to that variant, which nothing does yet — 📋).

## 2. Logging / verbosity ✅

`ndex::init_tracing` (`crates/ndex/src/lib.rs`): `tracing_subscriber::fmt` writing to **stderr**. The filter comes from the `NDEX_LOG` env var if set and valid; otherwise from the flags:

| Flags | Default filter |
|---|---|
| `--quiet` | `error` |
| (none) | `warn` |
| `-v` | `info` |
| `-vv` | `debug` |
| `-vvv`+ | `trace` |

`--quiet` wins only when `NDEX_LOG` is unset. Repeated init is tolerated (`try_init`, result discarded). Locked by `init_tracing_does_not_panic` (`crates/ndex/tests/characterization.rs`).

## 3. Argument surface (`crates/ndex/src/args.rs`)

Top level: `ndex [OPTIONS] <COMMAND>`, `name = "ndex"`, about = `deep file indexer for archival storage`, `-V/--version` from the workspace package version. `--version` output contains `ndex` and `--help` lists `search`, `index`, `completions` (locked by `prints_version`, `help_lists_core_commands` in `crates/ndex/tests/cli.rs`). The clap definition itself is validated by `cli_definition_is_internally_consistent` (`Cli::command().debug_assert()`).

### 3.1 Global options (`GlobalOpts`) — parsed ✅, consumed 🚧

All flags are `global = true` (accepted before or after the subcommand).

| Flag | Value | Default | Consumed by |
|---|---|---|---|
| `-v`, `--verbose` | count (repeatable) | 0 | ✅ `init_tracing` |
| `-q`, `--quiet` | bool | false | ✅ `init_tracing` |
| `--color <WHEN>` | free string (`auto \| always \| never` documented, **not validated**) | `auto` | ⛔ nothing reads it yet |
| `--no-hyperlinks` | bool | false | ⛔ nothing reads it yet |
| `--config <PATH>` | path | none | ⛔ nothing reads it yet |
| `--ssh-key <PATH>` | path | none | ⛔ transport pending |
| `--ssh-port <PORT>` | u16 | none (PRD default 22 applied nowhere yet) | ⛔ transport pending |
| `--ssh-user <USER>` | string | none (PRD default `$USER`) | ⛔ transport pending |
| `--ssh-option <OPT>` | string, repeatable | `[]` | ⛔ transport pending |
| `--remote-path <PATH>` | string | none | ⛔ transport pending |

PRD §13.2 scopes the SSH flags to `ndex search`; the code makes them global so every remote-capable command shares them. The SSH defaults these flags override are owned by [62-client-transport.md](62-client-transport.md).

### 3.2 Command reference

`TARGET` below is a `[HOST:]PATH` string parsed by `hosts::parse_target` ([62-client-transport.md](62-client-transport.md)). Handler status refers to `crates/ndex/src/commands.rs`.

| Command | Positionals | Args struct | Handler status |
|---|---|---|---|
| `search` | `TARGET QUERY` | `SearchArgs` | ⛔ `todo!()` |
| `index` | `TARGET` | `IndexArgs` | ⛔ `todo!()` |
| `init` | `PATH` (local `PathBuf`, remote init is v0.2 — PRD §13.4) | `InitArgs` | ⛔ `todo!()` |
| `info` | `TARGET FILE` | `InfoArgs` | ⛔ `todo!()` |
| `stats` | `TARGET` | `TargetArg` | ⛔ `todo!()` |
| `verify` | `TARGET` | `VerifyArgs` | ⛔ `todo!()` |
| `reindex` | `TARGET` | `ReindexArgs` | ⛔ `todo!()` |
| `delete` | `TARGET GLOB` | `DeleteArgs` | ⛔ `todo!()` |
| `config` | `TARGET [KEY]` | `ConfigArgs` | ⛔ `todo!()` |
| `completions` | `SHELL` | `CompletionsArgs` | ✅ |
| `tag`, `dedup`, `compact` | — | (unit variants) | ✅ v0.2 stub error (§3.3) |

Parsing of `search` and the v0.2 unit variants is locked by `search_and_v0_2_subcommands_parse` (`crates/ndex/tests/characterization.rs`).

**`search`** (`SearchArgs`) — positional `target`, `query`, then:

| Flag | Value | Default |
|---|---|---|
| `-m`, `--mode` | free string (`auto \| fts \| semantic \| hybrid` documented, not validated) | `auto` |
| `--mime <GLOB>` | string | none |
| `--after` / `--before` | string (ISO 8601 or relative like `2w`) | none |
| `--larger` / `--smaller` | string (e.g. `10MB`) | none |
| `--path <GLOB>` | string, **repeatable** (`Vec`) | `[]` |
| `--tag <TAG>` | string, repeatable (OR semantics) | `[]` |
| `--lang <CODE>` | string (ISO 639-1) | none |
| `-n`, `--limit` | u32 | `20` |
| `--offset` | u32 | `0` |
| `-f`, `--format` | free string (`pretty \| plain \| json \| jsonl \| paths \| csv` documented, not validated) | `pretty` |
| `--explain` | bool | false |
| `--no-refresh` / `--refresh` | bool each (not mutually exclusive in clap) | false |
| `--fail-no-results` | bool — fail on empty result via `NdexError::NoResults` (exit code owned by [14-errors.md](../10-core/14-errors.md)) | false |

PRD §13.2 additionally specifies `-c/--context <N>`, `--no-context`, `--no-score`, `--count`, and `--refresh-timeout <S>`; none exist in code — 📋 (see Divergences).

**`index`** (`IndexArgs`): `--full`, `--verify`, `--dry-run`, `--jobs <N>` (`Option<u32>`), `--no-vectors`, `--only-new` — all plain bools except `--jobs`. PRD §13.3's `--batch-size`, `--enable-ner`, `--max-file-size`, `--status` exist only on the server CLI ([63-remote.md](63-remote.md)) — 📋 on the client.

**`init`** (`InitArgs`): positional `path: PathBuf`; `--model <MODEL>` (default `default`), `--exclude <PAT>` (repeatable), `--no-fts`, `--no-meta`. Model semantics are owned by the server handler that actually implements init ([63-remote.md §5.1](63-remote.md)).

**`info`** (`InfoArgs`): positional `target`, `file`; `-f/--format` default `pretty` (documented values `pretty | json`).

**`stats`** (`TargetArg`): positional `target`; `-f/--format` default `pretty` (documented values `pretty | json`).

**`verify`** (`VerifyArgs`): `--sample <FRAC>` (`Option<f64>`), `--path <GLOB>` (repeatable `Vec` — PRD shows a single glob), `--fail-fast`. No `--format` flag despite PRD §13.5 listing one — 📋.

**`reindex`** (`ReindexArgs`): `--vectors`, `--fts`, `--all`, `--confirm` — four independent bools, no mutual-exclusion group.

**`delete`** (`DeleteArgs`): positional `target`, `glob`; `--dry-run`, `--confirm`.

**`config`** (`ConfigArgs`): positional `target`, optional positional `key`. PRD §13.10 specifies a `get <KEY>` verb; code takes the key directly (`ndex config /pool auto_refresh.threshold`) — see Divergences.

**`completions`** (`CompletionsArgs`): positional `shell: clap_complete::Shell` (validated enum: `bash`, `elvish`, `fish`, `powershell`, `zsh`).

### 3.3 v0.2 stubs ✅

`tag`, `dedup`, `compact` dispatch to `commands::unavailable_v0_2(name)`, which returns `NdexError::Other` with message:

```
'ndex {command}' is planned for v0.2 and not yet available.
```

Through §1 this prints on stderr prefixed `error: ` and exits with the general-error code (`NdexError::Other` — [14-errors.md](../10-core/14-errors.md)). Locked by `v0_2_commands_are_unavailable_with_exit_1` (`crates/ndex/tests/cli.rs`) and `unavailable_v0_2_is_a_clear_error` (`crates/ndex/tests/characterization.rs`). PRD §13.9 wants a compact-specific message pointing at `ndex reindex --vectors`; the code uses the generic message — see Divergences.

## 4. Dispatch (`ndex::run`, `crates/ndex/src/lib.rs`) ✅

Pure match, one handler per variant, all in `crates/ndex/src/commands.rs`:

| Variant | Handler | PRD |
|---|---|---|
| `Search` | `commands::search(a, &global)` | §13.2 |
| `Index` | `commands::index(a, &global)` | §13.3 |
| `Init` | `commands::init(a, &global)` | §13.4 |
| `Info` | `commands::info(a, &global)` | §13.5 |
| `Stats` | `commands::stats(a, &global)` | §13.5 |
| `Verify` | `commands::verify(a, &global)` | §13.5 |
| `Reindex` | `commands::reindex(a, &global)` | §13.6 |
| `Delete` | `commands::delete(a, &global)` | §13.8 |
| `Config` | `commands::config(a, &global)` | §13.10 |
| `Completions` | `commands::completions(a)` (no globals) | §13.7 |
| `Tag` / `Dedup` / `Compact` | `commands::unavailable_v0_2("tag"/"dedup"/"compact")` | §13.1 |

Every connecting handler's documented shape (module doc of `commands.rs`): resolve target → build `Transport` → open `Session` → send request → render response. All bodies are `todo!()` ⛔ except `completions` ✅, which generates completions for the `Cli` definition to stdout via `clap_complete::generate` (locked by `generates_shell_completions`, `crates/ndex/tests/cli.rs`).

## 5. Rendering (`crates/ndex/src/render/`)

### 5.1 Color palette ✅

`render::color` (`crates/ndex/src/render/mod.rs`) — semantic ANSI constants, matching the PRD §13.7 table exactly:

| Constant | Escape | Meaning |
|---|---|---|
| `PATH` | `\x1b[1m` | bold |
| `MATCH` | `\x1b[1;33m` | bold yellow |
| `SCORE` | `\x1b[2m` | dim |
| `MIME` | `\x1b[36m` | cyan |
| `SIZE` | `\x1b[32m` | green |
| `DATE` | `\x1b[34m` | blue |
| `ERROR` | `\x1b[31m` | red |
| `TAG` | `\x1b[35m` | magenta |
| `RESET` | `\x1b[0m` | reset |

Locked by `color_scheme_is_ansi` (`crates/ndex/tests/characterization.rs`). Nothing consumes these constants yet (the renderers that would are stubs).

### 5.2 OSC 8 hyperlinks ✅

`render::osc8(uri, text)` → `\x1b]8;;{uri}\x1b\\{text}\x1b]8;;\x1b\\`. Locked byte-for-byte by `osc8_wraps_uri_and_text` (in both `render/mod.rs` unit tests and `crates/ndex/tests/characterization.rs`). The PRD §13.7 `file://host/path` scheme for remote results and `NDEX_HYPERLINKS` env handling are 📋.

### 5.3 Terminal capability detection ✅

`render::detect_caps()` builds the [`TerminalCaps`](../50-protocol/52-handshake.md) advertised in the handshake:

- `width`/`height` from `terminal_size`, falling back to **80×24** when there is no TTY;
- `color` from `supports-color` on **stdout** (this crate honors `NO_COLOR`/`FORCE_COLOR` conventions; `--color` and `NDEX_COLOR` are not consulted — 📋);
- `hyperlinks` from `supports-hyperlinks` on stdout;
- `unicode` hardcoded `true`.

Locked by `detect_caps_returns_sane_defaults` (`crates/ndex/tests/characterization.rs`).

### 5.4 Format dispatch 🚧

`render::render_search(result, fmt, caps)` matches on [`OutputFormat`](../50-protocol/53-messages.md) and calls one renderer per variant in `crates/ndex/src/render/format.rs`. Nothing yet converts the CLI's `--format` **string** to `OutputFormat` — that mapping belongs to the `todo!()` command handlers (⛔). Per the doc comment on `render_search`, the *caller* is responsible for the PRD §13.7 piped-output rule: when stdout is not a TTY, downgrade `Pretty` → `Plain` and disable color/hyperlinks (📋, no caller exists).

| Format | Renderer | Status | Contract |
|---|---|---|---|
| `pretty` | `format::pretty(result, caps)` | ⛔ | ranked, colorized, OSC 8 links, snippet highlights; layout example in PRD §14 |
| `plain` | `format::plain(result)` | ⛔ | TTY-off default; no color/hyperlinks/progress |
| `json` | `format::json(result)` | ⛔ | single JSON object incl. `root` and raw scores (contract pinned by ignored test `json_renderer_emits_a_json_object`) |
| `jsonl` | `format::jsonl(result)` | ⛔ | one JSON object per hit |
| `paths` | `format::paths(result)` | ✅ | raw path bytes + `\n` per hit, written via locked stdout (`hit.path.as_bytes()` — bytes preserved per [12-paths.md](../10-core/12-paths.md)); for `xargs` piping |
| `csv` | `format::csv(result)` | ⛔ | no documented column set yet |

`paths` over zero or more hits returning `Ok` is locked by `paths_renderer_emits_each_path` (`crates/ndex/tests/characterization.rs`).

### 5.5 Progress rendering 📋

PRD §13.7 specifies `indicatif` multi-bars when interactive, periodic line updates when piped, driven by wire `ProgressEvent`s ([53-messages.md](../50-protocol/53-messages.md)). `indicatif` is a declared dependency in `crates/ndex/Cargo.toml`, but no progress-rendering code exists.

## 6. Environment variables

| Variable | Status | Where |
|---|---|---|
| `NDEX_LOG` | ✅ | `init_tracing` (§2) |
| `NO_COLOR` / `FORCE_COLOR` | ✅ (indirect) | via `supports-color` in `detect_caps` (§5.3) |
| `NDEX_COLOR`, `NDEX_HYPERLINKS` | 📋 | PRD §13.7; nothing reads them |
| `NDEX_SSH_COMMAND`, `NDEX_REMOTE_PATH`, `NDEX_CONFIG_DIR` | 📋 | transport/hosts concerns — [62-client-transport.md](62-client-transport.md) |

## 7. Client config files 📋

PRD §13.7 specifies `~/.config/ndex/config.toml` (`[display]` color/hyperlinks/format defaults, `[ssh]` default key/user) with precedence *flags > env > hosts.toml > config.toml*. No code reads it; the `--config` override flag parses but is unconsumed. (`hosts.toml` is owned by [62-client-transport.md](62-client-transport.md).) The `toml`, `serde_json`, `rmp-serde`, `anstream`, and `anstyle` dependencies in `crates/ndex/Cargo.toml` are declared but currently unused — scaffolding for this and §5.

## Divergences & open questions

1. **Missing `search` flags vs PRD §13.2** — `-c/--context`, `--no-context`, `--no-score`, `--count`, `--refresh-timeout` are specified in the PRD but absent from `SearchArgs`. 📋 or PRD trim needed.
2. **Missing `index` flags vs PRD §13.3** — client `IndexArgs` lacks `--batch-size`, `--enable-ner`, `--max-file-size`, `--status`, all of which the server CLI has. Unclear whether the omission is intentional scope-cutting or drift.
3. **`config` grammar** — PRD §13.10 says `ndex config <TARGET> get <KEY>`; code has no `get` verb (`key` is a bare optional positional).
4. **`compact` stub message** — PRD §13.9 mandates a specific message recommending `ndex reindex --vectors`; code emits the generic v0.2 stub message (§3.3).
5. **`verify` lacks `--format`** and takes repeatable `--path` where PRD §13.5 shows a single glob and a `-f/--format` flag.
6. **String-typed enums** — `--color`, `--mode`, `--format` accept any string at parse time (`ndex search /p q --format bogus` parses). Validation is deferred to the `todo!()` handlers; clap `ValueEnum` would reject at exit code 2 instead. Undecided which layer owns validation.
7. **`--refresh` / `--no-refresh` and `--vectors`/`--fts`/`--all`** are not declared mutually exclusive in clap; conflicting combinations parse successfully. Resolution semantics are unspecified.
8. **`--ssh-port` default** — PRD says default 22; the clap arg has no default and no code applies one (see also [62-client-transport.md](62-client-transport.md) Divergences).
9. **PRD §13.1 stub wording** — PRD says stubs "print `Error: 'ndex <cmd>' …`"; the actual prefix is `error: ` (lowercase, from `main.rs`) and the channel is stderr with the general-error exit ([14-errors.md](../10-core/14-errors.md), test-locked).
10. **Duplicated helpers across binaries** — `init_tracing` and `unavailable_v0_2` are copy-pasted in `ndex-remote` ([63-remote.md](63-remote.md)); neither lives in a shared crate, so the two surfaces can drift.
