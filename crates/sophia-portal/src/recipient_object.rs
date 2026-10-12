//! Bounded immutable objects installed by an admitted recipient. The frontend
//! owns names and fids; this store owns the payload and its non-forgeable lease.
use crate::{
    coordinator::{PortalAccessLease, PortalRecipient, PortalTransferFailure},
    files::PortalFileExecution,
};
use sophia_9p::Errno;
use sophia_protocol::{NamespaceId, PortalTransferId};
use std::sync::{Arc, Mutex};

struct Object {
    lease: PortalAccessLease,
    bytes: Vec<u8>,
}
struct State {
    next: u64,
    current: Option<(u64, Arc<Object>)>,
}
pub struct PortalObjectRecipient {
    namespace: NamespaceId,
    state: Mutex<State>,
}
impl PortalObjectRecipient {
    pub fn namespace(&self) -> NamespaceId {
        self.namespace
    }
    pub fn new(namespace: NamespaceId) -> Self {
        Self {
            namespace,
            state: Mutex::new(State {
                next: 1,
                current: None,
            }),
        }
    }
    /// The export's check/open/metadata/read paths all use this same check.
    /// A removed incarnation is stale, even when the pathname has a successor.
    pub fn read(&self, qid: u64, offset: u64, count: u32) -> Result<Vec<u8>, Errno> {
        self.with_payload(qid, |bytes| {
            let start = usize::try_from(offset)
                .unwrap_or(usize::MAX)
                .min(bytes.len());
            bytes[start..]
                .iter()
                .take(count as usize)
                .copied()
                .collect()
        })
    }
    fn with_payload<T>(&self, qid: u64, read: impl FnOnce(&[u8]) -> T) -> Result<T, Errno> {
        let object = {
            let state = self.state.lock().map_err(|_| Errno::EIO)?;
            state
                .current
                .as_ref()
                .filter(|(current, _)| *current == qid)
                .map(|(_, object)| object.clone())
                .ok_or(Errno::ESTALE)?
        };
        object
            .lease
            .with_access(|| {
                let state = self.state.lock().map_err(|_| Errno::EIO)?;
                if state
                    .current
                    .as_ref()
                    .is_none_or(|(current, _)| *current != qid)
                {
                    return Err(Errno::ESTALE);
                }
                Ok(read(&object.bytes))
            })
            .ok_or(Errno::ESTALE)?
    }
    pub fn current(&self) -> Result<u64, Errno> {
        let qid = self
            .state
            .lock()
            .map_err(|_| Errno::EIO)?
            .current
            .as_ref()
            .map(|(qid, _)| *qid)
            .ok_or(Errno::ENOENT)?;
        self.read(qid, 0, 0)?;
        Ok(qid)
    }
    pub fn size(&self, qid: u64) -> Result<u64, Errno> {
        self.with_payload(qid, |bytes| bytes.len() as u64)
    }
    /// Revoking the frontend's binding never revives an older object.
    pub fn revoke(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.current = None;
        }
    }
}
impl PortalRecipient for PortalObjectRecipient {
    fn execute(&self, work: &PortalFileExecution, lease: PortalAccessLease) -> bool {
        if work.grant.target_namespace != self.namespace
            || lease.grant() != &work.grant
            || work.payload.len() > 65536
        {
            return false;
        }
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let Some(next) = state.next.checked_add(1) else {
            return false;
        };
        let qid = state.next;
        // This bind replaces exactly one name. Retained fids keep their old
        // Qid, never the new bytes; stale payloads need not remain allocated.
        state.current = Some((
            qid,
            Arc::new(Object {
                lease,
                bytes: work.payload.clone(),
            }),
        ));
        state.next = next;
        true
    }
    fn failed(&self, _: PortalTransferId, _: PortalTransferFailure) {}
}
