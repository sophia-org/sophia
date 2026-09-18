//! Retained opening notification after actual catalog and output FIFO ownership.
use super::*;
use crate::application_catalog::NativeCatalogPublication;
use sophia_protocol::{NativeLauncherOpening, TransactionId};

pub(super) struct OpenRequest {
    output: OutputId,
    id: u64,
    transfer: Option<(TransactionId, NativeLauncherOpening)>,
}
impl NativeLauncherContentService {
    /// Session policy requests UI only; this grants neither focus nor execution.
    /// One pending request is retained without retargeting an existing opening.
    pub fn request_open(&mut self, output: OutputId, opening: u64) -> bool {
        if !output.is_valid()
            || opening == 0
            || self.open_request.is_some()
            || (self.opening.is_some() && self.closing.is_none())
        {
            return false;
        }
        self.open_request = Some(OpenRequest {
            output,
            id: opening,
            transfer: None,
        });
        true
    }

    pub fn service_open_request(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        publication: &NativeCatalogPublication,
        outputs: &[HeadlessOutput],
        transaction: &mut dyn FnMut() -> ServiceResult<TransactionId>,
    ) -> ServiceResult<bool> {
        self.validate(transport)?;
        if publication.grant() != self.grant {
            return Err(ShellTransportError::WrongContentGrant.into());
        }
        if self.open_request.is_none() {
            return Ok(false);
        }
        let Some(catalog) = publication.published() else {
            return Ok(false);
        };
        self.publish_outputs(transport, outputs, transaction)?;
        let request = self
            .open_request
            .as_mut()
            .expect("request checked before publication");
        let output = self
            .content
            .published_facts
            .iter()
            .find(|fact| fact.output.id == request.output.raw())
            .ok_or("requested launcher output is not published")?
            .output;
        let opening = NativeLauncherOpening {
            grant: self.grant,
            opening: request.id,
            output,
            catalog_generation: catalog.wire().generation,
            state_revision: 1,
        };
        if let Some((_, retained)) = request.transfer {
            if retained != opening {
                return Err(ShellTransportError::WrongActivation.into());
            }
        } else {
            request.transfer = Some((transaction()?, opening));
        }
        let (tx, opening) = request
            .transfer
            .expect("transfer retained before admission");
        let result = if self.closing.is_some() {
            self.reopen(transport, tx, opening)
        } else {
            transport
                .publish_native_launcher_opening(tx, opening)
                .map(|()| true)
        };
        match result {
            Ok(true) => {
                self.opening = Some(opening);
                self.open_request = None;
                Ok(true)
            }
            Ok(false) | Err(ShellTransportError::ContentQueueSaturated) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}
