//! Bridges core progress updates to wire `ProgressEvent` frames (PRD §13.7).

use ndex_core::progress::{ProgressKind, ProgressSink, ProgressUpdate};
use ndex_protocol::{ProgressChild, ProgressEvent};

/// Stable phase name for a [`ProgressKind`] (PRD §13.7 progress phases).
pub fn phase_name(kind: ProgressKind) -> &'static str {
    match kind {
        ProgressKind::Walk => "walk",
        ProgressKind::Diff => "diff",
        ProgressKind::Extract => "extract",
        ProgressKind::Embed => "embed",
        ProgressKind::Fts => "fts",
        ProgressKind::Meta => "meta",
    }
}

/// Map a core [`ProgressUpdate`] to the wire [`ProgressEvent`].
pub fn to_progress_event(update: &ProgressUpdate) -> ProgressEvent {
    ProgressEvent {
        phase: phase_name(update.kind).to_string(),
        current: update.current,
        total: update.total,
        message: update.message.clone(),
        children: update
            .children
            .iter()
            .map(|c| ProgressChild {
                label: c.label.clone(),
                current: c.current,
                total: c.total,
                message: c.message.clone(),
            })
            .collect(),
    }
}

/// A [`ProgressSink`] that frames `Progress` messages back to the connected client.
pub struct WireProgressSink {
    // TODO(skeleton): a shared handle to the session's FrameWriter (Mutex<FrameWriter<…>>).
}

impl ProgressSink for WireProgressSink {
    fn emit(&self, update: &ProgressUpdate) {
        let _event = to_progress_event(update);
        // TODO(skeleton): codec::to_vec_named(&ServerMessage::Progress(event)) → write_frame.
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndex_core::progress::ProgressChildUpdate;

    #[test]
    fn maps_update_to_event() {
        let update = ProgressUpdate {
            kind: ProgressKind::Extract,
            current: 100,
            total: Some(1000),
            message: Some("processing".into()),
            children: vec![ProgressChildUpdate {
                label: "worker-3".into(),
                current: 33,
                total: Some(250),
                message: None,
            }],
        };
        let event = to_progress_event(&update);
        assert_eq!(event.phase, "extract");
        assert_eq!(event.current, 100);
        assert_eq!(event.children.len(), 1);
        assert_eq!(event.children[0].label, "worker-3");
    }
}
