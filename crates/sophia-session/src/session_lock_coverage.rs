//! Bounded diagnostic publication of all-head cover proof after topology
//! changes. It observes Session and the runtime; it changes neither policy.
use sophia_engine::SessionLockEpoch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLockCoverageRecord {
    pub lock_epoch: u64,
    pub topology_epoch: u64,
    /// The native owner whose heads retired the cover.
    pub owner: u64,
    pub outputs: usize,
    pub heads: usize,
}

#[derive(Default)]
pub struct SessionLockCoveragePublication {
    last: Option<(SessionLockEpoch, u64, u64)>,
}

impl SessionLockCoveragePublication {
    /// At most one record for a lock, published topology and native owner.
    /// `proof` comes only from the runtime's all-current-head retirement
    /// observation on `owner`. A replacement owner on an unchanged topology,
    /// as after a same-port return, owes its own record: its heads are new.
    pub fn update(
        &mut self,
        locked: Option<SessionLockEpoch>,
        topology_epoch: Option<u64>,
        owner: Option<u64>,
        proof: Option<(SessionLockEpoch, usize, usize)>,
    ) -> Option<SessionLockCoverageRecord> {
        let Some(locked) = locked else {
            self.last = None;
            return None;
        };
        let topology_epoch = topology_epoch.filter(|epoch| *epoch != 0)?;
        let owner = owner.filter(|owner| *owner != 0)?;
        let (epoch, outputs, heads) = proof?;
        if epoch != locked
            || outputs == 0
            || heads < outputs
            || self
                .last
                .is_some_and(|(last_epoch, last_topology, last_owner)| {
                    last_epoch == epoch
                        && (last_topology > topology_epoch
                            || (last_topology == topology_epoch && last_owner == owner))
                })
        {
            return None;
        }
        self.last = Some((epoch, topology_epoch, owner));
        Some(SessionLockCoverageRecord {
            lock_epoch: epoch.raw(),
            topology_epoch,
            owner,
            outputs,
            heads,
        })
    }
}

pub fn session_lock_coverage_record(record: SessionLockCoverageRecord) -> String {
    format!(
        "sophia_live_session_lock schema=1 status=covered epoch={} topology_epoch={} owner={} outputs={} heads={}",
        record.lock_epoch, record.topology_epoch, record.owner, record.outputs, record.heads,
    )
}
