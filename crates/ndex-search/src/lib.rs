//! `ndex-search` — query resolution, retrieval orchestration, fusion, and scoring.
//!
//! Resolves `auto` mode ([`mode`]), prepares queries ([`query`]), runs FTS + semantic retrieval
//! through `ndex-store` and `ndex-embed` ([`search`]), and fuses/normalizes scores ([`fuse`]).
//! Returns engine-level [`Hit`]s; the server joins file metadata to build wire `SearchHit`s.

pub mod fuse;
pub mod mode;
pub mod query;
pub mod search;

pub use fuse::{ScoreExplain, min_max_normalize, rrf_score};
pub use mode::{Resolution, resolve};
pub use query::embed_query;
pub use search::{Hit, SearchOutcome, run};
