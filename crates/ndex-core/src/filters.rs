//! Search request types: execution mode and filter set.

use serde::{Deserialize, Serialize};

/// Search execution mode (PRD §10.7).
///
/// Lives in `ndex-core` so `ndex-search` can resolve and return it without depending on
/// `ndex-protocol`; the wire protocol re-exports it (and serializes it as the variant name).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SearchMode {
    #[default]
    Auto,
    Fts,
    Semantic,
    Hybrid,
}

/// Filters applied to a search (PRD §12.7).
///
/// Lives in `ndex-core` so that both `ndex-protocol` (the wire `SearchRequestData`)
/// and `ndex-search` can use it — `ndex-search` does not depend on `ndex-protocol`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchFilters {
    /// MIME glob, e.g. `image/*`.
    pub mime: Option<String>,
    /// Modified-after, unix nanoseconds.
    pub after_ns: Option<i64>,
    /// Modified-before, unix nanoseconds.
    pub before_ns: Option<i64>,
    /// Minimum size in bytes.
    pub larger: Option<u64>,
    /// Maximum size in bytes.
    pub smaller: Option<u64>,
    /// Path glob, e.g. `invoices/**/*.pdf`.
    pub path_glob: Option<String>,
    /// Tag filter with OR semantics.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Language filter (ISO 639-1).
    pub lang: Option<String>,
}
