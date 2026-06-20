//! Document language detection via `whatlang` (PRD §10.2).

/// Minimum text length before attempting detection (PRD §10.2).
pub const MIN_DETECT_LEN: usize = 20;

/// Detect the dominant language of `text`, returning a language code, or `None` for very short
/// or low-confidence input (PRD §10.2).
///
/// Note: `whatlang` returns ISO 639-3 codes (e.g. `eng`); mapping to the ISO 639-1 codes stored
/// in `doc_meta.lang` (e.g. `en`) is a follow-up refinement.
pub fn detect(text: &str) -> Option<String> {
    if text.len() < MIN_DETECT_LEN {
        return None;
    }
    let info = whatlang::detect(text)?;
    if !info.is_reliable() {
        return None;
    }
    Some(info.lang().code().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_clear_english() {
        let text = "The annual report summarizes the company's financial performance over the \
                    past fiscal year, including revenue growth, operating margins, and the \
                    projected earnings expected across the coming quarters and beyond.";
        assert_eq!(detect(text).as_deref(), Some("eng"));
    }

    #[test]
    fn rejects_too_short() {
        assert!(detect("hi").is_none());
    }
}
