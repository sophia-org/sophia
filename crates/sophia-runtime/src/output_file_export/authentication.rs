//! Reserve the output root before an external admission owner pins identity.
use super::*;
use crate::authenticated_files::{PreparedFileRoot, ProtectedFiles};
use sophia_9p::export::{AttachContext, Attachment, Epoch};

/// The exclusive borrow keeps the output grant fixed until publication.
/// Dropping a preparation has no admission or output-proposal side effect.
pub struct OutputFileAttachment<'a> {
    attached: &'a mut bool,
    epoch: Epoch,
}

impl PreparedFileRoot for OutputFileAttachment<'_> {
    type Node = OutputFileNode;

    fn publish(self) -> Attachment<Self::Node> {
        *self.attached = true;
        Attachment {
            root: OutputFileNode::Root,
            epoch: self.epoch,
        }
    }
}

impl ProtectedFiles for OutputFileExport {
    type Prepared<'a> = OutputFileAttachment<'a>;

    fn prepare_authenticated_root(
        &mut self,
        context: &AttachContext<'_>,
    ) -> Result<Self::Prepared<'_>, Errno> {
        self.live()?;
        if self.attached || self.connection != Some(context.connection) {
            return Err(Errno::EACCES);
        }
        Ok(OutputFileAttachment {
            attached: &mut self.attached,
            epoch: Epoch(self.epoch),
        })
    }

    fn allocate_auth_qid(&mut self) -> Result<u64, Errno> {
        self.qids.allocate(1)
    }

    fn reset_authenticated_root(&mut self, connection: ConnectionId) {
        if self.connection == Some(connection) {
            self.attached = false;
        }
    }
}
