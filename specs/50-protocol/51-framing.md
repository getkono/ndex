# 51 — Wire Framing & Codec

**Owns:** the byte-level wire format of the IPC channel — the magic preamble, the length-prefixed frame layout, the frame/scan size limits, and the MessagePack codec settings that every message passes through.

**Sources:**
- `crates/ndex-protocol/src/frame.rs` — `FrameWriter`, `FrameReader`, preamble scan
- `crates/ndex-protocol/src/codec.rs` — `to_vec_named`, `from_slice`
- `crates/ndex-core/src/constants.rs` — the three wire constants below (the remaining constants in that file are store/config facts owned by [13-config](../10-core/13-config.md))
- Pinned by `crates/ndex-protocol/tests/characterization.rs` and the unit tests in `frame.rs`

The protocol runs over a byte-stream pair: the server's stdin/stdout, reached via SSH or a local subprocess (transport ownership: [62-client-transport](../60-interfaces/62-client-transport.md), [63-remote](../60-interfaces/63-remote.md)).

## 1. Stream layout ✅

The two directions are not symmetric — only the **server → client** stream carries the preamble, because only that direction can be contaminated by shell-startup output (PRD §12.2):

```
server → client:  [ ≤ garbage budget of shell noise ] NDEX\x00\x01 [frame] [frame] …
client → server:  [frame] [frame] …                      (no preamble, no garbage tolerance)
```

`FrameReader::scan_preamble` is the client-side entry point; `FrameWriter::write_preamble` is the server-side one. Frames in both directions are identical in layout.

## 2. Wire constants ✅

| Constant | Value | Meaning |
|---|---|---|
| `MAGIC_PREAMBLE` | `b"NDEX\x00\x01"` = `4E 44 45 58 00 01` (6 bytes) | ASCII `NDEX` + NUL + protocol-epoch byte `0x01`; written once by the server before any frame |
| `MAX_FRAME_BYTES` | `16 * 1024 * 1024` = 16 777 216 (16 MiB) | Maximum **payload** size of one frame (excludes the 4-byte header); enforced on both read and write |
| `MAX_PREAMBLE_SCAN_BYTES` | `4096` | Leading-garbage budget the client tolerates before the preamble (see §5 for the exact boundary) |

The exact preamble bytes are pinned by `write_preamble_emits_exactly_the_magic_bytes`.

## 3. Frame layout ✅

```
offset   0               4
         ┌───────────────┬─────────────────────────────────┐
         │ length: u32   │ payload: length bytes           │
         │ big-endian    │ (MessagePack-encoded message)   │
         └───────────────┴─────────────────────────────────┘
```

- Length prefix is a **big-endian `u32`** counting payload bytes only. Pinned byte-exactly: a 5-byte payload produces header `00 00 00 05`; a 258-byte payload produces `00 00 01 02` (`length_prefix_is_u32_big_endian`).
- Largest legal frame on the wire: 4 + 16 777 216 bytes. The cap is inclusive on both sides: a payload of exactly `MAX_FRAME_BYTES` is accepted (`frame_at_exactly_max_frame_bytes_roundtrips`); one byte more is rejected on write (`write_frame_rejects_oversize_payload`) and read (`read_frame_rejects_length_prefix_over_max_frame_bytes`).
- **Empty payloads (`length = 0`) are legal at the framing layer** and round-trip (`frame_roundtrips_including_empty_payload`) — but an empty payload is not a decodable message; `from_slice(&[])` errors (`from_slice_on_garbage_bytes_is_err_not_panic`).
- There is no checksum, sequence number, compression, or padding. Integrity is delegated to the transport (SSH / local pipe).

## 4. Write path ✅ — `FrameWriter<W: Write>`

`crates/ndex-protocol/src/frame.rs`

| Method | Behavior |
|---|---|
| `new(inner)` | Wraps any `W: Write`; no buffering added |
| `write_preamble()` | Writes `MAGIC_PREAMBLE`, then flushes. Server side, once, before any frame |
| `write_frame(payload)` | Rejects `payload.len() > MAX_FRAME_BYTES` **before writing anything**; then writes the BE header, the payload, and flushes |
| `into_inner()` | Returns the transport (pinned by `frame_writer_into_inner_returns_the_transport`) |

- Oversize error: `NdexError::Protocol` (taxonomy: [14-errors](../10-core/14-errors.md)) with message text `frame size {N} exceeds 16777216 byte limit`. Pinned by `write_frame_rejects_oversize_payload`.
- A defensive `u32::try_from` guards the length conversion (`"frame length overflow"`) — unreachable on 32/64-bit targets given the size check, but present.
- **Every frame is flushed individually.** This is a deliberate latency choice: progress events and results must reach the client immediately over a pipe.

## 5. Read path ✅ — `FrameReader<R: Read>`

`crates/ndex-protocol/src/frame.rs`

### `read_frame() -> Result<Vec<u8>>`

1. `read_exact` 4 header bytes. EOF or a short read surfaces as `NdexError::Io` (`UnexpectedEof`), *not* a protocol error — pinned as an error (not a hang/panic) by `read_frame_on_empty_stream_is_err`.
2. Decode BE `u32`; if it exceeds `MAX_FRAME_BYTES`, return `NdexError::Protocol` with the same `frame size {N} exceeds 16777216 byte limit` text — **before allocating or reading the body**. Pinned by `read_frame_rejects_length_prefix_over_max_frame_bytes` (boundary +1 and `u32::MAX`).
3. Allocate `len` zeroed bytes (worst case one 16 MiB allocation) and `read_exact` the payload.

After any error the stream position is undefined; there is no resynchronization mechanism — the caller's only sound move is to drop the connection. PRD §12.2 additionally requires the receiver to *send an `Error` message (if possible) and close the connection* on overflow; that behavior belongs to the (stubbed) serve loop and session ([63-remote](../60-interfaces/63-remote.md), [62-client-transport](../60-interfaces/62-client-transport.md)) — the framing layer only returns `Err`.

### `scan_preamble() -> Result<()>`

Byte-at-a-time incremental match against `MAGIC_PREAMBLE`:

```
matched = 0, consumed = 0
loop:
    read 1 byte; consumed += 1
    if byte == magic[matched]:
        matched += 1
        if matched == 6: return Ok
    else:
        matched = (byte == magic[0]) ? 1 : 0   # single-byte restart
    if consumed - matched > MAX_PREAMBLE_SCAN_BYTES: return Err(Protocol)
```

- The single-byte restart on mismatch is correct only because `MAGIC_PREAMBLE` has no self-overlapping prefix/suffix (asserted by a code comment, exploited by `scan_preamble_handles_partial_false_start`, which feeds `NDEXNDEX\x00` teaser prefixes).
- Budget: `consumed - matched` is the garbage count under the most optimistic assumption that the current partial match completes, so the error fires at the earliest byte where success within the budget has become impossible. The garbage tolerance is **exactly `MAX_PREAMBLE_SCAN_BYTES` (4096) bytes, inclusive**: a preamble preceded by 4096 garbage bytes succeeds; 4097 fails. Boundary pinned at 4095/4096/4097 by `scan_preamble_garbage_budget_is_exactly_max_scan_bytes`.
- Failure text (exact): `protocol preamble not found; server stdout may be contaminated by shell startup output (see PRD §12.2)`.
- Any read failure mid-scan (including EOF) is wrapped as `NdexError::Protocol("reading preamble: {e}")` — unlike `read_frame`, where I/O errors pass through as `NdexError::Io`. Pinned by `scan_preamble_errs_on_empty_stream`.
- Pinned behaviors: success at stream start, success after realistic MOTD garbage, the exact 4095/4096/4097 budget boundary, failure after `MAX_PREAMBLE_SCAN_BYTES + 100` garbage bytes, failure on empty stream.

## 6. MessagePack codec ✅

`crates/ndex-protocol/src/codec.rs` — two thin wrappers around `rmp-serde` (dependency and version pinning: [71-toolchain](../70-operations/71-toolchain.md)):

| Function | Wraps | Error mapping |
|---|---|---|
| `to_vec_named<T: Serialize>(&T) -> Result<Vec<u8>>` | `rmp_serde::to_vec_named` | `NdexError::Protocol("encode: {e}")` |
| `from_slice<T: DeserializeOwned>(&[u8]) -> Result<T>` | `rmp_serde::from_slice` | `NdexError::Protocol("decode: {e}")` |

- **`_named` (struct-map) mode is mandatory** (PRD §12.4): struct fields serialize as string-keyed map entries, which is what makes externally-tagged enum decoding and `#[serde(default)]` forward-compatibility work. This is the codec decision that carries PRD §12.3's additive-evolution rule — with maps, a decoder skips unknown keys and fills absent defaulted fields; the compact positional encoding (`rmp_serde::to_vec`) breaks both properties and must never be used on the wire. Verified and pinned by the cross-version decode tests (`decode_ignores_unknown_extra_field`, `decode_fills_missing_defaulted_field`, `handshake_req_decodes_when_new_fields_are_absent`; contract details owned by [53-messages](53-messages.md) §1). How enums and each field type map to MessagePack is owned by [53-messages](53-messages.md).
- Decode is total: truncated input (`from_slice_on_truncated_bytes_is_err_not_panic`) and garbage input including the empty slice (`from_slice_on_garbage_bytes_is_err_not_panic`) return `Err`, never panic.
- The codec is deterministic for a given value but **not canonical** — nothing depends on byte-identical re-encoding.

## 7. End-to-end data path ✅

`encode → frame → transport → deframe → decode` is exercised as a whole by `encoded_message_survives_a_full_frame_roundtrip` (a `ServerMessage::Progress` through `to_vec_named` → `write_frame` → `read_frame` → `from_slice` over an in-memory `Cursor`).

## Divergences & open questions

1. **Oversize error text differs from PRD §12.2.** PRD prescribes `"Frame size <N> exceeds 16 MiB limit."`; code emits `frame size {N} exceeds 16777216 byte limit`. Cosmetic, but the PRD text is presented as a contract.
2. **Overflow response behavior is unimplemented.** PRD §12.2 requires the receiver to close the connection and, if possible, send an `Error` message; `read_frame` only returns `Err`. Enforcement is deferred to the ⛔ serve loop / session. Likewise PRD's guarantee that the server truncates a pathological `SearchHit` snippet before framing has no code anywhere.
3. **Preamble epoch byte vs. protocol version.** `constants.rs` calls the trailing `0x01` a "protocol-epoch byte"; PRD §12.2 calls it a "version byte". Neither defines whether it tracks the negotiated protocol version ([52-handshake](52-handshake.md)) — i.e., whether a future protocol 2 changes the preamble. Undefined.
4. **Error-type asymmetry between the two read paths.** EOF during `read_frame` is `NdexError::Io`; EOF during `scan_preamble` is `NdexError::Protocol`. Both map to the same generic exit code today ([14-errors](../10-core/14-errors.md)), but the inconsistency will matter if exit-code mapping ever distinguishes them.

*Resolved 2026-07: the former garbage-tolerance off-by-one (a preamble after 4097 garbage bytes was accepted while the doc comment promised 4096) — the budget check is now `consumed - matched > MAX_PREAMBLE_SCAN_BYTES`, making the tolerance exactly 4096 inclusive, boundary-pinned (§5).*
