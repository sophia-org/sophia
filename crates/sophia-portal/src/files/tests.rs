#![cfg(test)]

use super::*;
use sophia_protocol::{PortalDecision, PortalTransfer};

struct Inactive;
impl PortalFileAuthority for Inactive {
    fn namespace(&self, _: NamespaceId) -> Option<NamespaceContext> {
        None
    }
    fn source_generation(&self, _: &PortalRequest) -> Option<u64> {
        None
    }
    fn now_msec(&self) -> u64 {
        0
    }
    fn live(&self) -> bool {
        false
    }
}

#[test]
fn history_and_faults_do_not_prevent_parking_but_pending_custody_does() {
    let mut export = PortalFileExport::new(Inactive, PortalFileLimits::defaults(1)).unwrap();
    export.retained = 3;
    export.faulted = true;
    assert!(
        export.idle(),
        "history and a dead authority need no periodic service"
    );
    let id = PortalTransferId::from_raw(1);
    export.issued = Some(id);
    assert!(!export.idle());
    export.issued = None;
    export.upload = Some(Upload {
        transfer: id,
        qid: 1,
        total: 1,
        bytes: vec![0],
        writer: true,
    });
    assert!(!export.idle());
    export.upload = None;
    let source = NamespaceId::from_raw(1);
    let target = NamespaceId::from_raw(2);
    export.execution = Some(PortalFileExecution {
        request: PortalRequest {
            transfer: PortalTransfer {
                transfer: id,
                source_namespace: source,
                target_namespace: target,
                kind: PortalTransferKind::Clipboard,
                mime_type: Some("text/plain".into()),
                byte_size: 1,
                decision: PortalDecision::Pending,
                generation: 1,
            },
            deadline_msec: 100,
        },
        grant: PortalGrant {
            transfer: id,
            source_namespace: source,
            target_namespace: target,
            kind: PortalTransferKind::Clipboard,
            source_generation: 1,
            broker_generation: 1,
            deadline_msec: 100,
            state: PortalGrantState::Active,
        },
        payload: vec![0],
        authority_revision: 1,
    });
    assert!(!export.idle());
    export.execution = None;
    assert!(export.idle());
    assert_eq!(export.retained, 3);
    assert!(export.faulted);
}
