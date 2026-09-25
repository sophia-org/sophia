use super::{OverviewInputPresentation, OverviewProjection, PolicyOverviewPublication};
use sophia_protocol::*;

/// Session-owned authority for one exact WM publication and shell connection.
/// Socket transport and physical presentation remain the caller's responsibility.
pub struct OverviewAuthority {
    pub publication: PolicyOverviewPublication,
    pub catalog: ShellOverviewCatalog,
    request: Option<ShellOverviewRequest>,
    staged: Option<(ShellOverviewCandidate, Option<OverviewProjection>)>,
    presented: Option<(ShellOverviewCandidate, OverviewInputPresentation)>,
    last_candidate: u64,
    last_request: u64,
    presentation_epoch: u64,
    output: Option<(OutputId, u64)>,
    revoked: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverviewSelection {
    pub output: OutputId,
    pub output_generation: u64,
    pub workspace: u64,
    pub target: Option<SurfaceId>,
}

impl OverviewAuthority {
    pub fn new(
        publication: PolicyOverviewPublication,
        shell_epoch: u64,
        catalog_generation: u64,
    ) -> Result<Self, &'static str> {
        validate_wm_overview(&publication.workspaces)
            .map_err(|_| "invalid overview publication")?;
        if publication.connection_epoch.is_none() || shell_epoch == 0 || catalog_generation == 0 {
            return Err("invalid overview authority epoch");
        }
        let mut next_window = 1u16;
        let mut workspaces = Vec::new();
        for (index, row) in publication.workspaces.iter().enumerate() {
            let mut windows = Vec::new();
            let mut focused = 0;
            for placement in &row.placements {
                let slot = next_window;
                next_window = next_window
                    .checked_add(1)
                    .ok_or("overview window slots exhausted")?;
                windows.push(slot);
                if row.focus == Some(placement.surface) {
                    focused = slot;
                }
            }
            workspaces.push(ShellOverviewWorkspace {
                slot: (index + 1) as u16,
                output: row.output,
                active: row.active,
                focused,
                windows,
            });
        }
        let catalog = ShellOverviewCatalog {
            connection_epoch: shell_epoch,
            generation: catalog_generation,
            workspaces,
        };
        validate_shell_overview_catalog(&catalog).map_err(|_| "invalid overview catalog")?;
        Ok(Self {
            publication,
            catalog,
            request: None,
            staged: None,
            presented: None,
            last_candidate: 0,
            last_request: 0,
            presentation_epoch: 0,
            output: None,
            revoked: false,
        })
    }

    pub fn busy(&self) -> bool {
        self.request.is_some() || self.staged.is_some()
    }

    pub fn input(&self) -> Option<OverviewInputPresentation> {
        if self.revoked || self.staged.as_ref().is_some_and(|(c, _)| !c.visible) {
            return None;
        }
        self.presented.as_ref().map(|(_, input)| input.clone())
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
        self.presented = None;
        self.staged = None;
        self.request = None;
    }

    pub fn request(
        &mut self,
        operation: ShellOverviewOperation,
        output: OutputId,
        output_generation: u64,
        request_generation: u64,
        pick: Option<(u16, u16)>,
    ) -> Result<ShellOverviewRequest, &'static str> {
        if self.revoked
            || self.busy()
            || request_generation <= self.last_request
            || output_generation == 0
            || !self
                .catalog
                .workspaces
                .iter()
                .any(|row| row.output == output)
            || operation != ShellOverviewOperation::Toggle && self.presented.is_none()
            || self.presented.is_some() && self.output != Some((output, output_generation))
            || (operation == ShellOverviewOperation::Pick) != pick.is_some()
        {
            return Err("stale overview input request");
        }
        let (workspace, window) = pick.unwrap_or((0, 0));
        let request = ShellOverviewRequest {
            connection_epoch: self.catalog.connection_epoch,
            catalog_generation: self.catalog.generation,
            request_generation,
            presentation_epoch: self.presentation_epoch,
            output,
            operation,
            workspace,
            window,
        };
        self.output = Some((output, output_generation));
        self.last_request = request_generation;
        self.request = Some(request);
        Ok(request)
    }

    pub fn stage(
        &mut self,
        candidate: ShellOverviewCandidate,
        projection: Option<OverviewProjection>,
    ) -> Result<(), &'static str> {
        let request = self.request.ok_or("unsolicited overview reply")?;
        if self.revoked
            || self.staged.is_some()
            || candidate.connection_epoch != request.connection_epoch
            || candidate.catalog_generation != request.catalog_generation
            || candidate.request_generation != request.request_generation
            || candidate.candidate_generation <= self.last_candidate
            || candidate.visible && candidate.activate
            || candidate.visible != projection.is_some()
        {
            return Err("stale overview reply");
        }
        let index = self
            .catalog
            .workspaces
            .iter()
            .position(|row| row.slot == candidate.workspace)
            .ok_or("unknown overview workspace")?;
        let row = &self.catalog.workspaces[index];
        if row.output != request.output
            || candidate.window != 0 && !row.windows.contains(&candidate.window)
        {
            return Err("overview selection escaped catalog");
        }
        let activation = matches!(
            request.operation,
            ShellOverviewOperation::Accept | ShellOverviewOperation::Pick
        );
        let visible = match request.operation {
            ShellOverviewOperation::Toggle => self.presented.is_none(),
            ShellOverviewOperation::Dismiss
            | ShellOverviewOperation::Accept
            | ShellOverviewOperation::Pick => false,
            _ => true,
        };
        if candidate.activate != activation || candidate.visible != visible {
            return Err("overview reply changed the requested operation");
        }
        if activation {
            let expected = if request.operation == ShellOverviewOperation::Pick {
                (request.workspace, request.window)
            } else {
                let (shown, _) = self
                    .presented
                    .as_ref()
                    .ok_or("overview has no retired selection")?;
                (shown.workspace, shown.window)
            };
            if expected != (candidate.workspace, candidate.window) {
                return Err("overview activation changed the retired selection");
            }
        }
        if let Some(projection) = &projection {
            if projection.overlay.output != request.output
                || projection.overlay.generation != candidate.candidate_generation
            {
                return Err("overview projection changed identity");
            }
        }
        self.last_candidate = candidate.candidate_generation;
        self.staged = Some((candidate, projection));
        Ok(())
    }

    pub fn staged(&self) -> Option<&ShellOverviewCandidate> {
        self.staged.as_ref().map(|(c, _)| c)
    }

    pub fn retire(&mut self, epoch: u64) -> Result<Option<OverviewSelection>, &'static str> {
        if self.revoked || epoch == 0 {
            return Err("invalid overview retirement");
        }
        let (candidate, projection) = self
            .staged
            .take()
            .ok_or("overview has no staged candidate")?;
        let (output, output_generation) = self.output.unwrap();
        self.request = None;
        self.presentation_epoch = epoch;
        self.presented = projection.map(|projection| {
            (
                candidate,
                OverviewInputPresentation {
                    output,
                    epoch,
                    catalog: self.catalog.generation,
                    targets: projection.targets,
                },
            )
        });
        if !candidate.activate {
            return Ok(None);
        }
        let index = self
            .catalog
            .workspaces
            .iter()
            .position(|row| row.slot == candidate.workspace)
            .unwrap();
        let row = &self.catalog.workspaces[index];
        let target = row
            .windows
            .iter()
            .position(|slot| *slot == candidate.window)
            .map(|index_in_row| {
                self.publication.workspaces[index].placements[index_in_row].surface
            });
        Ok(Some(OverviewSelection {
            output,
            output_generation,
            workspace: self.publication.workspaces[index].workspace,
            target,
        }))
    }
}
