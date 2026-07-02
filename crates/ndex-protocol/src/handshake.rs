//! Protocol version negotiation (PRD §12.3).

use ndex_core::error::{NdexError, Result};

/// The protocol version this build speaks.
pub const PROTOCOL_VERSION: u32 = 1;
/// Lowest protocol version this build accepts.
pub const MIN_PROTOCOL: u32 = 1;
/// Highest protocol version this build accepts.
pub const MAX_PROTOCOL: u32 = 1;

/// Negotiate the highest protocol version supported by both the client's advertised
/// `[client_min, client_max]` range and this build's `[MIN_PROTOCOL, MAX_PROTOCOL]`.
///
/// Returns [`NdexError::VersionIncompatible`] (exit code 5) when the ranges do not overlap.
pub fn negotiate(client_min: u32, client_max: u32) -> Result<u32> {
    let hi = client_max.min(MAX_PROTOCOL);
    let lo = client_min.max(MIN_PROTOCOL);
    if lo > hi {
        return Err(NdexError::VersionIncompatible(format!(
            "client supports protocol {client_min}..={client_max}, \
             server supports {MIN_PROTOCOL}..={MAX_PROTOCOL}"
        )));
    }
    Ok(hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_overlap() {
        assert_eq!(negotiate(1, 1).unwrap(), 1);
        assert_eq!(negotiate(1, 5).unwrap(), MAX_PROTOCOL);
    }

    #[test]
    fn rejects_disjoint_ranges() {
        let err = negotiate(2, 3).unwrap_err();
        assert_eq!(err.exit_code(), 5);
    }
}
