//! The index identity file (`index.toml`) and schema-version policy (PRD §5.3).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{NdexError, Result};

/// Current index schema version. Bumped on any breaking index-format change; ndex refuses to
/// open an index with a different version and requires a full rebuild (PRD §5).
pub const SCHEMA_VERSION: u32 = 3;

/// Contents of `index.toml`. Written once at `init`, never modified (PRD §5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexIdentity {
    pub identity: Identity,
    pub embedding: EmbeddingIdentity,
    pub hashing: Hashing,
    pub fts: FtsIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub schema_version: u32,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingIdentity {
    pub model_name: String,
    /// BLAKE3 of the ONNX model file, hex.
    pub model_hash: String,
    pub dimensions: u32,
    pub mrl_dimensions: u32,
    /// Stored embedding precision, e.g. `f16` (PRD §5.3).
    pub vector_scalar: String,
    pub hnsw_m: u32,
    pub hnsw_ef_construction: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hashing {
    pub algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FtsIdentity {
    pub tokenizer_version: u32,
}

impl IndexIdentity {
    /// Load `index.toml`.
    ///
    /// A missing file is [`NdexError::IndexNotFound`] (exit 3); other I/O failures stay
    /// [`NdexError::Io`]; malformed TOML is [`NdexError::Config`].
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                NdexError::IndexNotFound(path.display().to_string())
            } else {
                NdexError::Io(e)
            }
        })?;
        toml::from_str(&text).map_err(|e| NdexError::Config(e.to_string()))
    }

    /// Render as TOML.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string(self).map_err(|e| NdexError::Config(e.to_string()))
    }

    /// Refuse to open an index whose schema version differs from this build (PRD §5.3).
    pub fn check_compatible(&self) -> Result<()> {
        if self.identity.schema_version != SCHEMA_VERSION {
            return Err(NdexError::SchemaMismatch(format!(
                "index schema version {} is not supported (this build expects {}); run `ndex reindex`",
                self.identity.schema_version, SCHEMA_VERSION
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> IndexIdentity {
        IndexIdentity {
            identity: Identity {
                schema_version: SCHEMA_VERSION,
                created_by: "ndex-remote 0.1.0".to_string(),
                created_at: "2026-03-17T08:00:00Z".to_string(),
            },
            embedding: EmbeddingIdentity {
                model_name: "snowflake-arctic-embed-m-v2.0".to_string(),
                model_hash: "a3f2e8".to_string(),
                dimensions: 768,
                mrl_dimensions: 256,
                vector_scalar: "f16".to_string(),
                hnsw_m: 32,
                hnsw_ef_construction: 200,
            },
            hashing: Hashing {
                algorithm: "blake3".to_string(),
            },
            fts: FtsIdentity {
                tokenizer_version: 1,
            },
        }
    }

    #[test]
    fn toml_roundtrip() {
        let id = sample();
        let round: IndexIdentity = toml::from_str(&id.to_toml().unwrap()).unwrap();
        assert_eq!(id, round);
    }

    #[test]
    fn rejects_mismatched_schema() {
        let mut id = sample();
        assert!(id.check_compatible().is_ok());
        id.identity.schema_version = SCHEMA_VERSION + 1;
        let err = id.check_compatible().unwrap_err();
        assert_eq!(err.exit_code(), 6);
    }
}
