//! One bounded catalog publication retains its exact source and FIFO remainder.
use super::PublishedApplicationCatalog;
use sophia_protocol::{ContentGrant, TransactionId};
use sophia_runtime::{ShellTransportConnection, ShellTransportError};
use std::collections::VecDeque;

const RECORDS_PER_VISIT: usize = 32;
const BYTES_PER_VISIT: usize = 64 * 1024;

pub struct NativeCatalogPublication {
    grant: ContentGrant,
    catalog: PublishedApplicationCatalog,
    remaining: VecDeque<Vec<u8>>,
}
impl NativeCatalogPublication {
    pub fn new(
        transport: &ShellTransportConnection<'_>,
        transaction: TransactionId,
        catalog: PublishedApplicationCatalog,
    ) -> Result<Self, ShellTransportError> {
        if !transport.supports_native_launcher() {
            return Err(ShellTransportError::MissingCapability);
        }
        let grant = transport
            .content_grant()
            .ok_or(ShellTransportError::WrongContentGrant)?;
        if catalog.wire().connection_epoch != grant.connection_epoch {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let frames = catalog.frames(transaction)?;
        if frames.iter().any(|frame| frame.len() > BYTES_PER_VISIT) {
            return Err(ShellTransportError::ActivationQueueSaturated);
        }
        Ok(Self {
            grant,
            catalog,
            remaining: frames.into(),
        })
    }
    pub const fn grant(&self) -> ContentGrant {
        self.grant
    }

    /// Available after every catalog record is FIFO-owned, not peer receipt.
    /// Queue an Opening on that same FIFO only after this becomes available.
    pub fn published(&self) -> Option<&PublishedApplicationCatalog> {
        self.remaining.is_empty().then_some(&self.catalog)
    }

    /// No socket I/O here. On refusal the exact front remains; on success the
    /// prevalidated front is removed immediately, without allocation/callback.
    /// The FIFO then owns partial-write and final-byte lifetime/accounting.
    pub fn service(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
    ) -> Result<bool, ShellTransportError> {
        if transport.content_grant() != Some(self.grant) || !transport.supports_native_launcher() {
            return Err(ShellTransportError::WrongContentGrant);
        }
        let mut bytes = 0;
        for _ in 0..RECORDS_PER_VISIT {
            let Some(frame) = self.remaining.front() else {
                return Ok(true);
            };
            if bytes + frame.len() > BYTES_PER_VISIT {
                break;
            }
            let length = frame.len();
            match transport.enqueue_async(frame.clone()) {
                Ok(()) => {
                    self.remaining.pop_front();
                    bytes += length;
                }
                Err(ShellTransportError::ActivationQueueSaturated) => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        Ok(self.remaining.is_empty())
    }
}
