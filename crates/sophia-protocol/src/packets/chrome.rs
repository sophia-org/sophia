use crate::ids::{IconTokenId, SurfaceId};

/// Longest label any authority may emit, and the longest Engine will accept.
///
/// One bound shared by the authority that reduces, the broker that rules, and the
/// engine that stores. Three copies of a limit is three chances for a label to be
/// valid at one hop and rejected at the next.
// Sanitized presentation values are shared with descriptor clients through
// the SDK. They carry no application identity or disclosure authority.
pub use sophia_shell_protocol::shell::metadata::{
    AttentionState, DisplayLabel, MAX_CHROME_LABEL_LEN, TrustLevel,
};

/// How much of a surface's own identity an authority may put in a label.
///
/// Deliberately three values. This is a security boundary expressed as a
/// vocabulary, so every value has to earn its place: `None` because a surface the
/// broker has not ruled on must disclose nothing, `ClassOnly` because a taskbar can
/// group by application without learning what a document is called, and `Full`
/// because a window switcher is useless if it cannot tell two documents apart.
///
/// A fourth value should be argued for, not added. Widening a server-to-client enum
/// is pre-freeze-only work, and this vocabulary is easier to keep small than to
/// shrink later.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum MetadataDisclosure {
    /// No label at all. The default for any surface without a rule.
    #[default]
    None,
    /// The application class only. Never the instance, never a window title.
    ClassOnly,
    /// The window's own title, bounded and validated.
    Full,
}

impl MetadataDisclosure {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ClassOnly => "class_only",
            Self::Full => "full",
        }
    }

    /// Whether this level permits any label text at all.
    pub const fn discloses_text(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// What the metadata broker publishes about one surface.
///
/// The broker decides these because they are cross-authority facts. Trust follows
/// from the namespace an authority admitted a client into, and icon tokens are
/// drawn from one space shared by every authority, so an authority deciding either
/// alone would let two authorities disagree about what a user is looking at.
///
/// The rule travels to the authority; the authority's raw metadata never travels
/// back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataDisclosureRule {
    pub surface: SurfaceId,
    pub disclosure: MetadataDisclosure,
    pub trust_level: TrustLevel,
    pub icon: Option<IconTokenId>,
    pub generation: u64,
}

/// What an authority emits after applying a rule to metadata it already holds.
///
/// This is the only metadata that crosses an authority boundary. It carries a
/// finished label or no label, never the property bytes it was reduced from, and
/// never the class, instance, PID, or path that produced it.
///
/// `disclosure` is echoed so a receiver can tell "this surface has no title" from
/// "this surface was not permitted to tell you its title" without inferring it from
/// an absent field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReducedMetadataCandidate {
    pub surface: SurfaceId,
    pub label: Option<DisplayLabel>,
    pub disclosure: MetadataDisclosure,
    pub generation: u64,
}

/// What the metadata broker sends Engine about one surface.
///
/// A boundary packet, so it lives with the other packets rather than inside the
/// crate that happens to receive it. The broker produces it and Engine consumes it;
/// putting it in Engine would make the producer depend on its own consumer.
///
/// The label arrives as plain text plus a redaction bit rather than a `DisplayLabel`
/// because Engine validates before it trusts: `ChromeDescriptorTable` is where the
/// bound and the control-character rule are enforced, and a pre-built `DisplayLabel`
/// would imply that check already happened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SanitizedChromeMetadata {
    pub surface: SurfaceId,
    pub label: Option<String>,
    pub label_redacted: bool,
    pub icon: Option<IconTokenId>,
    pub trust_level: TrustLevel,
    pub attention: AttentionState,
    pub generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChromeDescriptor {
    pub surface: SurfaceId,
    pub label: Option<DisplayLabel>,
    pub icon: Option<IconTokenId>,
    pub trust_level: TrustLevel,
    pub attention: AttentionState,
    pub generation: u64,
}

pub const SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerKind {
    Portal,
    Metadata,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerHealthState {
    Starting,
    Ready,
    Degraded,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrokerHealthError {
    MessageTooLong { len: usize, max: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrokerHealthPacket {
    pub broker: BrokerKind,
    pub state: BrokerHealthState,
    pub generation: u64,
    pub message: Option<String>,
}

impl BrokerHealthPacket {
    pub fn new(
        broker: BrokerKind,
        state: BrokerHealthState,
        generation: u64,
        message: Option<String>,
    ) -> Result<Self, BrokerHealthError> {
        let packet = Self {
            broker,
            state,
            generation,
            message,
        };
        packet.validate()?;
        Ok(packet)
    }

    pub fn validate(&self) -> Result<(), BrokerHealthError> {
        if let Some(message) = &self.message
            && message.len() > SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN
        {
            return Err(BrokerHealthError::MessageTooLong {
                len: message.len(),
                max: SOPHIA_BROKER_HEALTH_MAX_MESSAGE_LEN,
            });
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChromeActionRequest {
    pub surface: SurfaceId,
    pub generation: u64,
    pub kind: ChromeActionKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChromeActionKind {
    CloseSurfaceRequested,
}
