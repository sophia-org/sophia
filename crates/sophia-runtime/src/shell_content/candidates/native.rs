use super::*;

/// Session supplies the exact catalog already published to this connection and
/// its latest issued model revision. These are not values selected by the peer.
#[derive(Clone, Copy)]
pub struct NativeLauncherCandidateContext<'a> {
    pub opening: NativeLauncherOpening,
    pub state_revision: u64,
    pub catalog: &'a ShellApplicationCatalog,
}

/// Bounded, non-owning row metadata carried by the actual candidate and renderer
/// bundle. It contains no resource lease and does not confer focus before Present.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeLauncherCandidateBinding {
    pub opening: u64,
    pub catalog_generation: u64,
    pub state_revision: u64,
    pub selected: u16,
    row_count: u16,
    rows: [u16; SOPHIA_SHELL_MAX_LAUNCHER_ROWS],
}

impl NativeLauncherCandidateBinding {
    pub fn rows(&self) -> &[u16] {
        &self.rows[..usize::from(self.row_count)]
    }

    fn from_begin(v: &NativeLauncherCandidateBegin) -> Result<Self, ContentCandidateError> {
        if v.opening == 0
            || v.catalog_generation == 0
            || v.state_revision == 0
            || v.rows.len() > SOPHIA_SHELL_MAX_LAUNCHER_ROWS
            || v.content.surface_count != 1
            || v.content.placement_count == 0
            || v.content.target_count as usize != v.rows.len()
            || !v.rows.iter().enumerate().all(|(i, slot)| {
                *slot > 0
                    && usize::from(*slot) <= SOPHIA_SHELL_MAX_APPLICATIONS
                    && !v.rows[..i].contains(slot)
            })
            || if v.rows.is_empty() {
                v.selected != 0
            } else {
                !v.rows.contains(&v.selected)
            }
        {
            return Err(ContentCandidateError::Malformed);
        }
        let mut rows = [0; SOPHIA_SHELL_MAX_LAUNCHER_ROWS];
        rows[..v.rows.len()].copy_from_slice(&v.rows);
        Ok(Self {
            opening: v.opening,
            catalog_generation: v.catalog_generation,
            state_revision: v.state_revision,
            selected: v.selected,
            row_count: v.rows.len() as u16,
            rows,
        })
    }

    fn validate_current(
        self,
        begin: &ContentCandidateBegin,
        current: NativeLauncherCandidateContext<'_>,
    ) -> Result<(), ContentCandidateError> {
        if begin.grant != current.opening.grant
            || begin.output != current.opening.output
            || self.opening != current.opening.opening
            || current.opening.state_revision != 1
            || self.catalog_generation != current.opening.catalog_generation
            || self.catalog_generation != current.catalog.generation
            || begin.grant.connection_epoch != current.catalog.connection_epoch
            || self.state_revision != current.state_revision
        {
            return Err(ContentCandidateError::Stale);
        }
        if current.catalog.entries.len() > SOPHIA_SHELL_MAX_APPLICATIONS
            || !self.rows().iter().all(|slot| {
                let mut entries = current.catalog.entries.iter().filter(|e| e.slot == *slot);
                entries.next().is_some_and(|entry| entry.available) && entries.next().is_none()
            })
        {
            return Err(ContentCandidateError::Malformed);
        }
        Ok(())
    }
}

impl ContentCandidateStore {
    pub fn begin_native_launcher(
        &mut self,
        transaction: TransactionId,
        begin: NativeLauncherCandidateBegin,
        current: NativeLauncherCandidateContext<'_>,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if self.profile != ContentStoreProfile::NativeLauncher {
            return Err(ContentCandidateError::Malformed);
        }
        let binding = NativeLauncherCandidateBinding::from_begin(&begin).and_then(|binding| {
            if 40 + 108 + 2 * binding.rows().len() > self.limits.max_candidate_bytes as usize {
                return Err(ContentCandidateError::Budget);
            }
            binding.validate_current(&begin.content, current)?;
            Ok(binding)
        });
        // The same permit and response-credit owner settles a refused native
        // Begin. A validation failure must not silently consume a peer request.
        match binding {
            Ok(binding) => self.begin_inner(transaction, begin.content, Some(binding), None, now),
            Err(error) => self.begin_inner(transaction, begin.content, None, Some(error), now),
        }
    }

    pub fn chunk_native_launcher(
        &mut self,
        transaction: TransactionId,
        chunk: ContentCandidateChunk,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if self.profile != ContentStoreProfile::NativeLauncher {
            return Err(ContentCandidateError::Malformed);
        }
        self.chunk_inner(transaction, chunk, now)
    }

    pub fn end_native_launcher(
        &mut self,
        transaction: TransactionId,
        end: ContentCandidateEnd,
        context: ContentCandidateContext<'_>,
        current: NativeLauncherCandidateContext<'_>,
        resources: &ContentResourceStore,
        now: u64,
    ) -> Result<(), ContentCandidateError> {
        if self.profile != ContentStoreProfile::NativeLauncher {
            return Err(ContentCandidateError::Malformed);
        }
        self.end_inner(transaction, end, context, Some(current), resources, now)
    }

    /// Recheck live opening/catalog/geometry immediately before transferring the
    /// actual candidate to rendering. A refusal keeps the pending owner for its
    /// existing explicit cancellation/timeout path; no renderer owner is minted.
    pub fn begin_native_launcher_submission(
        &mut self,
        candidate_generation: u64,
        context: ContentCandidateContext<'_>,
        current: NativeLauncherCandidateContext<'_>,
        now: u64,
    ) -> Result<ContentRenderBundle, ContentCandidateError> {
        if self.profile != ContentStoreProfile::NativeLauncher {
            return Err(ContentCandidateError::Malformed);
        }
        self.check_grant(current.opening.grant)?;
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
            candidate.native_launcher,
            &candidate.begin,
            &candidate.surfaces,
            &candidate.targets,
            context.allocations,
            Some(current),
        )?;
        validate_surfaces(&candidate.surfaces, context.allocations, context.output)?;
        self.begin_submission_inner(context.output, candidate_generation, now)
    }
}

pub(super) fn validate_binding(
    binding: Option<NativeLauncherCandidateBinding>,
    begin: &ContentCandidateBegin,
    surfaces: &[ContentSurface],
    targets: &[ContentTarget],
    allocations: &[ContentAllocationSnapshot],
    current: Option<NativeLauncherCandidateContext<'_>>,
) -> Result<(), ContentCandidateError> {
    match (binding, current) {
        (None, None) => {
            if surfaces.iter().any(|s| {
                allocation(allocations, s.allocation).is_ok_and(|a| a.native_opening.is_some())
            }) {
                return Err(ContentCandidateError::AllocationLost);
            }
            Ok(())
        }
        (Some(binding), Some(current)) => {
            binding.validate_current(begin, current)?;
            if surfaces.len() != 1 || targets.len() != binding.rows().len() {
                return Err(ContentCandidateError::Malformed);
            }
            let surface = &surfaces[0];
            let actual = allocation(allocations, surface.allocation)?;
            if actual.native_opening != Some(binding.opening) || surface.role != 3 {
                return Err(ContentCandidateError::AllocationLost);
            }
            if !targets.iter().zip(binding.rows()).all(|(target, slot)| {
                target.surface_index == 0
                    && target.action_kind == 2
                    && target.action_id == u64::from(*slot)
            }) {
                return Err(ContentCandidateError::Malformed);
            }
            Ok(())
        }
        _ => Err(ContentCandidateError::Stale),
    }
}
