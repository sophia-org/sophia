//! Snapshot the existing epoch, input and aggregate response owners.

use super::ShellComponentTransport;
use super::ShellSessionTransport;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellContentShutdown {
    pub settled_candidates: usize,
    pub accounting: ShellContentAccounting,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShellContentAccounting {
    /// Common registry snapshot. With independent components this is Session
    /// accounting and must not be summed once per connection. The remaining
    /// fields count only this connection's exact credits, FIFO and input.
    pub epochs: crate::ContentEpochAccounting,
    /// Store credits, queued store responses, action-cancel/outcome credits,
    /// and FIFO frames. A partial write still owns the entire frame charge.
    pub response_records: usize,
    pub response_bytes: usize,
    pub input_records: usize,
    pub input_bytes: usize,
}

impl ShellContentAccounting {
    pub fn quiescent(&self) -> bool {
        self.epochs.quiescent()
            && self.response_records == 0
            && self.response_bytes == 0
            && self.input_records == 0
            && self.input_bytes == 0
    }
}

impl ShellComponentTransport {
    /// Final shutdown only, after successful native detach/cleanup. The caller
    /// transfers its remaining backend owner; the owner is dropped BEFORE any
    /// submitted candidate's terminal transition. Earlier Engine/scene owners
    /// must already have ended. This is not a reconnect or cancellation API.
    ///
    /// A live connection or epoch refuses and returns the untouched backend.
    /// Independent retained pixel consumers remain charged in the returned
    /// snapshot; the caller must reject non-quiescence. This proves returned
    /// completion, not recovery from a panic in an owner's destructor.
    pub fn finish_content_after_backend_drop<B>(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
        backend: B,
    ) -> Result<ShellContentShutdown, B> {
        if self.negotiation.is_some()
            || self.stream.is_some()
            || self.content_grant.is_some()
            || epochs.resources(self.store_grant).is_some()
        {
            return Err(backend);
        }
        let settled_candidates = epochs.finish_after_backend_drop(backend)?;
        Ok(ShellContentShutdown {
            settled_candidates,
            accounting: self.collect_content_accounting(epochs),
        })
    }

    /// Observe charges rather than allocating a second resource registry.
    pub fn content_accounting(
        &self,
        epochs: &crate::ContentEpochRegistry,
    ) -> ShellContentAccounting {
        let epoch_accounting = epochs.accounting();
        let (bulk_records, bulk_bytes) = epochs.bulk_occupancy(self.store_grant);
        let reserved = epochs.control_occupancy(self.store_grant)
            + self.action_cancellations.len()
            + usize::from(self.indicator_response.is_some())
            + usize::from(self.catalog_response.is_some())
            + self.native_control.credits();
        let controls = reserved - bulk_records + self.output.controls();
        let negotiating = usize::from(self.negotiation.is_some());
        ShellContentAccounting {
            epochs: epoch_accounting,
            response_records: reserved
                + self.output.records()
                + negotiating * super::negotiation_service::REPLY_RECORDS,
            response_bytes: bulk_bytes
                + self.output.bulk_bytes()
                + controls * self.control_frame_bytes()
                + negotiating * super::negotiation_service::REPLY_BYTES,
            input_records: self.inbox.len() + negotiating,
            input_bytes: self.input.len()
                + self.inbox.iter().map(Vec::len).sum::<usize>()
                + negotiating * (super::SOPHIA_IPC_HEADER_LEN + 12),
        }
    }

    /// Collection only releases stores whose actual tracked consumers ended.
    /// Calling this after disconnect is safe and requires no socket operation.
    pub fn collect_content_accounting(
        &mut self,
        epochs: &mut crate::ContentEpochRegistry,
    ) -> ShellContentAccounting {
        epochs.collect();
        self.content_accounting(epochs)
    }
}

// Legacy single-shell facade, delegating to the same shared registry path.

// The owned legacy and borrowed Session façades share forwarding, not policy.
macro_rules! transport_facade {
    ($transport:ty) => {
        impl $transport {
            pub fn finish_content_after_backend_drop<B>(
                &mut self,
                backend: B,
            ) -> Result<ShellContentShutdown, B> {
                self.state
                    .finish_content_after_backend_drop(&mut self.content_epochs, backend)
            }

            pub fn content_accounting(&self) -> ShellContentAccounting {
                self.state.content_accounting(&self.content_epochs)
            }

            pub fn collect_content_accounting(&mut self) -> ShellContentAccounting {
                self.state
                    .collect_content_accounting(&mut self.content_epochs)
            }
        }
    };
}
transport_facade!(ShellSessionTransport);
transport_facade!(crate::shell_transport::ShellTransportConnection<'_>);
