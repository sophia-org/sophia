//! Revision-8 persistent catalog actions. Codec support does not grant authority.
//! Identities extend the catalog transaction before ApplicationsEnd; candidates
//! bind that exact catalog generation, and activation echoes an issued action.
use crate::ipc::cursor::Cursor;
use crate::ipc::shell_content::{
    fields::{Wire, reserved},
    validation,
};
use crate::{
    ContentAction, ContentCandidateBegin, ContentCandidateChunk, IpcCodecError, IpcMessageKind,
    ShellContentRecord, TransactionId, decode_frame, encode_frame,
};

pub const SOPHIA_SHELL_PERSISTENT_CATALOG_REVISION: u16 = 8;
pub const SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG: u64 = 1 << 12;
pub const SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES: usize = 256;

/// One stable identity per catalog entry, inside its Begin/End transaction.
/// Names are Session-owned (registered:<id> or desktop:<desktop-file-id>).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellCatalogIdentity {
    pub connection_epoch: u64,
    pub catalog_generation: u64,
    pub slot: u16,
    pub identity: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogCandidateBegin {
    pub content: ContentCandidateBegin,
    pub catalog_generation: u64,
}
/// Exact issued pointer action plus the catalog bound to its presented candidate.
/// There is no transient opening, keyboard event or focus lease in this family.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogActivation {
    pub action: ContentAction,
    pub catalog_generation: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogActivationOutcome {
    pub activation: CatalogActivation,
    /// Admitted=1, stale=2, unknown=3, unauthorized=4, capacity=5.
    /// Admission is queue ownership, not application startup.
    pub status: u16,
    pub reason: u16,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShellCatalogActionRecord {
    Identity(ShellCatalogIdentity),
    CandidateBegin(CatalogCandidateBegin),
    CandidateChunk(ContentCandidateChunk),
    Activate(CatalogActivation),
    ActivationOutcome(CatalogActivationOutcome),
}
fn require(ok: bool, field: &'static str) -> Result<(), IpcCodecError> {
    if ok {
        Ok(())
    } else {
        Err(IpcCodecError::InvalidRecord(field))
    }
}
fn activation(v: &CatalogActivation) -> Result<(), IpcCodecError> {
    validation::validate(&ShellContentRecord::Action(v.action.clone()))?;
    require(
        v.catalog_generation > 0
            && v.action.kind == 1
            && v.action.reason == 0
            && (1..=4096).contains(&v.action.action_id),
        "persistent catalog activation",
    )
}
fn validate(record: &ShellCatalogActionRecord) -> Result<(), IpcCodecError> {
    match record {
        ShellCatalogActionRecord::Identity(v) => require(
            v.connection_epoch > 0
                && v.catalog_generation > 0
                && (1..=4096).contains(&v.slot)
                && !v.identity.is_empty()
                && v.identity.len() <= SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES
                && crate::shell_launcher_text_valid(
                    &v.identity,
                    SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES,
                )
                && ["registered:", "desktop:"].iter().any(|prefix| {
                    v.identity
                        .strip_prefix(prefix)
                        .is_some_and(|tail| !tail.is_empty())
                }),
            "persistent catalog identity",
        ),
        ShellCatalogActionRecord::CandidateBegin(v) => {
            validation::validate(&ShellContentRecord::CandidateBegin(v.content.clone()))?;
            require(v.catalog_generation > 0, "persistent candidate catalog")
        }
        ShellCatalogActionRecord::CandidateChunk(v) => {
            validation::validate_catalog_candidate_chunk(v)
        }
        ShellCatalogActionRecord::Activate(v) => activation(v),
        ShellCatalogActionRecord::ActivationOutcome(v) => {
            activation(&v.activation)?;
            require(
                (1..=5).contains(&v.status) && v.reason == 0,
                "persistent catalog outcome",
            )
        }
    }
}
impl Wire for ShellCatalogIdentity {
    fn put(&self, out: &mut Vec<u8>) {
        self.connection_epoch.put(out);
        self.catalog_generation.put(out);
        self.slot.put(out);
        0u16.put(out);
        (self.identity.len() as u16).put(out);
        0u16.put(out);
        out.extend_from_slice(self.identity.as_bytes());
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        let connection_epoch = u64::take(c)?;
        let catalog_generation = u64::take(c)?;
        let slot = u16::take(c)?;
        reserved::<u16>(c)?;
        let length = usize::from(u16::take(c)?);
        reserved::<u16>(c)?;
        require(
            length <= SOPHIA_SHELL_CATALOG_IDENTITY_MAX_BYTES,
            "catalog identity length",
        )?;
        let identity = std::str::from_utf8(c.slice(length)?)
            .map_err(|_| IpcCodecError::InvalidRecord("catalog identity UTF-8"))?
            .to_owned();
        Ok(Self {
            connection_epoch,
            catalog_generation,
            slot,
            identity,
        })
    }
}
impl Wire for CatalogCandidateBegin {
    fn put(&self, out: &mut Vec<u8>) {
        self.content.put(out);
        self.catalog_generation.put(out);
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        Ok(Self {
            content: ContentCandidateBegin::take(c)?,
            catalog_generation: u64::take(c)?,
        })
    }
}
impl Wire for CatalogActivation {
    fn put(&self, out: &mut Vec<u8>) {
        self.action.put(out);
        self.catalog_generation.put(out);
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        Ok(Self {
            action: ContentAction::take(c)?,
            catalog_generation: u64::take(c)?,
        })
    }
}
impl Wire for CatalogActivationOutcome {
    fn put(&self, out: &mut Vec<u8>) {
        self.activation.put(out);
        self.status.put(out);
        self.reason.put(out);
    }
    fn take(c: &mut Cursor<'_>) -> Result<Self, IpcCodecError> {
        Ok(Self {
            activation: CatalogActivation::take(c)?,
            status: u16::take(c)?,
            reason: u16::take(c)?,
        })
    }
}

pub fn encode_shell_catalog_action_frame(
    transaction: TransactionId,
    record: &ShellCatalogActionRecord,
) -> Result<Vec<u8>, IpcCodecError> {
    require(transaction.is_valid(), "catalog action transaction")?;
    validate(record)?;
    let mut payload = Vec::new();
    use IpcMessageKind as K;
    let kind = match record {
        ShellCatalogActionRecord::Identity(v) => {
            v.put(&mut payload);
            K::ShellCatalogIdentity
        }
        ShellCatalogActionRecord::CandidateBegin(v) => {
            v.put(&mut payload);
            K::ShellCatalogCandidateBegin
        }
        ShellCatalogActionRecord::CandidateChunk(v) => {
            v.put(&mut payload);
            K::ShellCatalogCandidateChunk
        }
        ShellCatalogActionRecord::Activate(v) => {
            v.put(&mut payload);
            K::ShellCatalogActivate
        }
        ShellCatalogActionRecord::ActivationOutcome(v) => {
            v.put(&mut payload);
            K::ShellCatalogActivationOutcome
        }
    };
    encode_frame(kind, transaction, &payload)
}
pub fn decode_shell_catalog_action_frame(
    frame: &[u8],
) -> Result<(TransactionId, ShellCatalogActionRecord), IpcCodecError> {
    let (header, payload) = decode_frame(frame)?;
    require(header.transaction.is_valid(), "catalog action transaction")?;
    let mut c = Cursor::new(payload);
    use IpcMessageKind as K;
    let record = match header.message_kind {
        K::ShellCatalogIdentity => {
            ShellCatalogActionRecord::Identity(ShellCatalogIdentity::take(&mut c)?)
        }
        K::ShellCatalogCandidateBegin => {
            ShellCatalogActionRecord::CandidateBegin(CatalogCandidateBegin::take(&mut c)?)
        }
        K::ShellCatalogCandidateChunk => {
            ShellCatalogActionRecord::CandidateChunk(ContentCandidateChunk::take(&mut c)?)
        }
        K::ShellCatalogActivate => {
            ShellCatalogActionRecord::Activate(CatalogActivation::take(&mut c)?)
        }
        K::ShellCatalogActivationOutcome => {
            ShellCatalogActionRecord::ActivationOutcome(CatalogActivationOutcome::take(&mut c)?)
        }
        _ => {
            return Err(IpcCodecError::InvalidRecord(
                "not a persistent catalog record",
            ));
        }
    };
    c.finish()?;
    validate(&record)?;
    Ok((header.transaction, record))
}
