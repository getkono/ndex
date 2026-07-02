//! `index.toml` open + verification (PRD §5.3).

use std::path::Path;

use ndex_core::constants::INDEX_TOML;
use ndex_core::error::Result;
use ndex_core::identity::IndexIdentity;

/// Load `<ndex_dir>/index.toml` and refuse to proceed on a schema-version mismatch (PRD §5.3).
pub fn open_identity(ndex_dir: &Path) -> Result<IndexIdentity> {
    let identity = IndexIdentity::load(&ndex_dir.join(INDEX_TOML))?;
    identity.check_compatible()?;
    Ok(identity)
}

/// Write a fresh, immutable `index.toml` at `init` (PRD §5.3).
pub fn write_identity(ndex_dir: &Path, identity: &IndexIdentity) -> Result<()> {
    std::fs::write(ndex_dir.join(INDEX_TOML), identity.to_toml()?)?;
    Ok(())
}
