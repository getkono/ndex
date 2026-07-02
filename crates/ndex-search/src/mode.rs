//! `auto`-mode resolution heuristics and the empty-vector fallback policy (PRD §10.7, §16.3).

use ndex_core::SearchMode;

/// Stop words whose presence in a short query signals natural-language intent (PRD §10.7).
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "in", "on", "at", "for", "to", "of", "with",
    "by", "from", "how", "what", "why", "when", "where", "which", "who",
];

/// Warning when explicit `semantic` cannot run (PRD §16.3: zero hits, no silent FTS fallback).
const WARN_SEMANTIC_EMPTY_VECTORS: &str = "Vector index is empty; semantic search returned no \
     results. Run `ndex index` to build it, or use `--mode auto` to fall back to full-text \
     search.";

/// Warning when explicit `hybrid` degrades to FTS-only (PRD §16.3).
const WARN_HYBRID_EMPTY_VECTORS: &str = "Vector index is empty; hybrid search fell back to \
     full-text only. Run `ndex index` to enable semantic retrieval.";

/// Warning when `auto` skips semantic ranking because there are no vectors (PRD §10.7).
const WARN_AUTO_EMPTY_VECTORS: &str = "Vector index is empty; semantic ranking skipped (results \
     are full-text only). Run `ndex index` to enable it.";

/// The outcome of mode resolution: the mode retrieval will actually execute, plus any
/// user-facing warnings the caller must surface (PRD §10.7 "fallback with warning", §16.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The mode retrieval executes. Explicit `Semantic` with an empty vector index stays
    /// `Semantic`: nothing can run, and the search returns zero hits (PRD §16.3).
    pub mode: SearchMode,
    /// Warnings for the caller to surface (the CLI prints them to stderr). Empty when no
    /// degradation occurred.
    pub warnings: Vec<String>,
}

impl Resolution {
    fn clean(mode: SearchMode) -> Self {
        Self {
            mode,
            warnings: Vec::new(),
        }
    }

    fn warned(mode: SearchMode, warning: &str) -> Self {
        Self {
            mode,
            warnings: vec![warning.to_owned()],
        }
    }
}

/// Resolve a requested mode to the mode actually used, plus fallback warnings (PRD §10.7, §16.3).
///
/// Explicit modes pass through when vectors exist. With an empty/absent vector index:
/// `Semantic` stays `Semantic` but warns — the caller runs nothing and returns zero hits
/// (semantic is explicit opt-in; it never silently serves BM25); `Hybrid` falls back to
/// `Fts` with a warning (its FTS half is still meaningful); `Auto` selects `Fts` with a
/// warning that semantic ranking was skipped. `Auto` with vectors present applies the
/// query-characteristic heuristics.
pub fn resolve(query: &str, requested: SearchMode, vectors_empty: bool) -> Resolution {
    match requested {
        SearchMode::Fts => Resolution::clean(SearchMode::Fts),
        SearchMode::Semantic if vectors_empty => {
            Resolution::warned(SearchMode::Semantic, WARN_SEMANTIC_EMPTY_VECTORS)
        }
        SearchMode::Semantic => Resolution::clean(SearchMode::Semantic),
        SearchMode::Hybrid if vectors_empty => {
            Resolution::warned(SearchMode::Fts, WARN_HYBRID_EMPTY_VECTORS)
        }
        SearchMode::Hybrid => Resolution::clean(SearchMode::Hybrid),
        SearchMode::Auto if vectors_empty => {
            Resolution::warned(SearchMode::Fts, WARN_AUTO_EMPTY_VECTORS)
        }
        SearchMode::Auto => Resolution::clean(resolve_auto(query)),
    }
}

/// Query-characteristic heuristics for `auto` (PRD §10.7); reached only when vectors exist.
fn resolve_auto(query: &str) -> SearchMode {
    if has_phrase(query) || has_operators(query) {
        return SearchMode::Fts;
    }
    let tokens: Vec<&str> = query.split_whitespace().collect();
    // Short keyword queries with no stop words are pure-keyword → FTS; everything else → hybrid.
    if tokens.len() <= 3 && !tokens.iter().any(|t| is_stop_word(t)) {
        return SearchMode::Fts;
    }
    SearchMode::Hybrid
}

fn has_phrase(query: &str) -> bool {
    query.matches('"').count() >= 2
}

fn has_operators(query: &str) -> bool {
    query.contains(':')
        || query
            .split_whitespace()
            .any(|t| matches!(t, "AND" | "OR" | "NOT"))
}

fn is_stop_word(token: &str) -> bool {
    STOP_WORDS.contains(&token.to_ascii_lowercase().as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_keyword_queries_use_fts() {
        assert_eq!(
            resolve("blake3", SearchMode::Auto, false).mode,
            SearchMode::Fts
        );
        assert_eq!(
            resolve("config.toml", SearchMode::Auto, false).mode,
            SearchMode::Fts
        );
    }

    #[test]
    fn natural_language_uses_hybrid() {
        assert_eq!(
            resolve(
                "how do I configure the embedding model",
                SearchMode::Auto,
                false
            )
            .mode,
            SearchMode::Hybrid
        );
        // Short but contains a stop word → natural-language intent → hybrid.
        assert_eq!(
            resolve("what is blake3", SearchMode::Auto, false).mode,
            SearchMode::Hybrid
        );
    }

    #[test]
    fn phrases_and_operators_use_fts() {
        assert_eq!(
            resolve("\"exact phrase here\"", SearchMode::Auto, false).mode,
            SearchMode::Fts
        );
        assert_eq!(
            resolve("invoice AND 2024", SearchMode::Auto, false).mode,
            SearchMode::Fts
        );
        assert_eq!(
            resolve("mime:application/pdf", SearchMode::Auto, false).mode,
            SearchMode::Fts
        );
    }

    #[test]
    fn empty_vectors_auto_and_hybrid_fall_back_to_fts_with_warning() {
        let auto = resolve("long natural language query here", SearchMode::Auto, true);
        assert_eq!(auto.mode, SearchMode::Fts);
        assert_eq!(auto.warnings, vec![WARN_AUTO_EMPTY_VECTORS.to_owned()]);

        let hybrid = resolve("anything", SearchMode::Hybrid, true);
        assert_eq!(hybrid.mode, SearchMode::Fts);
        assert_eq!(hybrid.warnings, vec![WARN_HYBRID_EMPTY_VECTORS.to_owned()]);

        // Explicit FTS is unaffected — no fallback, no warning.
        let fts = resolve("anything", SearchMode::Fts, true);
        assert_eq!(fts.mode, SearchMode::Fts);
        assert!(fts.warnings.is_empty());
    }

    #[test]
    fn empty_vectors_explicit_semantic_stays_semantic_with_warning() {
        let sem = resolve("anything", SearchMode::Semantic, true);
        assert_eq!(sem.mode, SearchMode::Semantic);
        assert_eq!(sem.warnings, vec![WARN_SEMANTIC_EMPTY_VECTORS.to_owned()]);
    }

    #[test]
    fn explicit_modes_pass_through() {
        for m in [SearchMode::Fts, SearchMode::Semantic, SearchMode::Hybrid] {
            let r = resolve("x", m, false);
            assert_eq!(r.mode, m);
            assert!(r.warnings.is_empty());
        }
    }
}
