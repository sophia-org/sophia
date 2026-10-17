//! Translation at the independent SDK boundary. The reducer owns policy; these
//! adapters preserve the complete result and borrow its labels for one encoding.
use crate::{MetadataBrokerCommand as Command, MetadataBrokerRejection as Rejection};
use sophia_broker_protocol::{Invalid, Request, RequestKind, ResponseRow, RowKind};
use sophia_protocol::{
    AttentionState, MetadataDisclosure, NamespaceProfile, SurfaceId, TrustLevel,
    broker_files::BrokerFileRequest,
};

pub fn request(value: Request<'_>) -> Result<BrokerFileRequest<'_>, Invalid> {
    let surface = SurfaceId::new(value.surface_index, value.surface_generation);
    Ok(match value.kind {
        RequestKind::SurfaceAdmitted => BrokerFileRequest::SurfaceAdmitted {
            surface,
            profile: match value.value {
                1 => NamespaceProfile::ClassicShared,
                2 => NamespaceProfile::Confined,
                _ => return Err(Invalid),
            },
        },
        RequestKind::CandidateReduced => BrokerFileRequest::CandidateReduced {
            surface,
            disclosure: disclosure(value.value)?,
            label: value.label,
            redacted: value.redacted,
            generation: value.candidate_generation,
        },
        RequestKind::AttentionChanged => BrokerFileRequest::AttentionChanged {
            surface,
            attention: match value.value {
                1 => AttentionState::None,
                2 => AttentionState::Notice,
                3 => AttentionState::Critical,
                _ => return Err(Invalid),
            },
        },
        RequestKind::SurfaceRemoved => BrokerFileRequest::SurfaceRemoved { surface },
        RequestKind::SetDisclosure => BrokerFileRequest::SetDisclosure {
            surface,
            disclosure: disclosure(value.value)?,
        },
    })
}

pub fn response_rows(
    result: &Result<Vec<Command>, Rejection>,
) -> Result<([ResponseRow<'_>; 2], usize), Invalid> {
    let mut rows = [ResponseRow::default(); 2];
    let commands = match result {
        Ok(commands) => commands,
        Err(reason) => {
            rows[0].kind = RowKind::Rejected;
            rows[0].value = match reason {
                Rejection::UnknownSurface => 1,
                Rejection::StaleGeneration => 2,
                Rejection::CapacityExhausted => 3,
                Rejection::DisclosureExceeded => 4,
            };
            return Ok((rows, 1));
        }
    };
    if commands.len() > 2 {
        return Err(Invalid);
    }
    for (row, command) in rows.iter_mut().zip(commands) {
        let surface = match command {
            Command::PublishRule(rule) => {
                row.kind = RowKind::PublishRule;
                row.disclosure = match rule.disclosure {
                    MetadataDisclosure::None => 1,
                    MetadataDisclosure::ClassOnly => 2,
                    MetadataDisclosure::Full => 3,
                };
                row.trust = trust(rule.trust_level);
                row.icon = rule.icon.map(|icon| icon.raw());
                row.generation = rule.generation;
                rule.surface
            }
            Command::EmitDescriptor { descriptor, action } => {
                row.kind = RowKind::EmitDescriptor;
                row.trust = trust(descriptor.trust_level);
                row.attention = match descriptor.attention {
                    AttentionState::None => 1,
                    AttentionState::Notice => 2,
                    AttentionState::Critical => 3,
                };
                row.icon = descriptor.icon.map(|icon| icon.raw());
                row.label = descriptor.label.as_deref();
                row.redacted = descriptor.label_redacted;
                row.generation = descriptor.generation;
                row.grant_token = action.token;
                row.grant_revocation_epoch = action.revocation_epoch;
                row.grant_target_generation = action.target_generation;
                descriptor.surface
            }
            Command::RetireSurface { surface } => {
                row.kind = RowKind::RetireSurface;
                *surface
            }
        };
        row.surface_index = surface.index();
        row.surface_generation = surface.generation();
    }
    Ok((rows, commands.len().max(1)))
}
fn disclosure(value: u16) -> Result<MetadataDisclosure, Invalid> {
    match value {
        1 => Ok(MetadataDisclosure::None),
        2 => Ok(MetadataDisclosure::ClassOnly),
        3 => Ok(MetadataDisclosure::Full),
        _ => Err(Invalid),
    }
}
fn trust(value: TrustLevel) -> u16 {
    match value {
        TrustLevel::Unknown => 1,
        TrustLevel::Trusted => 2,
        TrustLevel::Untrusted => 3,
        TrustLevel::Isolated => 4,
    }
}
