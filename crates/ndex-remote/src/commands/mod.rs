//! Standalone command handlers (also reused by the serve loop's dispatch).
//!
//! Each handler wires the engine crates (store/extract/embed/search/reconcile). Bodies are
//! `todo!()`; the v0.2 command stubs return a clear "planned for v0.2" error.

pub mod completions;
pub mod indexing;
pub mod maintain;
pub mod model;
pub mod read;

use ndex_core::error::{NdexError, Result};

/// Shared handler for the v0.2 command stubs (PRD §13.1).
pub fn unavailable_v0_2(command: &str) -> Result<()> {
    Err(NdexError::Other(format!(
        "'ndex-remote {command}' is planned for v0.2 and not yet available."
    )))
}
