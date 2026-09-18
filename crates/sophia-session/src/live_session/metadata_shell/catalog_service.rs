//! One persistent catalog component borrows Session's connection and content
//! owners. Construction does not enable process admission or manufacture focus.
use super::{content::LiveContentSession, take_shell_transaction};
use crate::application_catalog::PublishedApplicationCatalog;
use crate::session_actions::SessionLaunchQueue;
use sophia_backend_live::{
    LiveProductionCpuScene, LiveProductionNativeScanout, LiveProductionVisualRuntime,
};
use sophia_engine::{HeadlessOutput, PresentedContentBinding, PresentedContentTarget};
use sophia_protocol::{ContentGrant, OutputId, OutputReservation, Rect, SessionApplicationId};
use sophia_runtime::{ShellTransportConnection, ShellTransportError};

type ServiceResult<T> = Result<T, Box<dyn std::error::Error>>;

pub struct CatalogComponentService {
    grant: ContentGrant,
    content: LiveContentSession,
    next_transaction: u64,
}

impl CatalogComponentService {
    pub fn new(
        transport: &ShellTransportConnection<'_>,
        panel_limit: u16,
        reservation: Option<sophia_config::ShellComponentReservation>,
    ) -> Result<Self, ShellTransportError> {
        if panel_limit == 0 || !transport.supports_persistent_catalog() {
            return Err(ShellTransportError::MissingCapability);
        }
        let grant = transport
            .content_grant()
            .ok_or(ShellTransportError::MissingCapability)?;
        let mut content = LiveContentSession::new(true, true, Some(panel_limit));
        content.component_reservation = reservation;
        Ok(Self {
            grant,
            content,
            next_transaction: 1,
        })
    }
    fn validate(
        &self,
        transport: &ShellTransportConnection<'_>,
    ) -> Result<(), ShellTransportError> {
        if !transport.supports_persistent_catalog() || transport.content_grant() != Some(self.grant)
        {
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
        publication: &PublishedApplicationCatalog,
        runtime: &mut LiveProductionVisualRuntime,
        scene: &LiveProductionCpuScene,
        native: Option<&mut LiveProductionNativeScanout>,
        outputs: &[HeadlessOutput],
        bounds: &[(OutputId, Rect)],
        root: Rect,
    ) -> ServiceResult<()> {
        self.validate(transport)?;
        if publication.wire().connection_epoch != self.grant.connection_epoch {
            return Err(ShellTransportError::WrongContentGrant.into());
        }
        let next = &mut self.next_transaction;
        self.content.service_with_catalog(
            transport,
            runtime,
            scene,
            native,
            outputs,
            bounds,
            root,
            &mut || take_shell_transaction(next),
            Some(publication.wire()),
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
    #[allow(clippy::too_many_arguments)]
    pub fn service_actions(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        publication: &PublishedApplicationCatalog,
        presented: &[PresentedContentBinding],
        launches: &mut SessionLaunchQueue,
        application: SessionApplicationId,
        active_children: usize,
    ) -> ServiceResult<usize> {
        self.validate(transport)?;
        let next = &mut self.next_transaction;
        self.content
            .service_actions(transport, presented, &mut || take_shell_transaction(next))?;
        Ok(self.content.service_catalog_requests(
            transport,
            publication,
            presented,
            launches,
            application,
            active_children,
        )?)
    }
    pub fn presented_work_area_bands(&self) -> Option<Vec<OutputReservation>> {
        self.content
            .has_presented_content()
            .then(|| self.content.work_area_bands())
    }
}
