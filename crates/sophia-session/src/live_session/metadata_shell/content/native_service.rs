//! Native launcher state borrows the component owner's exact connection.
use super::*;
use sophia_protocol::{ContentGrant, ShellApplicationCatalog};
use sophia_runtime::{ContentCandidateContext, NativeLauncherCandidateContext};

#[path = "native_service/allocation.rs"]
mod allocation;
#[path = "native_service/closing.rs"]
mod closing;
#[path = "native_service/input.rs"]
mod input;
#[path = "native_service/opening.rs"]
mod opening;

type ServiceResult<T> = Result<T, Box<dyn std::error::Error>>;

/// This owns presentation obligations, not a socket, process or resource pool.
/// The component owner must retain it through close/removal and handle errors by
/// retiring this exact connection. Construct a new service after reconnect.
pub struct NativeLauncherContentService {
    grant: ContentGrant,
    content: LiveContentSession,
    opening: Option<sophia_protocol::NativeLauncherOpening>,
    submitted: Option<u64>,
    closing: Option<closing::Closing>,
    open_request: Option<opening::OpenRequest>,
    focus_pending: bool,
    inputs: std::collections::VecDeque<input::PendingInput>,
    input_bytes: usize,
}

impl NativeLauncherContentService {
    pub fn new(transport: &ShellTransportConnection<'_>) -> Result<Self, ShellTransportError> {
        if !transport.supports_native_launcher() {
            return Err(ShellTransportError::MissingCapability);
        }
        Ok(Self {
            grant: transport
                .content_grant()
                .ok_or(ShellTransportError::MissingCapability)?,
            content: LiveContentSession::new(true, true, None),
            opening: None,
            submitted: None,
            closing: None,
            open_request: None,
            focus_pending: false,
            inputs: std::collections::VecDeque::with_capacity(32),
            input_bytes: 0,
        })
    }

    fn validate(
        &self,
        transport: &ShellTransportConnection<'_>,
    ) -> Result<(), ShellTransportError> {
        if !transport.supports_native_launcher() || transport.content_grant() != Some(self.grant) {
            return Err(ShellTransportError::WrongContentGrant);
        }
        Ok(())
    }

    pub const fn grant(&self) -> ContentGrant {
        self.grant
    }

    /// Output facts may precede an opening, through the same content FIFO.
    pub fn publish_outputs(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        outputs: &[HeadlessOutput],
        transaction: &mut dyn FnMut() -> ServiceResult<sophia_protocol::TransactionId>,
    ) -> ServiceResult<()> {
        self.validate(transport)?;
        self.content
            .publish_outputs(transport, outputs, transaction)
    }

    /// Service only the current opening. Closing/removal is deliberately a
    /// separate owner transition; an absent opening cannot authorize old work.
    #[allow(clippy::too_many_arguments)]
    pub fn service_open(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        catalog: &ShellApplicationCatalog,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_backend_live::LiveProductionCpuScene,
        native: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
        outputs: &[HeadlessOutput],
        bounds: &[(OutputId, Rect)],
        root: Rect,
        transaction: &mut dyn FnMut() -> ServiceResult<sophia_protocol::TransactionId>,
    ) -> ServiceResult<()> {
        self.validate(transport)?;
        let (opening, state_revision) = transport
            .native_launcher_state()
            .ok_or(ShellTransportError::WrongCandidate)?;
        if self.closing.is_some() || self.opening.is_some_and(|owned| owned != opening) {
            return Err(ShellTransportError::WrongActivation.into());
        }
        self.opening = Some(opening);
        if catalog.connection_epoch != self.grant.connection_epoch
            || catalog.generation != opening.catalog_generation
        {
            return Err(ShellTransportError::WrongCandidate.into());
        }
        self.content
            .publish_outputs(transport, outputs, transaction)?;
        let now = self.content.now_msec();
        let current = NativeLauncherCandidateContext {
            opening,
            state_revision,
            catalog,
        };
        let allocations = transport.content_allocation_snapshots();
        let context = ContentCandidateContext {
            output: opening.output,
            facts_generation: self.content.facts_generation,
            interaction_generation: 1,
            allocations: &allocations,
        };
        transport.service_native_launcher_content(context, current, now)?;
        while let Some((_, request)) = transport.next_content_allocation_request() {
            if request.operation == 3 {
                transport.release_content_allocation(request.allocation_request_id)?;
                continue;
            }
            match self
                .content
                .resolve_native_allocation(&request, opening, outputs)
            {
                Ok(snapshot) => transport.grant_content_allocation(
                    request.allocation_request_id,
                    snapshot,
                    &[],
                )?,
                Err(error) => {
                    transport.reject_content_allocation(request.allocation_request_id, error)?
                }
            }
        }
        while let Some((demand_transaction, demand)) = transport.next_content_demand() {
            let permit = self.content.next_permit_id;
            self.content.next_permit_id = permit
                .checked_add(1)
                .ok_or("native content permit identity exhausted")?;
            transport.grant_content_demand(demand_transaction, demand.output, permit, now)?;
        }
        let allocations = transport.content_allocation_snapshots();
        let context = ContentCandidateContext {
            allocations: &allocations,
            ..context
        };
        if let Some((_, generation)) = transport.next_content_submission_for(|output| {
            output == opening.output && !self.content.pending.iter().any(|v| v.output == output)
        }) {
            let bundle =
                transport.begin_native_launcher_submission(generation, context, current, now)?;
            self.content.submit_bundle(
                transport,
                runtime,
                scene,
                native,
                outputs,
                bounds,
                root,
                bundle,
                sophia_backend_live::LiveShellContentLayer::Launcher,
                now,
            )?;
            self.submitted = Some(generation);
        }
        Ok(())
    }

    /// Actual displayed identity comes from the runtime. The transport retains
    /// Presented before focus and refuses focus without that exact binding.
    pub fn observe_presentation(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &sophia_backend_live::LiveProductionVisualRuntime,
    ) -> Result<bool, ShellTransportError> {
        self.validate(transport)?;
        let observed = self.content.observe_presentation(transport, runtime)?;
        self.focus_pending |= observed;
        Ok(observed)
    }

    /// Retry focus from the transport's actual Presented owner. A newer state
    /// revision may await another candidate; Prepared never establishes focus.
    pub fn service_focus(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        transaction: sophia_protocol::TransactionId,
    ) -> Result<bool, ShellTransportError> {
        self.validate(transport)?;
        if !self.focus_pending || self.closing.is_some() || !self.inputs.is_empty() {
            return Ok(false);
        }
        match transport.install_native_launcher_focus(transaction) {
            Ok(_) => {
                self.focus_pending = false;
                Ok(true)
            }
            Err(
                ShellTransportError::ContentQueueSaturated | ShellTransportError::WrongCandidate,
            ) => Ok(false),
            Err(error) => Err(error),
        }
    }
}
