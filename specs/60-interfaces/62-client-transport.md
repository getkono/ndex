# 62 — Client transport: targets, hosts, session

**Owns:** `[HOST:]PATH` target parsing, the `hosts.toml` host-alias schema and resolution, the `Transport` abstraction (local subprocess and SSH) including the client-side SSH defaults and remote-discovery probe list, and the protocol `Session` lifecycle on the client side.

**Sources:**
- `crates/ndex/src/hosts.rs`
- `crates/ndex/src/transport.rs`
- `crates/ndex/src/session.rs`
- Tests: unit tests in `crates/ndex/src/hosts.rs`, `crates/ndex/tests/characterization.rs`

This is the most stub-heavy area of the client: target parsing and the constants are real; everything that touches a process or a socket is ⛔ `todo!()` with intent captured in doc comments and PRD §12.5–§12.6, §13.7. Wire framing, the magic preamble, and version negotiation themselves are owned by [51-framing.md](../50-protocol/51-framing.md) and [52-handshake.md](../50-protocol/52-handshake.md); message payloads by [53-messages.md](../50-protocol/53-messages.md).

---

## 1. Target parsing ✅ (`crates/ndex/src/hosts.rs`)

```rust
pub struct Target { pub host: Option<String>, pub path: String }
pub fn parse_target(input: &str) -> Target
```

Grammar (PRD §13.2): a leading `host:` prefix selects a remote **only** when the text before the *first* colon is non-empty and contains no `/`. Otherwise the whole input is a local path. Parsing is infallible.

| Input | host | path |
|---|---|---|
| `nas:/pool/archive` | `nas` | `/pool/archive` |
| `nas.local:/pool` | `nas.local` | `/pool` |
| `nas:` | `nas` | `""` (empty — alias `default_root` applies later, §2) |
| `/pool/archive` | none | `/pool/archive` |
| `rel/path` | none | `rel/path` |
| `/pool:weird` | none | `/pool:weird` (colon after `/` is path content) |
| `:x` | none | `:x` (empty host ⇒ local) |

Locked by `parses_remote_and_local_targets` (unit test in `hosts.rs`) and `parse_target_distinguishes_remote_from_local` (`crates/ndex/tests/characterization.rs` — the `:x` case is only pinned there).

Note: a single-segment relative path containing a colon-free hostname-shaped word plus colon (e.g. `c:file` on any platform) always parses as *remote* host `c`; there is no Windows drive-letter carve-out.

## 2. Host aliases — `hosts.toml` (PRD §13.7)

### 2.1 Schema ✅ (types), loading ⛔

`HostsConfig` deserializes the client host-alias file (serde + TOML):

```toml
[hosts.<alias>]
hostname    = "nas.local"                    # required
user        = "admin"                        # optional
port        = 22                             # optional (u16)
key         = "~/.ssh/nas_ed25519"           # optional
remote_path = "/usr/local/bin/ndex-remote"   # optional
default_root = "/pool/archive"               # optional — enables `ndex search nas: "q"`
```

`hosts` defaults to empty, so a missing/empty file is a valid config. All `HostEntry` fields except `hostname` are `Option`.

`load_hosts()` ⛔ — intended to read `$NDEX_CONFIG_DIR` or `~/.config/ndex/hosts.toml` (source TODO comment); no code path exists yet, and nothing resolves a parsed `Target.host` against a `HostsConfig`.

### 2.2 Resolution precedence 📋

PRD §13.7: CLI flags and env vars override config-file values; per-host `hosts.toml` settings override global `config.toml` settings ([61-client-cli.md §7](61-client-cli.md)). When `Target.path` is empty, the alias's `default_root` supplies the index root. None of this is implemented.

## 3. Transport (`crates/ndex/src/transport.rs`)

```rust
pub type Streams = (Box<dyn Write + Send>, Box<dyn Read + Send>); // remote's stdin, stdout
pub trait Transport { fn spawn(&self) -> Result<Streams>; }
```

Both implementations target the same server entry point: `ndex-remote serve --root <root>` ([63-remote.md §4](63-remote.md)).

### 3.1 `LocalTransport` ⛔

Fields: `remote_path: String`, `root: String`. Intent (doc comment + PRD §3 local mode): `std::process::Command::new(remote_path).args(["serve", "--root", root])` with piped stdin/stdout; return the boxed child streams. Local targets therefore go through the same protocol as remote ones — the client has no direct index access (architecture rule, [00-architecture.md](../00-architecture.md)).

### 3.2 `SshTransport` ⛔

Fields: `host`, `user: Option<String>`, `port: u16`, `key: Option<String>`, `remote_path: Option<String>`, `root: String`, `ssh_options: Vec<String>`. Intent (doc comment + PRD §12.6): build `ssh [DEFAULT_SSH_OPTIONS] [-i key] [-p port] user@host "ndex-remote serve --root <root>"`; map host-key verification failure to a clear error (PRD: SSH exits 255 under `BatchMode=yes` instead of prompting, which would deadlock the stdio channel).

Note `port` is a bare `u16` (not `Option`): the PRD's default of 22 must be applied by whoever constructs the transport, and no constructor exists yet.

### 3.3 SSH defaults ✅ (constant)

```rust
pub const DEFAULT_SSH_OPTIONS: &[&str] = &[
    "-T",
    "-o", "BatchMode=yes",
    "-o", "ServerAliveInterval=30",
    "-o", "ServerAliveCountMax=3",
];
```

Injected unless overridden by the user (PRD §12.5 keepalive, §12.6 host-key deadlock prevention; `-T` suppresses PTY allocation per §12.2). User overrides arrive via `--ssh-option` ([61-client-cli.md §3.1](61-client-cli.md)); the merge/override mechanics are unimplemented. There is no application-level heartbeat in v0.1 (PRD §12.5).

### 3.4 Remote discovery 🚧 (constant only)

```rust
pub const REMOTE_DISCOVERY_PATHS: &[&str] = &[
    "ndex-remote",                    // PATH lookup
    "~/.local/bin/ndex-remote",
    "/usr/local/bin/ndex-remote",
];
```

This covers steps 3–5 of the PRD §12.6 probe order; steps 1–2 (`--remote-path` flag, `NDEX_REMOTE_PATH` env var) are expected to short-circuit before the list is consulted. No probing code consumes the constant yet, and the PRD-specified not-found error (install one-liner + `--remote-path` hint) is 📋.

## 4. Session ⛔ (`crates/ndex/src/session.rs`)

```rust
pub struct Session {
    reader: FrameReader<Box<dyn Read + Send>>,
    writer: FrameWriter<Box<dyn Write + Send>>,
}
```

Wraps a `Transport`'s streams in the protocol framing types (owned by [51-framing.md](../50-protocol/51-framing.md)).

- `Session::connect(transport, handshake) -> Result<(Session, HandshakeResp)>` ⛔ — intent per doc comment: spawn the transport → wrap streams in `Frame{Reader,Writer}` → scan for the magic preamble (tolerated-garbage budget and failure message owned by [51-framing.md](../50-protocol/51-framing.md)) → send `HandshakeReq`, receive/negotiate `HandshakeResp` ([52-handshake.md](../50-protocol/52-handshake.md)).
- `Session::request(&mut self, message, on_progress) -> Result<ServerMessage>` ⛔ — send one `ClientMessage`; forward each interleaved `Progress` frame to the `on_progress` callback; return the terminal response (`SearchResult`, `IndexComplete`, `Error`, …). One request in flight at a time (the signature admits no pipelining).

No tests cover this module; the end-to-end contract is pinned only by the `#[ignore]`d `ssh_transport_roundtrip` in `crates/ndex-remote/tests/integration.rs` (client ↔ ssh-localhost ↔ serve loop).

## Divergences & open questions

1. **`SshTransport.port: u16` with no default** — PRD §13.2 documents default 22, but the field is mandatory and nothing supplies 22. Either the field becomes `Option<u16>` or the (nonexistent) constructor owns the default.
2. **`NDEX_SSH_COMMAND`** (PRD §13.7: override the SSH binary) has no hook: `SshTransport` has no field for the ssh program name.
3. **Discovery constant vs PRD probe order** — `REMOTE_DISCOVERY_PATHS` encodes only the last three steps of PRD §12.6 and its first entry conflates "`which ndex-remote`" with a plain `Command::new("ndex-remote")` PATH lookup; whether probing happens client-side (multiple ssh attempts) or via a single remote shell one-liner is undecided in code.
4. **`HostEntry.key`/`default_root` tilde expansion** — PRD examples use `~/…` paths; no expansion code exists, and `Option<String>` (not `PathBuf`) suggests it is deferred to the shell/ssh, which will not work for `default_root`.
5. **Windows drive letters** — `parse_target("c:/data")` yields host `c` (§1 note). Acceptable for a POSIX-first tool but undocumented in the PRD.
6. **No alias-resolution function** — `parse_target` returns a raw `host` string; nothing distinguishes "alias defined in hosts.toml" from "literal hostname". PRD implies fall-through to literal hostname when no alias matches; unimplemented and unspecified in code.
