# 33 — Chunking

**Owns:** the algorithm that splits extracted `Block`s into `Chunk`s — window packing, token counting, overlap semantics, ordinal assignment, and byte-offset propagation.

**Sources:**
- `crates/ndex-extract/src/chunk.rs`
- Tests: `crates/ndex-extract/tests/characterization.rs`

Chunking is stage 3 of the ingest pipeline: the reconciler ([31-reconcile.md](31-reconcile.md)) feeds the `Extraction.blocks` produced by an extractor ([32-extraction.md](32-extraction.md)) into `Chunker::chunk(file_id, &blocks)` and writes the resulting chunks to FTS ([../20-store/23-fts.md](../20-store/23-fts.md)).

## 1. Interface — ✅ implemented

`Chunker::new(tokens: &dyn TokenCounter, config: &Chunking)` binds:

- a `TokenCounter` (trait in `ndex-core`; the production counter is the embedding model's tokenizer — see [11-data-model.md](../10-core/11-data-model.md) / [34-embedding.md](34-embedding.md)), and
- the `[chunking]` config section (`target_tokens`, `overlap_tokens`, `min_tokens`, `heading_prefix` — field set and default values owned by [13-config.md](../10-core/13-config.md)).

`chunk(file_id: i64, blocks: &[Block]) -> Vec<Chunk>` is pure: no I/O, no config mutation. `Block` and `Chunk` shapes are owned by [11-data-model.md](../10-core/11-data-model.md).

## 2. Algorithm — 🚧 partial (v0.1 word-window packing)

The PRD §4.5 "recursive structure-aware splitting" design (boundary priority section → paragraph → sentence → word, cross-block merging, heading propagation) is the documented intent — the `Chunker` doc comment restates it — but v0.1 implements only per-block word-window packing. Each block is chunked independently; blocks are never merged or split across each other.

### 2.1 Effective parameters

Computed once per `chunk()` call:

- `target = max(config.target_tokens, 1)`
- `overlap = min(config.overlap_tokens, target - 1)` (saturating; so overlap is always strictly less than target and window advance always makes progress)

### 2.2 Word segmentation (`word_spans`)

A *word* is a maximal run of non-whitespace characters (`char::is_whitespace`, i.e. Unicode whitespace). `word_spans(text)` returns the byte ranges `(start, end)` of every word in block order. Blocks that contain only whitespace (or are empty) yield no spans and are skipped entirely — they produce no chunk.

### 2.3 Token counting

Each word is counted **individually**: `toks[w] = max(tokens.count(word), 1)`. A window's token count is the sum of its words' counts. Consequences:

- A counter that returns 0 for some input is clamped to 1 per word, so windows always terminate.
- Because words are counted in isolation, the sum can differ from the counter's result on the joined text (subword tokenizers merge/split differently across whitespace). Chunk sizes are therefore approximate in model tokens.
- Inter-word whitespace contributes no tokens.

### 2.4 Window packing

For each block, starting at word `i = 0`:

1. **Fill:** extend the window `[i, j)` greedily while `count + toks[j] <= target`. The first word is admitted *unconditionally* (`j == i` case) — a single word whose token count exceeds `target` still forms (or starts) a chunk, so a chunk's size can exceed `target`. There is no sub-word splitting and no hard-max enforcement (PRD §4.5's 8192-token hard max is 📋 — no code).
2. **Emit** a `Chunk`:
   - `file_id` — as passed in;
   - `chunk_ord` — a single counter incremented per emitted chunk, monotonic from 0 **across all blocks of the file** (not per block);
   - `byte_start = block.byte_start + span_start(i)`, `byte_end = block.byte_start + span_end(j-1)` — i.e. offsets into the same coordinate space as the block's offsets (for the text-family extractors that is the decoded/NFC-normalized text, see [32-extraction.md](32-extraction.md) §7.2);
   - `block_type = block.block_type.clone()` — every chunk carries its source block's type;
   - `text` — the exact slice of `block.text` from the first word's start to the last word's end, **including** the original inter-word whitespace (interior newlines survive; leading/trailing whitespace of the window does not, by construction of the spans).
3. **Advance with overlap:** if the window reached the end of the block, stop. Otherwise walk `k` backward from `j`, accumulating `back += toks[k]`, while `k > i + 1 && back < overlap`; the next window starts at `i = max(k, i + 1)`. Properties:
   - the new start is always at least one word past the previous start (progress guaranteed);
   - the realized overlap is measured in the *per-word* token counts and stops at the first word boundary where `back >= overlap`, so it approximates `overlap_tokens` from below/above by at most one word;
   - overlap never reaches back to the previous window's first word (`k > i + 1`).

`Block.heading_path` is read by nothing: heading propagation / `heading_prefix` handling (PRD §4.5 step 5) is 📋 — no code. `min_tokens` is likewise never read — there is no small-chunk merging, and trailing windows may be arbitrarily small (a final 1-word chunk is possible). Note PRD §4.8 defines `min_tokens` as a splitter minimum that must not gate indexing; the current code trivially satisfies the "don't gate" half by ignoring the knob.

### 2.5 Worked example

With `target = 5`, `overlap = 2`, a whitespace token counter, and one paragraph block of 12 one-token words `w0 … w11`:

| chunk_ord | words | why |
|---|---|---|
| 0 | `w0..w4` | 5 tokens fill the target |
| 1 | `w3..w7` | start stepped back 2 tokens from `w5` |
| 2 | `w6..w10` | same |
| 3 | `w9..w11` | final short window (3 tokens; no `min_tokens` merge) |

## 3. Locked-in behavior (characterization)

`chunker_produces_ordered_chunks_for_the_file` (`crates/ndex-extract/tests/characterization.rs`) pins, for a 1000-word block under the default config with a whitespace counter:

- at least one chunk is produced;
- every chunk carries the `file_id` passed to `chunk()`;
- `chunk_ord` equals the chunk's index — monotonically increasing from 0 with no gaps.

The in-module unit test `chunk_boundary_conditions` (`crates/ndex-extract/src/chunk.rs`) is `#[ignore]`d with a `todo!()` body — the named boundary cases (empty input, single token, exactly-`target`, `target + 1`, all-heading blocks) are ⛔ untested.

## 4. Status summary

| Aspect (PRD §4.5) | Status |
|---|---|
| Word-window packing to ~`target_tokens` with `overlap_tokens` overlap | ✅ implemented |
| Token sizing via injected `TokenCounter` | ✅ implemented (per-word approximation, §2.3) |
| Monotonic `chunk_ord`, byte offsets, `block_type` per chunk | ✅ implemented |
| Boundary priority: section/heading → paragraph → sentence → word | 📋 planned (only extractor-provided block boundaries + word windows exist; no sentence logic — `unicode-segmentation` is a declared dependency but unused) |
| Merge consecutive small blocks up to target (`min_tokens`) | 📋 planned (`min_tokens` unread) |
| Heading context propagation (`heading_prefix`, `Block.heading_path`) | 📋 planned (both unread) |
| 8192-token hard max | 📋 planned (oversized single words pass through, §2.4) |

## Divergences & open questions

1. **Doc comment vs body.** The `Chunker` rustdoc claims "boundary priority … heading → paragraph → sentence → word", "small blocks are merged", and "the most recent heading is propagated"; none of that is implemented. The `chunk()` method's own comment correctly scopes v0.1 to word windows — the struct-level comment overstates.
2. **PRD §4.5 chunking algorithm largely unimplemented:** no cross-block merging, no sentence-boundary splitting, no heading prefixing, no `min_tokens`, no hard max. Only PRD step 1 (extractor blocks), step 4 (chunk tuple fields), and an approximation of step 3 (overlap) exist.
3. **Chunks can exceed `target_tokens` without bound** — a single unbroken token run (base64 blob, minified JS "word") becomes one chunk of arbitrary size, and can also exceed the embedding model's context limit; nothing downstream is specified to truncate.
4. **Token accounting is per-word, not per-chunk.** Sizes drift from true model-token counts on subword tokenizers, and whitespace between words is uncounted; the realized chunk size in real tokens is unverified by any test.
5. **Overlap is approximate and asymmetric** (stops at the first word making `back >= overlap`, never reaches the prior window's first word). No test pins realized overlap; the only chunking test checks ordering/`file_id`, and the boundary-condition unit test is an ignored `todo!()`.
6. **Byte-offset coordinate space is inherited, not defined.** Chunk offsets are block-relative sums; for text-family extraction they index the normalized string, not the raw file (see [32-extraction.md](32-extraction.md) Divergence 9). Whether FTS/snippet consumers expect raw-file offsets is unresolved.
