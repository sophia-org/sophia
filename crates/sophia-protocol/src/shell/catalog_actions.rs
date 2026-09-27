//! Wire-neutral typed model for the revision-8 persistent catalog actions.
//!
//! Codec support does not grant authority. Identities extend the catalog
//! transaction before ApplicationsEnd; candidates bind that exact catalog
//! generation, and activation echoes an issued action.
use crate::InvalidRecord;
use crate::{
    ContentAction, ContentCandidate, ContentCandidateBegin, ContentCandidateChunk,
    ContentCandidateEnd, ShellContentRecord,
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
/// One whole catalog candidate: the underlying content candidate plus the
/// catalog generation it presents. Transports that carry it in parts
/// reassemble it; the candidate owner receives it as
/// [`CatalogCandidateBegin`], a [`ContentCandidateChunk`] and the shared r5
/// `CandidateEnd`, exactly as [`ContentCandidate::parts`] documents for the
/// base profile.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogContentCandidate {
    pub candidate: ContentCandidate,
    pub catalog_generation: u64,
}
impl CatalogContentCandidate {
    /// The candidate as the owner's `CandidateBegin`, `CandidateChunk` (both
    /// persistent-catalog records) and the shared r5 `CandidateEnd`.
    pub fn parts(
        &self,
    ) -> (
        ShellCatalogActionRecord,
        ShellCatalogActionRecord,
        ShellContentRecord,
    ) {
        let candidate = &self.candidate;
        let (surfaces, placements, targets) = (
            candidate.surfaces.len() as u32,
            candidate.placements.len() as u32,
            candidate.targets.len() as u32,
        );
        let begin = CatalogCandidateBegin {
            content: ContentCandidateBegin {
                grant: candidate.grant,
                candidate_generation: candidate.candidate_generation,
                output: candidate.output,
                facts_generation: candidate.facts_generation,
                pacing_permit: candidate.pacing_permit,
                interaction_generation: candidate.interaction_generation,
                surface_count: surfaces,
                placement_count: placements,
                target_count: targets,
            },
            catalog_generation: self.catalog_generation,
        };
        let chunk = ContentCandidateChunk {
            grant: candidate.grant,
            candidate_generation: candidate.candidate_generation,
            chunk_ordinal: 0,
            surfaces: candidate.surfaces.clone(),
            placements: candidate.placements.clone(),
            targets: candidate.targets.clone(),
        };
        let end = ShellContentRecord::CandidateEnd(ContentCandidateEnd {
            grant: candidate.grant,
            candidate_generation: candidate.candidate_generation,
            surface_count: surfaces,
            placement_count: placements,
            target_count: targets,
        });
        (
            ShellCatalogActionRecord::CandidateBegin(begin),
            ShellCatalogActionRecord::CandidateChunk(chunk),
            end,
        )
    }
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
fn require(ok: bool, field: &'static str) -> Result<(), InvalidRecord> {
    if ok {
        Ok(())
    } else {
        Err(InvalidRecord(field))
    }
}
fn activation(v: &CatalogActivation) -> Result<(), InvalidRecord> {
    crate::shell::content::validation::validate(&ShellContentRecord::Action(v.action.clone()))?;
    require(
        v.catalog_generation > 0
            && v.action.kind == 1
            && v.action.reason == 0
            && (1..=4096).contains(&v.action.action_id),
        "persistent catalog activation",
    )
}
pub(crate) fn validate(record: &ShellCatalogActionRecord) -> Result<(), InvalidRecord> {
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
            crate::shell::content::validation::validate(&ShellContentRecord::CandidateBegin(
                v.content.clone(),
            ))?;
            require(v.catalog_generation > 0, "persistent candidate catalog")
        }
        ShellCatalogActionRecord::CandidateChunk(v) => {
            crate::shell::content::validation::validate_catalog_candidate_chunk(v)
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
