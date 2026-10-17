use sophia_protocol::broker_files::BrokerFileRequest;
use std::time::Instant;

pub(super) struct Pending {
    pub id: u64,
    pub due: Instant,
    value: BrokerFileRequest<'static>,
    label: [u8; 128],
    length: Option<usize>,
}
impl Pending {
    // Called only after request encoding validated the label bound.
    pub fn new(id: u64, request: BrokerFileRequest<'_>, due: Instant) -> Self {
        use BrokerFileRequest::*;
        let mut label = [0; 128];
        let mut length = None;
        let value = match request {
            SurfaceAdmitted { surface, profile } => SurfaceAdmitted { surface, profile },
            CandidateReduced {
                surface,
                disclosure,
                label: text,
                redacted,
                generation,
            } => {
                if let Some(text) = text {
                    label[..text.len()].copy_from_slice(text.as_bytes());
                    length = Some(text.len());
                }
                CandidateReduced {
                    surface,
                    disclosure,
                    label: None,
                    redacted,
                    generation,
                }
            }
            AttentionChanged { surface, attention } => AttentionChanged { surface, attention },
            SurfaceRemoved { surface } => SurfaceRemoved { surface },
            SetDisclosure {
                surface,
                disclosure,
            } => SetDisclosure {
                surface,
                disclosure,
            },
        };
        Self {
            id,
            due,
            value,
            label,
            length,
        }
    }
    pub fn view(&self) -> BrokerFileRequest<'_> {
        if let BrokerFileRequest::CandidateReduced {
            surface,
            disclosure,
            redacted,
            generation,
            ..
        } = self.value
        {
            BrokerFileRequest::CandidateReduced {
                surface,
                disclosure,
                redacted,
                generation,
                label: self.length.map(|n| {
                    std::str::from_utf8(&self.label[..n]).expect("validated request UTF-8")
                }),
            }
        } else {
            self.value
        }
    }
}
