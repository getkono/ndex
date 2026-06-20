//! `auto`-mode resolution heuristics (PRD §10.7).

use ndex_core::SearchMode;

/// Stop words whose presence in a short query signals natural-language intent (PRD §10.7).
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "in", "on", "at", "for", "to", "of", "with",
    "by", "from", "how", "what", "why", "when", "where", "which", "who",
];

/// Resolve a requested mode to the mode actually used (PRD §10.7).
///
/// Explicit modes pass through, except that `Semantic`/`Hybrid` fall back to `Fts` when the
/// vector index is empty (PRD §16.3). `Auto` applies the query-characteristic heuristics.
pub fn resolve(query: &str, requested: SearchMode, vectors_empty: bool) -> SearchMode {
    match requested {
        SearchMode::Fts => SearchMode::Fts,
        SearchMode::Semantic if vectors_empty => SearchMode::Fts,
        SearchMode::Semantic => SearchMode::Semantic,
        SearchMode::Hybrid if vectors_empty => SearchMode::Fts,
        SearchMode::Hybrid => SearchMode::Hybrid,
        SearchMode::Auto => resolve_auto(query, vectors_empty),
    }
}

fn resolve_auto(query: &str, vectors_empty: bool) -> SearchMode {
    if vectors_empty || has_phrase(query) || has_operators(query) {
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
        assert_eq!(resolve("blake3", SearchMode::Auto, false), SearchMode::Fts);
        assert_eq!(
            resolve("config.toml", SearchMode::Auto, false),
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
            ),
            SearchMode::Hybrid
        );
        // Short but contains a stop word → natural-language intent → hybrid.
        assert_eq!(
            resolve("what is blake3", SearchMode::Auto, false),
            SearchMode::Hybrid
        );
    }

    #[test]
    fn phrases_and_operators_use_fts() {
        assert_eq!(
            resolve("\"exact phrase here\"", SearchMode::Auto, false),
            SearchMode::Fts
        );
        assert_eq!(
            resolve("invoice AND 2024", SearchMode::Auto, false),
            SearchMode::Fts
        );
        assert_eq!(
            resolve("mime:application/pdf", SearchMode::Auto, false),
            SearchMode::Fts
        );
    }

    #[test]
    fn empty_vectors_fall_back_to_fts() {
        assert_eq!(
            resolve("long natural language query here", SearchMode::Auto, true),
            SearchMode::Fts
        );
        assert_eq!(
            resolve("anything", SearchMode::Semantic, true),
            SearchMode::Fts
        );
        assert_eq!(
            resolve("anything", SearchMode::Hybrid, true),
            SearchMode::Fts
        );
    }

    #[test]
    fn explicit_modes_pass_through() {
        assert_eq!(resolve("x", SearchMode::Fts, false), SearchMode::Fts);
        assert_eq!(
            resolve("x", SearchMode::Semantic, false),
            SearchMode::Semantic
        );
        assert_eq!(resolve("x", SearchMode::Hybrid, false), SearchMode::Hybrid);
    }
}
