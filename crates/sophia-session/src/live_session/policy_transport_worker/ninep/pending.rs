//! Endpoint custody before the existing file/profile driver admission. The
//! supervisor supplies launch evidence; this is not a namespace inventory.
use super::*;
use sophia_runtime::{PolicyRole, PolicyRoleEndpoint, ProcessSupervisor, ProtectionDomainRole};

#[path = "../../../../tests/support/policy_file_pending.rs"]
mod tests;

pub(super) struct PendingEndpoint {
    endpoint: PolicyRoleEndpoint,
    qids: Option<WmQids>,
}
impl PendingEndpoint {
    pub(super) fn authorize(
        mut endpoint: PolicyRoleEndpoint,
        supervisor: &ProcessSupervisor,
        qids: WmQids,
    ) -> Result<Self, String> {
        if endpoint.role() != PolicyRole::Wm {
            return Err("WM file endpoint requires WM role".into());
        }
        let evidence = supervisor
            .protection_evidence()
            .ok_or("WM file endpoint requires supervised protection evidence")?;
        if supervisor.peer_id() != Some(evidence.peer_pid)
            || !evidence
                .roles
                .contains(&ProtectionDomainRole::SpatialPolicy)
        {
            return Err("WM file protection evidence does not name launched spatial peer".into());
        }
        endpoint
            .authorize_protected_peer(evidence)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            endpoint,
            qids: Some(qids),
        })
    }

    pub(super) fn accept(
        &mut self,
        cancellation: &NinePCancellation,
        deadline: Instant,
    ) -> Result<(UnixStream, WmQids), String> {
        if self.qids.is_none() {
            return Err("WM file endpoint already adopted".into());
        }
        loop {
            check_publication(&cancellation.stopped, deadline)
                .map_err(|e| format!("WM file accept stopped or expired: {e:?}"))?;
            if let Some(stream) = self.endpoint.poll_expected().map_err(|e| e.to_string())? {
                check_publication(&cancellation.stopped, deadline)
                    .map_err(|e| format!("WM file accept stopped or expired: {e:?}"))?;
                return Ok((stream, self.qids.take().expect("checked before accept")));
            }
            // The endpoint owns credential checking; this bounded wait is only
            // pre-reactor cancellation latency, with no lock held or new phase.
            std::thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(2)),
            );
        }
    }
}
impl Drop for PendingEndpoint {
    fn drop(&mut self) {
        if let Some(peer) = self.endpoint.active_peer() {
            let _ = self.endpoint.release_peer(peer);
        }
    }
}
