//! Persistent catalog candidates use the same permits, credits and byte leases
//! as ordinary content. The immutable catalog is supplied by Session, not peers.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum CandidateAuthority<'a> {
    Legacy,
    Native(NativeLauncherCandidateContext<'a>),
    Persistent(&'a ShellApplicationCatalog),
}

/// Non-owning catalog identity carried with the actual render bundle. This is
/// not a transient opening, focus lease, or permission to execute an action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PersistentCatalogCandidateBinding {
    pub catalog_generation: u64,
}

impl PersistentCatalogCandidateBinding {
    fn validate_current(
        self,
        begin: &ContentCandidateBegin,
        catalog: &ShellApplicationCatalog,
    ) -> Result<(), ContentCandidateError> {
        if self.catalog_generation == 0
            || self.catalog_generation != catalog.generation
            || begin.grant.connection_epoch != catalog.connection_epoch
        {
            return Err(ContentCandidateError::Stale);
        }
        if begin.surface_count != 1
            || begin.placement_count == 0
            || catalog.entries.len() > SOPHIA_SHELL_MAX_APPLICATIONS
        {
            return Err(ContentCandidateError::Malformed);
        }
        Ok(())
    }
}

pub(super) fn validate_binding(
    binding: Option<PersistentCatalogCandidateBinding>,
    begin: &ContentCandidateBegin,
    surfaces: &[ContentSurface],
    targets: &[ContentTarget],
    authority: CandidateAuthority<'_>,
) -> Result<(), ContentCandidateError> {
    match (binding, authority) {
        (None, CandidateAuthority::Legacy | CandidateAuthority::Native(_)) => Ok(()),
        (Some(binding), CandidateAuthority::Persistent(catalog)) => {
            binding.validate_current(begin, catalog)?;
            if surfaces.len() != 1 || surfaces[0].role != 1 {
                return Err(ContentCandidateError::Malformed);
            }
            for target in targets {
                let slot = u16::try_from(target.action_id)
                    .map_err(|_| ContentCandidateError::Malformed)?;
                let mut entries = catalog.entries.iter().filter(|entry| entry.slot == slot);
                if target.surface_index != 0
                    || target.action_kind != 3
                    || !entries.next().is_some_and(|entry| entry.available)
                    || entries.next().is_some()
                {
                    return Err(ContentCandidateError::Malformed);
                }
            }
            Ok(())
        }
        _ => Err(ContentCandidateError::Stale),
    }
}

impl ContentCandidateStore {
    pub fn begin_persistent_catalog(
        &mut self,
        transaction: TransactionId,
        begin: CatalogCandidateBegin,
        current: &ShellApplicationCatalog,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if self.profile != ContentStoreProfile::PersistentCatalog {
            return Err(ContentCandidateError::Malformed);
        }
        let binding = PersistentCatalogCandidateBinding {
            catalog_generation: begin.catalog_generation,
        };
        let validation = if self.limits.max_candidate_bytes < 48 {
            Err(ContentCandidateError::Budget)
        } else {
            binding.validate_current(&begin.content, current)
        };
        // Even a refused Begin settles the exact pacing permit's response debt.
        match validation {
            Ok(()) => self.begin_inner(transaction, begin.content, None, Some(binding), None, now),
            Err(error) => {
                self.begin_inner(transaction, begin.content, None, None, Some(error), now)
            }
        }
    }

    pub fn chunk_persistent_catalog(
        &mut self,
        transaction: TransactionId,
        chunk: ContentCandidateChunk,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if self.profile != ContentStoreProfile::PersistentCatalog {
            return Err(ContentCandidateError::Malformed);
        }
        self.chunk_inner(transaction, chunk, now)
    }

    pub fn end_persistent_catalog(
        &mut self,
        transaction: TransactionId,
        end: ContentCandidateEnd,
        context: ContentCandidateContext<'_>,
        current: &ShellApplicationCatalog,
        resources: &ContentResourceStore,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if self.profile != ContentStoreProfile::PersistentCatalog {
            return Err(ContentCandidateError::Malformed);
        }
        self.end_inner(
            transaction,
            end,
            context,
            CandidateAuthority::Persistent(current),
            resources,
            now,
        )
    }

    /// Validate current catalog and allocation again before transferring leases
    /// to rendering. Refusal leaves the pending candidate's exact owner intact.
    pub fn begin_persistent_catalog_submission(
        &mut self,
        candidate_generation: u64,
        context: ContentCandidateContext<'_>,
        current: &ShellApplicationCatalog,
        now: u64,
    ) -> Result<ContentRenderBundle, ContentCandidateError> {
        if self.profile != ContentStoreProfile::PersistentCatalog {
            return Err(ContentCandidateError::Malformed);
        }
        self.time(now)?;
        self.expire(now)?;
        let candidate = self
            .pending
            .get(&context.output)
            .ok_or(ContentCandidateError::Stale)?;
        if candidate.begin.candidate_generation != candidate_generation
            || candidate.begin.facts_generation != context.facts_generation
            || candidate.begin.interaction_generation != context.interaction_generation
        {
            return Err(ContentCandidateError::Stale);
        }
        validate_binding(
            candidate.persistent_catalog,
            &candidate.begin,
            &candidate.surfaces,
            &candidate.targets,
            CandidateAuthority::Persistent(current),
        )?;
        validate_surfaces(&candidate.surfaces, context.allocations, context.output)?;
        if context.allocations.iter().any(|allocation| {
            candidate.surfaces.iter().any(|surface| {
                surface.allocation == allocation.allocation && allocation.native_opening.is_some()
            })
        }) {
            return Err(ContentCandidateError::AllocationLost);
        }
        self.begin_submission_inner(context.output, candidate_generation, now)
    }
}
