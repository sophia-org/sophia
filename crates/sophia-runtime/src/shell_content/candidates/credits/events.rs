use super::{ContentFrameStreamEvent, ContentFrameStreamRecord};
use crate::ContentCandidateEvent;
use sophia_protocol::{ContentOutputId, ShellContentRecord};
use std::collections::VecDeque;

// Keep per-frame outcomes inline rather than allocate once per queue entry.
#[allow(clippy::large_enum_variant)]
enum Queued {
    Candidate(ContentCandidateEvent),
    Stream(ContentFrameStreamEvent),
}

/// Both typed readers stop at the same FIFO head; neither can skip the other.
#[derive(Default)]
pub(crate) struct CandidateEvents(VecDeque<Queued>);

impl CandidateEvents {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    pub(crate) fn push_back(&mut self, event: ContentCandidateEvent) {
        self.0.push_back(Queued::Candidate(event));
    }

    pub(crate) fn push_stream(&mut self, event: ContentFrameStreamEvent) {
        self.0.push_back(Queued::Stream(event));
    }

    pub(crate) fn front(&self) -> Option<&ContentCandidateEvent> {
        match self.0.front()? {
            Queued::Candidate(event) => Some(event),
            Queued::Stream(_) => None,
        }
    }

    pub(crate) fn back_mut(&mut self) -> Option<&mut ContentCandidateEvent> {
        match self.0.back_mut()? {
            Queued::Candidate(event) => Some(event),
            Queued::Stream(_) => None,
        }
    }

    pub(crate) fn pop_front(&mut self) -> Option<ContentCandidateEvent> {
        self.front()?;
        match self.0.pop_front()? {
            Queued::Candidate(event) => Some(event),
            Queued::Stream(_) => unreachable!("head was a candidate event"),
        }
    }

    pub(crate) fn stream_front(&self) -> Option<&ContentFrameStreamEvent> {
        match self.0.front()? {
            Queued::Stream(event) => Some(event),
            Queued::Candidate(_) => None,
        }
    }

    pub(crate) fn pop_stream(&mut self) -> Option<ContentFrameStreamEvent> {
        self.stream_front()?;
        match self.0.pop_front()? {
            Queued::Stream(event) => Some(event),
            Queued::Candidate(_) => unreachable!("head was a stream event"),
        }
    }

    pub(crate) fn has_output(&self, output: ContentOutputId) -> bool {
        self.0.iter().any(|event| match event {
            Queued::Candidate(event) => match &event.record {
                ShellContentRecord::CandidateOutcome(v) => v.output == output,
                ShellContentRecord::FramePermit(v) => v.output == output,
                _ => false,
            },
            Queued::Stream(event) => match event.record {
                ContentFrameStreamRecord::Opened { key, .. } => key.output == output,
                ContentFrameStreamRecord::Closed(notice) => notice.key.output == output,
            },
        })
    }
}
