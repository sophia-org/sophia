//! Inspection root reservation is separate from authentication commitment.
use super::{Admission, InspectionExport, Node};
use crate::authenticated_files::{PreparedFileRoot, ProtectedFiles};
use sophia_9p::{AttachContext, Attachment, ConnectionId, Epoch, Errno};

pub(in crate::inspection) struct PreparedInspectionRoot<'a> {
    attached: &'a mut Option<ConnectionId>,
    connection: ConnectionId,
    epoch: Epoch,
}
impl PreparedFileRoot for PreparedInspectionRoot<'_> {
    type Node = Node;
    fn publish(self) -> Attachment<Node> {
        *self.attached = Some(self.connection);
        Attachment {
            root: Node::Root,
            epoch: self.epoch,
        }
    }
}
impl ProtectedFiles for InspectionExport {
    type Prepared<'a> = PreparedInspectionRoot<'a>;
    fn prepare_authenticated_root(
        &mut self,
        c: &AttachContext<'_>,
    ) -> Result<Self::Prepared<'_>, Errno> {
        if !self.authorized() {
            return Err(Errno::ESTALE);
        }
        if !matches!(self.admission, Admission::Authenticated(Some(id)) if id == c.connection)
            || self.attached.is_some()
        {
            return Err(Errno::EACCES);
        }
        Ok(PreparedInspectionRoot {
            attached: &mut self.attached,
            connection: c.connection,
            epoch: Epoch(self.generation),
        })
    }
    fn allocate_auth_qid(&mut self) -> Result<u64, Errno> {
        self.shared.qids(1).map_err(|_| Errno::ENOSPC)
    }
    fn reset_authenticated_root(&mut self, connection: ConnectionId) {
        if matches!(self.admission, Admission::Authenticated(Some(id)) if id == connection) {
            self.attached = None;
            self.pinned = false;
            self.coherent = None;
        }
    }
}
