//! Embedding model registry and management (PRD §7.4).

use std::path::{Path, PathBuf};

use ndex_core::constants::DEFAULT_MODEL;
use ndex_core::error::{NdexError, Result};

/// Static description of an available embedding model (PRD §7.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// CLI shortname, e.g. `arctic`.
    pub shortname: &'static str,
    /// Full model name (also the on-disk directory name).
    pub full_name: &'static str,
    /// Expected BLAKE3 of `model.onnx` (hex). `None` = not yet pinned; set when a real
    /// model artifact hash is known (release time — RELEASING.md).
    pub onnx_blake3: Option<&'static str>,
    /// Expected BLAKE3 of `tokenizer.json` (hex). `None` = not yet pinned.
    pub tokenizer_blake3: Option<&'static str>,
    /// Native embedding dimensionality.
    pub dimensions: u32,
    /// MRL-truncated, stored/searched dimensionality.
    pub mrl_dimensions: u32,
    /// Number of supported languages.
    pub languages: u32,
    /// Release download URL.
    pub url: &'static str,
}

/// Built-in model registry. v0.1 ships the arctic model only (PRD §7.4).
pub static REGISTRY: &[ModelInfo] = &[ModelInfo {
    shortname: "arctic",
    full_name: DEFAULT_MODEL,
    // Not yet pinned: real release artifact hashes/URL land at packaging time (RELEASING.md).
    onnx_blake3: None,
    tokenizer_blake3: None,
    dimensions: 768,
    mrl_dimensions: 256,
    languages: 74,
    url: "https://github.com/justy/ndex/releases/download/models/snowflake-arctic-embed-m-v2.0.tar.gz",
}];

/// Look up a model by shortname or full name.
pub fn lookup(name: &str) -> Option<&'static ModelInfo> {
    REGISTRY
        .iter()
        .find(|m| m.shortname == name || m.full_name == name)
}

/// All available models.
pub fn list() -> &'static [ModelInfo] {
    REGISTRY
}

/// Root of the model store: `~/.ndex/models/` (PRD §7.4).
pub fn models_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| NdexError::Config("HOME environment variable is not set".into()))?;
    Ok(PathBuf::from(home).join(".ndex").join("models"))
}

/// On-disk directory for a specific model.
pub fn model_path(info: &ModelInfo) -> Result<PathBuf> {
    Ok(models_dir()?.join(info.full_name))
}

/// Fetch a model: download to a `.tmp`, BLAKE3-verify, then atomic rename (PRD §7.4, §16.1).
pub fn fetch(info: &ModelInfo) -> Result<()> {
    // TODO(skeleton): offline-first HTTP download + blake3 verify + rename (deferred ops).
    let _ = info;
    todo!()
}

/// Re-verify a downloaded model's integrity against the registry hashes (PRD §7.4).
pub fn verify(info: &ModelInfo) -> Result<bool> {
    let _ = info;
    todo!()
}

/// Import a pre-staged model tarball for air-gapped servers (PRD §7.4).
pub fn import(tarball: &Path) -> Result<()> {
    let _ = tarball;
    todo!()
}

/// Delete a downloaded model from disk.
pub fn delete(info: &ModelInfo) -> Result<()> {
    let _ = info;
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lookup() {
        assert_eq!(lookup("arctic").unwrap().full_name, DEFAULT_MODEL);
        assert!(lookup(DEFAULT_MODEL).is_some());
        assert!(lookup("does-not-exist").is_none());
    }

    #[test]
    fn arctic_dims_match_prd() {
        let m = lookup("arctic").unwrap();
        assert_eq!(m.dimensions, 768);
        assert_eq!(m.mrl_dimensions, 256);
    }
}
