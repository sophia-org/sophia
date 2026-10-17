use super::{header, invalid, put};
use crate::{AttentionState, BinaryCodecError, MetadataDisclosure, NamespaceProfile, SurfaceId};

/// Reduced authority-neutral inputs. Labels borrow the owner's pending request;
/// encoding copies at most 128 bytes into one fixed record. No raw identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerFileRequest<'a> {
    SurfaceAdmitted {
        surface: SurfaceId,
        profile: NamespaceProfile,
    },
    CandidateReduced {
        surface: SurfaceId,
        disclosure: MetadataDisclosure,
        label: Option<&'a str>,
        redacted: bool,
        generation: u64,
    },
    AttentionChanged {
        surface: SurfaceId,
        attention: AttentionState,
    },
    SurfaceRemoved {
        surface: SurfaceId,
    },
    SetDisclosure {
        surface: SurfaceId,
        disclosure: MetadataDisclosure,
    },
}
impl BrokerFileRequest<'_> {
    pub const fn kind(self) -> u16 {
        match self {
            Self::SurfaceAdmitted { .. } => 1,
            Self::CandidateReduced { .. } => 2,
            Self::AttentionChanged { .. } => 3,
            Self::SurfaceRemoved { .. } => 4,
            Self::SetDisclosure { .. } => 5,
        }
    }
    pub const fn surface(self) -> SurfaceId {
        match self {
            Self::SurfaceAdmitted { surface, .. }
            | Self::CandidateReduced { surface, .. }
            | Self::AttentionChanged { surface, .. }
            | Self::SurfaceRemoved { surface }
            | Self::SetDisclosure { surface, .. } => surface,
        }
    }
    /// Raw zero surface/generation values are preserved for reducer rejections.
    /// The owner enforces increasing request IDs across connection epochs.
    pub fn encode(
        self,
        epoch: u64,
        sequence: u64,
        request: u64,
    ) -> Result<[u8; 200], BinaryCodecError> {
        if request == 0 {
            return Err(invalid());
        }
        let mut bytes = header::<200>(32, epoch, 0, sequence)?;
        put(&mut bytes, 32, request, 8);
        put(&mut bytes, 40, u64::from(self.kind()), 2);
        put(&mut bytes, 48, u64::from(self.surface().index()), 4);
        put(&mut bytes, 52, u64::from(self.surface().generation()), 4);
        let value = match self {
            Self::SurfaceAdmitted { profile, .. } => match profile {
                NamespaceProfile::ClassicShared => 1,
                NamespaceProfile::Confined => 2,
            },
            Self::CandidateReduced {
                disclosure,
                label,
                redacted,
                generation,
                ..
            } => {
                if redacted && label.is_none() {
                    return Err(invalid());
                }
                if let Some(label) = label {
                    if label.len() > 128 {
                        return Err(invalid());
                    }
                    bytes[59] = 1;
                    put(&mut bytes, 60, label.len() as u64, 2);
                    bytes[72..72 + label.len()].copy_from_slice(label.as_bytes());
                }
                bytes[58] = u8::from(redacted);
                put(&mut bytes, 64, generation, 8);
                disclosure_value(disclosure)
            }
            Self::AttentionChanged { attention, .. } => match attention {
                AttentionState::None => 1,
                AttentionState::Notice => 2,
                AttentionState::Critical => 3,
            },
            Self::SurfaceRemoved { .. } => 0,
            Self::SetDisclosure { disclosure, .. } => disclosure_value(disclosure),
        };
        put(&mut bytes, 56, value, 2);
        Ok(bytes)
    }
}
fn disclosure_value(value: MetadataDisclosure) -> u64 {
    match value {
        MetadataDisclosure::None => 1,
        MetadataDisclosure::ClassOnly => 2,
        MetadataDisclosure::Full => 3,
    }
}
