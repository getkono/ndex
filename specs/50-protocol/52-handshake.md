# 52 — Handshake & Version Negotiation

**Owns:** the protocol version constants, the `negotiate` algorithm, the hello-exchange sequence contract, and the cross-version compatibility rules (PRD §12.3).

**Sources:**
- `crates/ndex-protocol/src/handshake.rs` — constants and `negotiate`
- Exchange-sequence intent: doc comments in `crates/ndex-remote/src/serve.rs` and `crates/ndex/src/session.rs` (both bodies `todo!()`)
- Pinned by `crates/ndex-protocol/tests/characterization.rs` and the unit tests in `handshake.rs`

The handshake message *shapes* (`HandshakeReq`, `HandshakeResp`) are owned by [53-messages](53-messages.md); the preamble that precedes the exchange is owned by [51-framing](51-framing.md).

## 1. Protocol version constants ✅

| Constant | Value | Meaning |
|---|---|---|
| `PROTOCOL_VERSION` | `1` | The version this build speaks |
| `MIN_PROTOCOL` | `1` | Lowest version this build accepts |
| `MAX_PROTOCOL` | `1` | Highest version this build accepts |

All three are pinned by `protocol_constants_are_pinned`. This build supports exactly one protocol version; the range machinery exists for future skew.

## 2. `negotiate(client_min, client_max) -> Result<u32>` ✅

Intersects the client's advertised range with this build's `[MIN_PROTOCOL, MAX_PROTOCOL]` and returns the **highest mutually supported version**:

```
hi = min(client_max, MAX_PROTOCOL)
lo = max(client_min, MIN_PROTOCOL)
if lo > hi  →  Err(NdexError::VersionIncompatible)
else        →  Ok(hi)
```

- The error message embeds both ranges: `client supports protocol {min}..={max}, server supports {MIN}..={MAX}`. `VersionIncompatible` maps to the remote-version-incompatible exit code — value owned by [14-errors](../10-core/14-errors.md); the characterization test asserts that mapping via `err.exit_code()`.
- Although symmetric in principle, the function is written from the **server's** perspective: it takes the client's range as arguments and the local build constants as the other side.
- Degenerate input: an inverted client range (`client_min > client_max`) falls out naturally as `lo > hi` → incompatible. There is no explicit validation and no distinct error for it.
- Pinned cases (`negotiate_returns_agreed_version_on_overlap`, `negotiate_errs_when_client_range_cannot_satisfy_server`, plus `handshake.rs` unit tests): `(1,1) → 1`; `(1,5) → MAX_PROTOCOL` (wider client range settles on the server's ceiling); `(MIN,MAX) → MAX`; `(0,0)`, `(99,99)`, and `(MAX+1, MAX+2)` all error with the version-incompatible exit code.

## 3. Hello exchange sequence

The first frames in each direction are always the handshake. The sequence contract (PRD §12.2/§12.3, restated by the `serve.rs`/`session.rs` doc comments):

| # | Actor | Action | Status |
|---|---|---|---|
| 1 | Server | Write the magic preamble ([51-framing](51-framing.md)) as the very first stdout bytes | ⛔ serve loop is `todo!()` ([63-remote](../60-interfaces/63-remote.md)) |
| 2 | Client | Scan for the preamble, discarding leading shell garbage ([51-framing](51-framing.md)) | ⛔ `Session::connect` is `todo!()` ([62-client-transport](../60-interfaces/62-client-transport.md)) |
| 3 | Client | Send `ClientMessage::Handshake(HandshakeReq)` as its first frame, advertising `[min_protocol, max_protocol]`, client version, capabilities, and terminal caps (shape: [53-messages](53-messages.md)) | ⛔ (same) |
| 4 | Server | Call `negotiate(min_protocol, max_protocol)` | function ✅; call site ⛔ |
| 5 | Server | On success: reply `ServerMessage::Handshake(HandshakeResp)` with `protocol_version` = the negotiated version, plus index identity/health snapshot | ⛔ |
| 6 | Server | On failure: reply `ServerMessage::Error` carrying the `VersionIncompatible` code ([14-errors](../10-core/14-errors.md)) — there is **no `HandshakeErr` variant** (see Divergences) | ⛔ |
| 7 | Both | Session proceeds with request/response traffic ([53-messages](53-messages.md)) | ⛔ |

Everything the protocol crate contributes (constants, `negotiate`, message shapes) is ✅ implemented and tested; the exchange itself is unwired at both ends.

## 4. Compatibility contract 🚧

PRD §12.3's rules, cross-checked against the mechanisms that actually exist in code:

| Rule (PRD §12.3) | Compatibility | Enforcing mechanism | Status |
|---|---|---|---|
| Add optional fields to existing messages | Compatible | Container-level `#[serde(default)]` on every `message.rs` payload struct (decoder fills missing fields) + serde's default ignore-unknown-fields (no `deny_unknown_fields` anywhere in the crate) | ✅ for `message.rs` structs; 🚧 the core-owned structs embedded in messages lack `#[serde(default)]` — see [53-messages](53-messages.md) Divergences |
| Add new message variants | Compatible — receiver of an unknown variant replies `Error` | `from_slice` on an unknown variant returns a decode `Err` ✅; translating that into an `Error` reply is the serve loop's job | ⛔ reply behavior unimplemented |
| Remove required fields / change semantics | Breaking — bump the protocol version | Convention only; nothing mechanical | 📋 |
| Version bumps rare (years apart) | — | Policy | 📋 |

The `payload_structs_roundtrip_at_their_defaults` characterization test pins the `#[serde(default)]` half of the additive-fields contract (a fully-defaulted struct survives the codec).

## 5. Capabilities 🚧

Both `HandshakeReq` and `HandshakeResp` carry `capabilities: Vec<String>` (shapes: [53-messages](53-messages.md)). **No capability vocabulary is defined anywhere in the v0.1 code**, and nothing reads the field. Test fixtures use `"progress"`, `"color"`, `"semantic"` as sample values, but these are illustrative, not normative. Mechanism ✅ (field exists and round-trips); semantics 📋.

## 6. What the client learns from the handshake

`HandshakeResp` (fields: [53-messages](53-messages.md)) gives the client an index snapshot before any request: schema version and embedding model identity (semantics: [11-data-model](../10-core/11-data-model.md)), file count, last-reconciled timestamp for staleness display, and an `index_healthy` flag. How the client acts on these (staleness warnings, schema-mismatch errors) is client behavior owned by [61-client-cli](../60-interfaces/61-client-cli.md) / [62-client-transport](../60-interfaces/62-client-transport.md) — all ⛔ today.

## Divergences & open questions

1. **No `HandshakeErr` message.** PRD §12.3 says the server responds with the negotiated version "or `HandshakeErr` with a clear upgrade instruction". The code has no such `ServerMessage` variant; the documented reconciliation (doc comment on `ServerMessage` in `crates/ndex-protocol/src/message.rs`) is to reuse `ServerMessage::Error` with the `VersionIncompatible` code. PRD's "clear upgrade instruction" wording is not reflected in any message text yet (the serve loop is ⛔).
2. **Client-side validation of the negotiated version is unspecified.** Nothing (spec'd or coded) requires the client to check that `HandshakeResp.protocol_version` falls inside the range it advertised; a buggy or hostile server could claim any version. `Session::connect` is ⛔, so the check has nowhere to live yet.
3. **`negotiate` is never exercised over the wire.** It is unit-tested in isolation, but no test performs an actual framed handshake exchange (encode `HandshakeReq` → negotiate → encode `HandshakeResp`/`Error`), because both loop ends are stubs.
4. **Capabilities are a dead field in v0.1.** Exchanged but never consumed, with no defined vocabulary — future use risks each side inventing incompatible strings.
5. **Keepalive is out of scope by design.** PRD §12.5 defers application-level heartbeat to v0.2; the v0.1 mitigation is SSH `ServerAlive*` options, owned by [62-client-transport](../60-interfaces/62-client-transport.md). 📋
