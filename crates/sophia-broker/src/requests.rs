use sophia_protocol::{
    DisplayLabel, MetadataDisclosure, ReducedMetadataCandidate, broker_files::BrokerFileRequest,
};

use crate::{MetadataBroker, MetadataBrokerCommand, MetadataBrokerEvent, MetadataBrokerRejection};

impl MetadataBroker {
    /// Reduce a file request without discarding any commands. The transport must
    /// submit the entire result as one response, including disclosure's rule and
    /// descriptor pair. Request identity and retry custody belong to the session.
    pub fn file_request(
        &mut self,
        request: BrokerFileRequest<'_>,
        default_class_only: bool,
    ) -> Result<Vec<MetadataBrokerCommand>, MetadataBrokerRejection> {
        match request {
            BrokerFileRequest::SurfaceAdmitted { surface, profile } => {
                let admitted =
                    self.update(MetadataBrokerEvent::SurfaceAdmitted { surface, profile })?;
                if default_class_only {
                    self.set_disclosure(surface, MetadataDisclosure::ClassOnly)
                } else {
                    Ok(admitted)
                }
            }
            BrokerFileRequest::CandidateReduced {
                surface,
                disclosure,
                label,
                redacted,
                generation,
            } => self.update(MetadataBrokerEvent::CandidateReduced(
                ReducedMetadataCandidate {
                    surface,
                    disclosure,
                    label: label.map(|text| DisplayLabel {
                        text: text.to_owned(),
                        redacted,
                    }),
                    generation,
                },
            )),
            BrokerFileRequest::AttentionChanged { surface, attention } => {
                self.update(MetadataBrokerEvent::AttentionChanged { surface, attention })
            }
            BrokerFileRequest::SurfaceRemoved { surface } => {
                self.update(MetadataBrokerEvent::SurfaceRemoved { surface })
            }
            BrokerFileRequest::SetDisclosure {
                surface,
                disclosure,
            } => self.set_disclosure(surface, disclosure),
        }
    }
}
