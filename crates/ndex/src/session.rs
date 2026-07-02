//! A protocol session over a [`Transport`] (PRD §12).

use std::io::{Read, Write};

use ndex_core::error::Result;
use ndex_protocol::{
    ClientMessage, FrameReader, FrameWriter, HandshakeReq, HandshakeResp, ProgressEvent,
    ServerMessage,
};

use crate::transport::Transport;

/// An established session: framed reader/writer over the transport's streams.
pub struct Session {
    reader: FrameReader<Box<dyn Read + Send>>,
    writer: FrameWriter<Box<dyn Write + Send>>,
}

impl Session {
    /// Connect: spawn the transport, scan the magic preamble, send the handshake, and negotiate
    /// the protocol version (PRD §12.2, §12.3). Returns the session and the server's handshake.
    pub fn connect(
        transport: &dyn Transport,
        handshake: HandshakeReq,
    ) -> Result<(Self, HandshakeResp)> {
        // TODO(skeleton): spawn → wrap in Frame{Reader,Writer} → scan_preamble → send/recv handshake.
        let _ = (transport, handshake);
        todo!()
    }

    /// Send a request, forwarding any interleaved `Progress` events to `on_progress`, and return
    /// the terminal response (`SearchResult`, `IndexComplete`, `Error`, …).
    pub fn request(
        &mut self,
        message: &ClientMessage,
        on_progress: &mut dyn FnMut(&ProgressEvent),
    ) -> Result<ServerMessage> {
        let _ = (&mut self.reader, &mut self.writer, message, on_progress);
        todo!()
    }
}
