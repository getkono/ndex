//! Server-side configuration (`config.toml`), mirroring PRD §17.

use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use serde::de::{Error as DeError, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{NdexError, Result};

/// A byte size parsed from human strings like `"2GiB"`, `"512MB"`, or a bare integer (PRD §17).
///
/// Serializes back as a raw `u64` byte count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSize(pub u64);

impl ByteSize {
    /// The size in bytes.
    pub const fn bytes(self) -> u64 {
        self.0
    }
}

impl FromStr for ByteSize {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty byte size".to_string());
        }
        let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
        let (num, unit) = s.split_at(split);
        let num: f64 = num
            .trim()
            .parse()
            .map_err(|_| format!("invalid number in byte size: {s:?}"))?;
        let mult: u64 = match unit.trim().to_ascii_lowercase().as_str() {
            "" | "b" => 1,
            "k" | "kb" => 1_000,
            "kib" => 1 << 10,
            "m" | "mb" => 1_000_000,
            "mib" => 1 << 20,
            "g" | "gb" => 1_000_000_000,
            "gib" => 1 << 30,
            "t" | "tb" => 1_000_000_000_000,
            "tib" => 1 << 40,
            other => return Err(format!("unknown byte unit: {other:?}")),
        };
        Ok(ByteSize((num * mult as f64) as u64))
    }
}

impl Serialize for ByteSize {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = ByteSize;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a byte size: an integer or a string like \"2GiB\"")
            }
            fn visit_u64<E: DeError>(self, v: u64) -> std::result::Result<ByteSize, E> {
                Ok(ByteSize(v))
            }
            fn visit_i64<E: DeError>(self, v: i64) -> std::result::Result<ByteSize, E> {
                u64::try_from(v)
                    .map(ByteSize)
                    .map_err(|_| E::custom("negative byte size"))
            }
            fn visit_str<E: DeError>(self, v: &str) -> std::result::Result<ByteSize, E> {
                v.parse::<ByteSize>().map_err(E::custom)
            }
        }
        d.deserialize_any(V)
    }
}

/// A duration parsed from human strings like `"1h"`, `"7d"`, or a bare integer of seconds.
///
/// Serializes back as a raw `u64` second count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationSetting(pub Duration);

impl DurationSetting {
    /// The underlying [`Duration`].
    pub const fn as_duration(self) -> Duration {
        self.0
    }
    /// Whole seconds.
    pub const fn secs(self) -> u64 {
        self.0.as_secs()
    }
    const fn from_secs(secs: u64) -> Self {
        Self(Duration::from_secs(secs))
    }
}

impl FromStr for DurationSetting {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty duration".to_string());
        }
        let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
        let (num, unit) = s.split_at(split);
        let num: u64 = num
            .trim()
            .parse()
            .map_err(|_| format!("invalid number in duration: {s:?}"))?;
        let secs = match unit.trim().to_ascii_lowercase().as_str() {
            "" | "s" | "sec" | "secs" => num,
            "m" | "min" | "mins" => num * 60,
            "h" | "hr" | "hrs" => num * 3_600,
            "d" | "day" | "days" => num * 86_400,
            "w" | "wk" | "wks" => num * 604_800,
            other => return Err(format!("unknown duration unit: {other:?}")),
        };
        Ok(Self::from_secs(secs))
    }
}

impl Serialize for DurationSetting {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_u64(self.secs())
    }
}

impl<'de> Deserialize<'de> for DurationSetting {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = DurationSetting;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a duration: integer seconds or a string like \"1h\"")
            }
            fn visit_u64<E: DeError>(self, v: u64) -> std::result::Result<DurationSetting, E> {
                Ok(DurationSetting::from_secs(v))
            }
            fn visit_i64<E: DeError>(self, v: i64) -> std::result::Result<DurationSetting, E> {
                u64::try_from(v)
                    .map(DurationSetting::from_secs)
                    .map_err(|_| E::custom("negative duration"))
            }
            fn visit_str<E: DeError>(self, v: &str) -> std::result::Result<DurationSetting, E> {
                v.parse::<DurationSetting>().map_err(E::custom)
            }
        }
        d.deserialize_any(V)
    }
}

/// Chunking parameters (PRD §4.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Chunking {
    pub target_tokens: usize,
    pub overlap_tokens: usize,
    pub min_tokens: usize,
    pub heading_prefix: bool,
}

impl Default for Chunking {
    fn default() -> Self {
        Self {
            target_tokens: 512,
            overlap_tokens: 128,
            min_tokens: 32,
            heading_prefix: true,
        }
    }
}

/// Extraction limits (PRD §4.6, §11.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Extraction {
    pub max_file_size: ByteSize,
    pub max_retries: u32,
}

impl Default for Extraction {
    fn default() -> Self {
        Self {
            max_file_size: ByteSize(2 * (1 << 30)),
            max_retries: 3,
        }
    }
}

/// ONNX embedding parameters (PRD §4.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    pub batch_size: u32,
    /// Backward-compatible alias for `intra_op_threads` (PRD §17).
    pub threads: u32,
    pub intra_op_threads: u32,
    pub inter_op_threads: u32,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            batch_size: 64,
            threads: 0,
            intra_op_threads: 0,
            inter_op_threads: 1,
        }
    }
}

/// Pre-search auto-refresh behavior (PRD §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoRefresh {
    pub enabled: bool,
    pub threshold: DurationSetting,
    pub warn_threshold: DurationSetting,
    pub timeout_secs: u64,
    pub index_new_only: bool,
}

impl Default for AutoRefresh {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: DurationSetting::from_secs(3_600),
            warn_threshold: DurationSetting::from_secs(604_800),
            timeout_secs: 30,
            index_new_only: true,
        }
    }
}

/// Ignore-file behavior (PRD §11.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ignore {
    pub respect_gitignore: bool,
    pub respect_ndexignore: bool,
}

impl Default for Ignore {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            respect_ndexignore: true,
        }
    }
}

/// Filesystem walk behavior (PRD §11.1, §11.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Walk {
    pub follow_symlinks: bool,
    pub hidden: bool,
}

impl Default for Walk {
    fn default() -> Self {
        Self {
            follow_symlinks: true,
            hidden: true,
        }
    }
}

/// Search scoring/tuning knobs (PRD §10.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Search {
    pub default_limit: u32,
    pub rrf_k: u32,
    pub title_boost: f32,
    pub fts_weight: f32,
    pub ef_search: u32,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            default_limit: 20,
            rrf_k: 60,
            title_boost: 2.0,
            fts_weight: 1.0,
            ef_search: 128,
        }
    }
}

/// Archive decompression safety limits (PRD §4.9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Archive {
    pub max_archive_total_size: ByteSize,
    pub max_archive_members: u32,
    pub max_archive_depth: u8,
    pub compression_ratio_limit: u32,
}

impl Default for Archive {
    fn default() -> Self {
        Self {
            max_archive_total_size: ByteSize(8 * (1 << 30)),
            max_archive_members: 100_000,
            max_archive_depth: 3,
            compression_ratio_limit: 200,
        }
    }
}

/// The full server configuration (`config.toml`). Every section and field has a default
/// so that a missing or partial config still loads (PRD §17).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub chunking: Chunking,
    pub extraction: Extraction,
    pub embedding: EmbeddingConfig,
    pub auto_refresh: AutoRefresh,
    pub ignore: Ignore,
    pub walk: Walk,
    pub search: Search,
    pub archive: Archive,
}

impl Config {
    /// Load and parse a `config.toml`.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_toml(&text)
    }

    /// Parse from a TOML string.
    pub fn from_toml(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|e| NdexError::Config(e.to_string()))
    }

    /// Render as a TOML string.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string(self).map_err(|e| NdexError::Config(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesize_parses_human_units() {
        assert_eq!("2GiB".parse::<ByteSize>().unwrap(), ByteSize(2 * (1 << 30)));
        assert_eq!("512MB".parse::<ByteSize>().unwrap(), ByteSize(512_000_000));
        assert_eq!("1024".parse::<ByteSize>().unwrap(), ByteSize(1024));
        assert!("nope".parse::<ByteSize>().is_err());
    }

    #[test]
    fn duration_parses_human_units() {
        assert_eq!("1h".parse::<DurationSetting>().unwrap().secs(), 3_600);
        assert_eq!("7d".parse::<DurationSetting>().unwrap().secs(), 604_800);
        assert_eq!("30".parse::<DurationSetting>().unwrap().secs(), 30);
    }

    #[test]
    fn defaults_match_prd() {
        let c = Config::default();
        assert_eq!(c.chunking.target_tokens, 512);
        assert_eq!(c.extraction.max_file_size.bytes(), 2 * (1 << 30));
        assert_eq!(c.embedding.batch_size, 64);
        assert_eq!(c.auto_refresh.threshold.secs(), 3_600);
        assert_eq!(c.search.rrf_k, 60);
        assert_eq!(c.archive.compression_ratio_limit, 200);
    }

    #[test]
    fn parses_prd_sample_and_roundtrips() {
        let sample = r#"
            [chunking]
            target_tokens = 512
            overlap_tokens = 128

            [extraction]
            max_file_size = "2GiB"

            [auto_refresh]
            threshold = "1h"
            warn_threshold = "7d"
            timeout_secs = 30

            [search]
            title_boost = 2.0
            fts_weight = 1.0
        "#;
        let cfg = Config::from_toml(sample).unwrap();
        assert_eq!(cfg.extraction.max_file_size.bytes(), 2 * (1 << 30));
        assert_eq!(cfg.auto_refresh.warn_threshold.secs(), 604_800);
        // Defaults fill the unspecified keys.
        assert_eq!(cfg.archive.max_archive_members, 100_000);

        let round = Config::from_toml(&cfg.to_toml().unwrap()).unwrap();
        assert_eq!(cfg, round);
    }
}
