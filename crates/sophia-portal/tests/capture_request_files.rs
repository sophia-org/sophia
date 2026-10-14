//! File semantics with a supplied authenticated root; Session transport
//! authentication remains covered by the existing namespace dispatch join.
use sophia_9p::{ConnectionId, Errno, OpenFlags, export::*};
use sophia_portal::{
    capture_requests::CaptureRequestQueue,
    client_files::{PortalClientFiles, PortalClientNode as Node},
    execution_fence::PortalExecutionFence,
    recipient_object::PortalObjectRecipient,
};
use sophia_protocol::{NamespaceId, OutputId, output_capture_request::*};
use sophia_runtime::authenticated_files::{PreparedFileRoot, ProtectedFiles};
use std::sync::Arc;

#[test]
fn capture_files_are_opt_in_authenticated_bounded_and_lock_fenced() {
    let namespace = NamespaceId::from_raw(2);
    let absent = Arc::new(PortalObjectRecipient::new(namespace));
    let mut files = PortalClientFiles::new(namespace, absent, 1).unwrap();
    assert_eq!(
        files.lookup(&Node::Root, WalkName::Child(b"capture")),
        Err(Errno::ENOENT)
    );
    let fence = PortalExecutionFence::default();
    let queue = Arc::new(CaptureRequestQueue::new(fence.clone()));
    let objects =
        Arc::new(PortalObjectRecipient::new(namespace).with_capture_requests(queue.clone()));
    let mut files = PortalClientFiles::new(namespace, objects, 1).unwrap();
    let access = Access {
        connection: ConnectionId(1),
        epoch: Epoch(1),
        node: &Node::Capture,
        operation: Operation::Write,
    };
    assert_eq!(files.check(&access), Err(Errno::ESTALE));
    files.bind_connection(ConnectionId(1)).unwrap();
    files
        .prepare_authenticated_root(&AttachContext {
            connection: ConnectionId(1),
            peer: None,
            uname: b"observer",
            aname: b"portal-client",
            n_uname: u32::MAX,
        })
        .unwrap()
        .publish();
    assert_eq!(files.check(&access), Ok(()));
    assert!(files.open(&Node::Capture, OpenFlags(0)).is_err());
    let mut capture = files.open(&Node::Capture, OpenFlags(1)).unwrap();
    let mut status = files.open(&Node::CaptureStatus, OpenFlags(0)).unwrap();
    let request = OutputCaptureRequest {
        serial: 1,
        output: OutputId::from_raw(7),
    }
    .encode()
    .unwrap();
    assert_eq!(
        files.write(&Node::Capture, &mut capture, 1, &request),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        files.write(&Node::Capture, &mut capture, 0, &request[..23]),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        files.write(&Node::Capture, &mut capture, 0, &request),
        Ok(24)
    );
    // An already-open status remains an immutable snapshot.
    assert_eq!(
        files.read(&Node::CaptureStatus, &mut status, 0, 32),
        Ok(ReadOutcome::Ready(
            OutputCaptureStatus {
                request: None,
                state: OutputCaptureState::Idle
            }
            .encode()
            .unwrap()
            .to_vec()
        ))
    );
    let work = queue.take().unwrap();
    fence.block();
    assert!(!queue.finish(work, OutputCaptureState::Ready));
    assert_eq!(
        files.write(&Node::Capture, &mut capture, 0, &request),
        Err(Errno::EACCES)
    );
    fence.unblock();
    let mut fresh = files.open(&Node::CaptureStatus, OpenFlags(0)).unwrap();
    let ReadOutcome::Ready(bytes) = files.read(&Node::CaptureStatus, &mut fresh, 0, 32).unwrap()
    else {
        panic!("snapshot")
    };
    assert_eq!(
        OutputCaptureStatus::decode(&bytes).unwrap().state,
        OutputCaptureState::Cancelled
    );
    files.reset_authenticated_root(ConnectionId(1));
    assert_eq!(files.check(&access), Err(Errno::ESTALE));
}
