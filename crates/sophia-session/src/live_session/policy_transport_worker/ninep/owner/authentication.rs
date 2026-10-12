//! Prepare the WM root without publishing role authority before afid commit.
use super::*;

pub(in crate::live_session::policy_transport_worker) struct WmFileAttachment<'a> {
    attached: &'a mut bool,
    epoch: Epoch,
}

impl PreparedFileRoot for WmFileAttachment<'_> {
    type Node = Node;

    fn publish(self) -> Attachment<Node> {
        *self.attached = true;
        Attachment {
            root: Node::Root,
            epoch: self.epoch,
        }
    }
}

impl<C: PolicyFileCodec> ProtectedFiles for WmFiles<C> {
    type Prepared<'a>
        = WmFileAttachment<'a>
    where
        C: 'a;

    fn prepare_authenticated_root(
        &mut self,
        context: &AttachContext<'_>,
    ) -> Result<Self::Prepared<'_>, Errno> {
        if self.revoked || self.attached || self.connection != Some(context.connection) {
            return Err(Errno::EACCES);
        }
        Ok(WmFileAttachment {
            attached: &mut self.attached,
            epoch: Epoch(self.epoch),
        })
    }

    fn allocate_auth_qid(&mut self) -> Result<u64, Errno> {
        self.qids.allocate(1)
    }

    fn reset_authenticated_root(&mut self, connection: ConnectionId) {
        // Only the authenticated wrapper calls this after releasing all fids.
        // Epoch, negotiation, journal and replay watermarks remain unchanged.
        if self.connection == Some(connection) {
            self.attached = false;
        }
    }
}

#[path = "../../../../../tests/support/policy_file_authentication.rs"]
mod tests;
