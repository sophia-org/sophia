//! Panel service state borrows a connection; it owns no socket or byte registry.
use super::{content::LiveContentSession, indicators::LiveIndicatorState, take_shell_transaction};
use sophia_backend_live::{
    LiveProductionCpuScene, LiveProductionNativeScanout, LiveProductionVisualRuntime,
};
use sophia_engine::{
    HeadlessOutput, PolicyIndicatorPublication, PresentedContentBinding, PresentedContentTarget,
};
use sophia_protocol::{ContentGrant, OutputId, OutputReservation, Rect};
use sophia_runtime::{ShellTransportConnection, ShellTransportError};

type ServiceResult<T> = Result<T, Box<dyn std::error::Error>>;

/// State for one admitted panel attempt. Call through the Session component
/// owner's exact connection borrow. Reconnect constructs a fresh service; it
/// cannot reuse the prior grant's published targets or transaction identities.
/// Admission, protected process supervision and native cleanup remain with
/// their actual owners. A service error must be handled for this connection.
pub struct PanelComponentService {
    grant: ContentGrant,
    pub(super) content: LiveContentSession,
    pub(super) indicators: LiveIndicatorState,
    next_transaction: u64,
}

impl PanelComponentService {
    pub fn new(
        transport: &ShellTransportConnection<'_>,
        panel_limit: u16,
        discrete_input: bool,
    ) -> Result<Self, ShellTransportError> {
        if panel_limit == 0
            || !transport.supports_content()
            || transport.supports_native_launcher()
            || (discrete_input && !transport.supports_content_discrete_input())
        {
            return Err(ShellTransportError::MissingCapability);
        }
        let grant = transport
            .content_grant()
            .ok_or(ShellTransportError::MissingCapability)?;
        Ok(Self {
            grant,
            content: LiveContentSession::new(true, discrete_input, Some(panel_limit)),
            indicators: LiveIndicatorState::default(),
            next_transaction: 1,
        })
    }

    fn validate(
        &self,
        transport: &ShellTransportConnection<'_>,
    ) -> Result<(), ShellTransportError> {
        if transport.content_grant() != Some(self.grant) || transport.supports_native_launcher() {
            return Err(ShellTransportError::WrongContentGrant);
        }
        Ok(())
    }

    pub const fn grant(&self) -> ContentGrant {
        self.grant
    }

    #[allow(clippy::too_many_arguments)]
    pub fn service_content(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &mut LiveProductionVisualRuntime,
        scene: &LiveProductionCpuScene,
        native: Option<&mut LiveProductionNativeScanout>,
        outputs: &[HeadlessOutput],
        bounds: &[(OutputId, Rect)],
        root: Rect,
    ) -> ServiceResult<()> {
        self.validate(transport)?;
        let next = &mut self.next_transaction;
        self.content.service(
            transport,
            runtime,
            scene,
            native,
            outputs,
            bounds,
            root,
            &mut || take_shell_transaction(next),
        )
    }

    pub fn observe_presentation(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &LiveProductionVisualRuntime,
    ) -> Result<bool, ShellTransportError> {
        self.validate(transport)?;
        self.content.observe_presentation(transport, runtime)
    }

    pub fn service_indicators(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        publication: Option<&PolicyIndicatorPublication>,
        active_output: Option<OutputId>,
    ) -> ServiceResult<()> {
        self.validate(transport)?;
        let next = &mut self.next_transaction;
        self.indicators
            .service_publication(transport, publication, active_output, &mut || {
                take_shell_transaction(next)
            })
    }

    pub fn issue_activation(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        target: PresentedContentTarget,
        runtime: &LiveProductionVisualRuntime,
    ) -> ServiceResult<Option<u64>> {
        self.validate(transport)?;
        let next = &mut self.next_transaction;
        self.content
            .issue_presented_activation(transport, target, runtime, &mut || {
                take_shell_transaction(next)
            })
    }

    pub fn service_actions(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        presented: &[PresentedContentBinding],
    ) -> ServiceResult<usize> {
        self.validate(transport)?;
        let next = &mut self.next_transaction;
        self.content
            .service_actions(transport, presented, &mut || take_shell_transaction(next))
    }

    pub(in crate::live_session) fn service_indicator_activation(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        admit: impl FnOnce(
            sophia_protocol::WmActionId,
            OutputId,
        ) -> ServiceResult<crate::live_session::LiveIndicatorAdmissionResult>,
    ) -> Result<bool, super::indicators::IndicatorServiceError> {
        self.validate(transport)
            .map_err(|error| super::indicators::IndicatorServiceError::Poll(error.into()))?;
        self.content
            .service_indicator_request(transport, &mut self.indicators, admit)
    }

    pub(super) fn presented_work_area_bands(&self) -> Option<Vec<OutputReservation>> {
        self.content
            .has_presented_content()
            .then(|| self.content.work_area_bands())
    }

    pub fn work_area_bands(&self) -> Vec<OutputReservation> {
        self.content.work_area_bands()
    }
}
