# Paths

**Owns:** `NdexPath`, the raw-bytes filesystem path type: construction, hashing, display/JSON rendering, and its custom serde impls.

**Sources:** `crates/ndex-core/src/path.rs`

## Design (PRD §8)

Filesystem paths are byte sequences, not strings, and are not guaranteed to be valid UTF-8 (legacy encodings, NFD artifacts, control characters). `NdexPath` therefore keeps paths as raw platform bytes end to end:

- In SQLite: stored as `BLOB` (schema owned by [manifest](../20-store/22-manifest.md)).
- On the wire: serialized via `Serializer::serialize_bytes`, i.e. a MessagePack `bin` — never a lossy string (encoding owned by [framing](../50-protocol/51-framing.md)).
- For display: lossy UTF-8; for JSON output: `\uXXXX`-escaped (rendering consumed by the [client CLI](../60-interfaces/61-client-cli.md)).

ndex is Unix-only for v0.1; the OS-string conversions are `#[cfg(unix)]`.

## Type ✅

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NdexPath(Vec<u8>);   // private field
```

Ordering, equality, and hashing are byte-wise, so `NdexPath` works as a `BTreeSet`/`HashMap` key (pinned by `ndexpath_orders_and_hashes_by_bytes` in `crates/ndex-core/tests/characterization.rs`). `Default` is the empty path.

## API ✅

| Method | Signature | Behavior |
|---|---|---|
| `new` | `(impl Into<Vec<u8>>) -> Self` | wrap raw bytes, no validation or normalization |
| `as_bytes` | `(&self) -> &[u8]` | borrow raw bytes |
| `into_bytes` | `(self) -> Vec<u8>` | consume into owned bytes |
| `display_lossy` | `(&self) -> Cow<'_, str>` | `String::from_utf8_lossy`; invalid bytes become `U+FFFD` (pinned by `ndexpath_display_is_lossy`) |
| `path_hash` | `(&self) -> u64` | `xxhash_rust::xxh3::xxh3_64` over the raw bytes — a non-cryptographic lookup accelerator (PRD §10.1 `path_hash` column, owned by [manifest](../20-store/22-manifest.md)), explicitly *not* a content hash. Deterministic and discriminating per `ndexpath_hash_is_deterministic_and_discriminating`; collisions are disambiguated downstream by a path-equality check (PRD §10.1) |
| `from_os_str` (`#[cfg(unix)]`) | `(&OsStr) -> Self` | copies the raw Unix bytes (`OsStrExt::as_bytes`) |
| `to_os_string` (`#[cfg(unix)]`) | `(&self) -> OsString` | `OsString::from_vec` of a clone; lossless round-trip incl. invalid UTF-8 pinned by `ndexpath_os_str_roundtrip_preserves_invalid_bytes` |
| `to_json_escaped` | `(&self) -> String` | byte-preserving JSON string rendering, below |

### `to_json_escaped` ✅

Walks the bytes with `utf8_chunks()`, alternating between valid-UTF-8 runs and invalid-byte runs:

- Valid chars are emitted as-is except JSON escapes: `"` → `\"`, `\` → `\\`, `\n`, `\r`, `\t`, `\u{08}` → `\b`, `\u{0c}` → `\f`, and any other char `< 0x20` → `\u{:04x}` (lowercase hex).
- Each invalid byte is emitted as `\u00XX` (lowercase hex), so every source byte is represented.

Pinned by `ndexpath_json_escaping_contract`: `/plain/path` passes through; `a"b\c` → `a\"b\\c`; the three bytes `61 FF 62` → the eight-character literal `a\u00ffb`. Note the output is the *contents* of a JSON string (no surrounding quotes); the caller supplies the quoting. See Divergences for the (non-)reversibility of this encoding.

## Serde ✅

Both impls are hand-written (no derive):

- **Serialize:** `serializer.serialize_bytes(&self.0)` — a byte string, which MessagePack encodes as `bin` and serde_json encodes as an array of integers.
- **Deserialize:** drives `deserialize_byte_buf` with a visitor accepting **four** shapes: `visit_bytes`, `visit_byte_buf` (msgpack `bin`), `visit_str` (UTF-8 string → its bytes), and `visit_seq` of `u8` (JSON's array-of-numbers representation). The `expecting` message is `"raw path bytes (a msgpack bin, byte sequence, or string)"`.

Non-UTF-8 bytes survive a serde round-trip unchanged (`ndexpath_serde_preserves_non_utf8`; the msgpack-`bin`-specific behavior is characterized in [ndex-protocol](../50-protocol/51-framing.md)).

## Divergences & open questions

- **The JSON escaping is not reversible, contrary to the characterization test's comment.** `ndexpath_json_escaping_contract` claims the rendering is reversible, but it is not injective after JSON decoding: the raw invalid byte `0xFF` renders as the escape `\u00ff` (which decodes to the char `ÿ`), while a path containing genuine UTF-8 `ÿ` (bytes `0xC3 0xBF`) renders as the literal char `ÿ` — both decode to the same string. PRD §8 only requires "`\uXXXX` escapes for non-UTF-8 bytes", which the code satisfies; the reversibility claim in the test comment overstates the contract. Consumers must treat JSON path output as display-only, not a lossless encoding.
- **Deserialization is intentionally broader than the doc comment.** The type-level doc says paths are "serialized as a MessagePack `bin` … never as a lossy string", yet `visit_str` accepts strings on input. This is needed for self-describing formats and harmless (string bytes are taken verbatim), but it means a peer *could* send paths as msgpack `str` and be accepted.
- **No path normalization or validation** (no reserved-byte checks, no trailing-slash / `..` handling, empty path allowed via `Default`). Any canonicalization contract belongs to callers; none is defined in core.
