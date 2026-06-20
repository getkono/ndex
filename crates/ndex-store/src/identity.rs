//! `index.toml` open + verification (PRD §5.3).

use std::path::Path;

use ndex_core::error::Result;
use ndex_core::identity::IndexIdentity;

/// Load `<ndex_dir>/index.toml` and refuse to proceed on a schema-version mismatch (PRD §5.3).
pub fn open_identity(ndex_dir: &Path) -> Result<IndexIdentity> {
    // TODO(skeleton): IndexIdentity::load(ndex_dir/index.toml) then check_compatible().
    let _ = ndex_dir;
    todo!()
}

/// Write a fresh, immutable `index.toml` at `init` (PRD §5.3).
pub fn write_identity(ndex_dir: &Path, identity: &IndexIdentity) -> Result<()> {
    let _ = (ndex_dir, identity);
    todo!()
}
