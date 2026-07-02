//! `[HOST:]PATH` parsing and client host configuration (PRD §13.2, §13.7).

use std::collections::HashMap;

use ndex_core::error::Result;
use serde::Deserialize;

/// A parsed search/index target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Remote host or alias; `None` for a local index.
    pub host: Option<String>,
    /// The index root path (possibly empty when a host alias supplies a default root).
    pub path: String,
}

/// Parse `[HOST:]PATH` (PRD §13.2).
///
/// A leading `host:` prefix selects a remote **only** when the text before the first colon
/// contains no `/` — so `nas:/pool` is remote, but `/pool:weird` and `rel/path` are local.
pub fn parse_target(input: &str) -> Target {
    if let Some((maybe_host, rest)) = input.split_once(':')
        && !maybe_host.is_empty()
        && !maybe_host.contains('/')
    {
        return Target {
            host: Some(maybe_host.to_string()),
            path: rest.to_string(),
        };
    }
    Target {
        host: None,
        path: input.to_string(),
    }
}

/// `~/.config/ndex/hosts.toml` — host aliases (PRD §13.7).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HostsConfig {
    #[serde(default)]
    pub hosts: HashMap<String, HostEntry>,
}

/// One `[hosts.<alias>]` entry.
#[derive(Debug, Clone, Deserialize)]
pub struct HostEntry {
    pub hostname: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub key: Option<String>,
    pub remote_path: Option<String>,
    pub default_root: Option<String>,
}

/// Load `hosts.toml` from the client config directory (PRD §13.7).
pub fn load_hosts() -> Result<HostsConfig> {
    // TODO(skeleton): read $NDEX_CONFIG_DIR or ~/.config/ndex/hosts.toml; parse TOML.
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_and_local_targets() {
        assert_eq!(
            parse_target("nas:/pool/archive"),
            Target {
                host: Some("nas".into()),
                path: "/pool/archive".into()
            }
        );
        assert_eq!(
            parse_target("nas.local:/pool"),
            Target {
                host: Some("nas.local".into()),
                path: "/pool".into()
            }
        );
        // Host alias with no path (default root applies later).
        assert_eq!(
            parse_target("nas:"),
            Target {
                host: Some("nas".into()),
                path: String::new()
            }
        );
        // Absolute and relative local paths.
        assert_eq!(
            parse_target("/pool/archive"),
            Target {
                host: None,
                path: "/pool/archive".into()
            }
        );
        assert_eq!(
            parse_target("rel/path"),
            Target {
                host: None,
                path: "rel/path".into()
            }
        );
        // A colon after a slash is part of the path, not a host separator.
        assert_eq!(
            parse_target("/pool:weird"),
            Target {
                host: None,
                path: "/pool:weird".into()
            }
        );
    }
}
