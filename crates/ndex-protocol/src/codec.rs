//! MessagePack (de)serialization helpers.
//!
//! Always uses the `_named` encoding so struct fields serialize as named map keys —
//! required for correct externally-tagged enum decoding and `#[serde(default)]`
//! forward-compatibility (PRD §12.4).

use ndex_core::error::{NdexError, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Serialize a value to MessagePack with named struct fields.
pub fn to_vec_named<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    rmp_serde::to_vec_named(value).map_err(|e| NdexError::Protocol(format!("encode: {e}")))
}

/// Deserialize a value from a MessagePack byte slice.
pub fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    rmp_serde::from_slice(bytes).map_err(|e| NdexError::Protocol(format!("decode: {e}")))
}
