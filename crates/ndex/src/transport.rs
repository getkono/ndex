//! Transport to an `ndex-remote serve` process: local subprocess or SSH (PRD §3, §12.6).

use std::io::{Read, Write};

use ndex_core::error::Result;

/// The remote-discovery probe order on the server (PRD §12.6).
pub const REMOTE_DISCOVERY_PATHS: &[&str] = &[
    "ndex-remote",
    "~/.local/bin/ndex-remote",
    "/usr/local/bin/ndex-remote",
];

/// Default SSH options ndex injects unless overridden (PRD §12.5, §12.6).
pub const DEFAULT_SSH_OPTIONS: &[&str] = &[
    "-T",
    "-o",
    "BatchMode=yes",
    "-o",
    "ServerAliveInterval=30",
    "-o",
    "ServerAliveCountMax=3",
];

/// The connected streams: the remote's stdin (we write) and stdout (we read).
pub type Streams = (Box<dyn Write + Send>, Box<dyn Read + Send>);

/// A way to spawn and connect to `ndex-remote serve`.
pub trait Transport {
    /// Spawn the remote and return its `(stdin writer, stdout reader)`.
    fn spawn(&self) -> Result<Streams>;
}

/// Spawn `ndex-remote serve` as a local subprocess (no SSH) — PRD §3 local mode.
pub struct LocalTransport {
    pub remote_path: String,
    pub root: String,
}

impl Transport for LocalTransport {
    fn spawn(&self) -> Result<Streams> {
        // TODO(skeleton): std::process::Command::new(remote_path).args(["serve","--root",root])
        //                 .stdin(piped).stdout(piped).spawn(); box the child streams.
        let _ = (&self.remote_path, &self.root);
        todo!()
    }
}

/// Spawn `ndex-remote serve` over SSH (PRD §12.6).
pub struct SshTransport {
    pub host: String,
    pub user: Option<String>,
    pub port: u16,
    pub key: Option<String>,
    pub remote_path: Option<String>,
    pub root: String,
    pub ssh_options: Vec<String>,
}

impl Transport for SshTransport {
    fn spawn(&self) -> Result<Streams> {
        // TODO(skeleton): build `ssh [DEFAULT_SSH_OPTIONS] [-i key] [-p port] user@host
        //                 "ndex-remote serve --root <root>"`; map host-key failure to a clear error.
        let _ = (
            &self.host,
            &self.user,
            self.port,
            &self.key,
            &self.remote_path,
            &self.root,
            &self.ssh_options,
        );
        todo!()
    }
}
