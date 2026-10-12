//! Reserve a shell root before the authentication owner pins identity.
use super::*;
use crate::authenticated_files::{PreparedFileRoot, ProtectedFiles};

pub(in crate::shell_transport) struct ShellFileAttachment<'a> {
    attached: &'a mut bool,
    epoch: Epoch,
}

impl PreparedFileRoot for ShellFileAttachment<'_> {
    type Node = Node;

    fn publish(self) -> Attachment<Node> {
        *self.attached = true;
        Attachment {
            root: Node::Root,
            epoch: self.epoch,
        }
    }
}

impl ProtectedFiles for ShellFiles {
    type Prepared<'a> = ShellFileAttachment<'a>;

    fn prepare_authenticated_root(
        &mut self,
        context: &AttachContext<'_>,
    ) -> Result<Self::Prepared<'_>, Errno> {
        if self.revoked || self.attached || self.connection != Some(context.connection) {
            return Err(Errno::EACCES);
        }
        Ok(ShellFileAttachment {
            attached: &mut self.attached,
            epoch: Epoch(self.epoch),
        })
    }

    fn allocate_auth_qid(&mut self) -> Result<u64, Errno> {
        // Auth fids and role objects share the component's monotonic allocator.
        self.allocate_qid()
    }

    fn reset_authenticated_root(&mut self, connection: ConnectionId) {
        if self.connection == Some(connection) {
            self.attached = false;
        }
    }
}
