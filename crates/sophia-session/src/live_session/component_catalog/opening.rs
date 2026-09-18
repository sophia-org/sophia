//! Join WM requests with the exact borrowed native content service.
use super::*;
use crate::live_session::metadata_shell::NativeLauncherContentService;
use sophia_runtime::ShellTransportConnection;

impl ComponentCatalog {
    pub(in crate::live_session) fn queue_open(&mut self, output: OutputId) -> bool {
        if self.stopped || self.queued_open.is_some() || !output.is_valid() {
            return false;
        }
        self.queued_open = Some((output, Instant::now()));
        true
    }
    pub(in crate::live_session) fn cancel_open_request(&mut self) {
        self.queued_open = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::live_session) fn service_open_content(
        &mut self,
        content: &mut NativeLauncherContentService,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &mut LiveProductionVisualRuntime,
        scene: &LiveProductionCpuScene,
        native: Option<&mut LiveProductionNativeScanout>,
        outputs: &[sophia_engine::HeadlessOutput],
        bounds: &[(OutputId, Rect)],
        root: Rect,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(publication) = self
            .publication
            .as_ref()
            .filter(|p| p.grant() == content.grant())
        else {
            return Ok(());
        };
        if publication.published().is_none() {
            return Ok(());
        }
        if let Some((output, since)) = self.queued_open.take() {
            if since.elapsed() < Duration::from_secs(5) {
                self.next_opening = self
                    .next_opening
                    .checked_add(1)
                    .ok_or("native opening exhausted")?;
                content.request_open(output, self.next_opening);
            } else {
                crate::session_eprintln!("sophia_native_launcher schema=1 status=open_expired");
            }
        }
        let next = &mut self.next_transaction;
        let mut transaction = || {
            *next = next.checked_add(1).ok_or("native transaction exhausted")?;
            Ok(TransactionId::from_raw(*next))
        };
        if let Err(error) = content.publish_outputs(transport, outputs, &mut transaction) {
            if matches!(
                error.downcast_ref::<sophia_runtime::ShellTransportError>(),
                Some(sophia_runtime::ShellTransportError::ContentQueueSaturated)
            ) {
                return Ok(());
            }
            return Err(error);
        }
        content.service_open_request(transport, publication, outputs, &mut transaction)?;
        if transport.native_launcher_state().is_none() {
            return Ok(());
        }
        content.service_open(
            transport,
            publication
                .published()
                .ok_or("catalog FIFO incomplete")?
                .wire(),
            runtime,
            scene,
            native,
            outputs,
            bounds,
            root,
            &mut transaction,
        )?;
        content.observe_presentation(transport, runtime)?;
        content.service_focus(transport, transaction()?)?;
        Ok(())
    }
}
