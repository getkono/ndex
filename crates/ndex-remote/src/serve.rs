//! The msgpack serve loop invoked over SSH (PRD §12, §13.11).

use ndex_core::error::Result;

use crate::cli::ServeArgs;

/// Run a serve session on stdin/stdout (PRD §13.11):
///
/// 1. Write the magic preamble.
/// 2. Read the client `Handshake`, negotiate the protocol version, reply.
/// 3. Dispatch each `ClientMessage` frame to a command handler, streaming `Progress` events and
///    a terminal result/`Error`.
/// 4. On EOF / `EPIPE` / `SIGHUP`, flush WAL + tantivy and exit cleanly.
///
/// `--read-only` rejects write operations; `CancelRequest` finishes the in-flight extraction and
/// replies `IndexComplete{ cancelled: true }` (PRD §16.2).
pub fn serve(args: ServeArgs) -> Result<()> {
    let _ = args;
    todo!()
}
