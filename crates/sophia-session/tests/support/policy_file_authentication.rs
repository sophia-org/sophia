//! Root publication mechanics only; no supplied verdict counts as authentication.
#![cfg(test)]
use super::*;
use crate::live_session::policy_transport_worker::ninep::typed_codec::TypedFileCodec;

fn context(connection: u64) -> AttachContext<'static> {
    AttachContext {
        connection: ConnectionId(connection),
        peer: None,
        uname: b"wm",
        aname: b"spatial-policy",
        n_uname: u32::MAX,
    }
}

fn files() -> WmFiles<TypedFileCodec> {
    let mut files = WmFiles::awaiting_negotiation(
        7,
        WmFileLimits {
            capability_ceiling: 0,
            profile_required: false,
        },
        WmQids::new(),
        TypedFileCodec,
    )
    .unwrap();
    files.bind_connection(ConnectionId(1));
    files
}

#[test]
fn prepared_wm_root_publishes_only_after_commit() {
    let mut files = files();
    {
        let _uncommitted = files.prepare_authenticated_root(&context(1)).unwrap();
    }
    assert!(!files.attached, "abandoned preparation cannot admit the WM");
    assert!(matches!(
        files.prepare_authenticated_root(&context(2)),
        Err(Errno::EACCES)
    ));
    let root = files
        .prepare_authenticated_root(&context(1))
        .unwrap()
        .publish();
    assert!(root.root == Node::Root && root.epoch == Epoch(7));
    assert!(files.attached);
    assert!(matches!(files.attach(&context(1)), Err(Errno::EACCES)));
}

#[test]
fn authenticated_wm_reset_preserves_epoch_negotiation_and_replay_bounds() {
    let mut files = files();
    files.bind_selected(0).unwrap();
    files.submission_watermark = 42;
    files.domain_transaction_watermark = 73;
    files.attach(&context(1)).unwrap();
    files.reset_authenticated_root(ConnectionId(2));
    assert!(matches!(files.attach(&context(1)), Err(Errno::EACCES)));
    files.reset_authenticated_root(ConnectionId(1));
    assert_eq!(files.attach(&context(1)).unwrap().epoch, Epoch(7));
    assert_eq!(files.selected_capabilities(), Some(0));
    assert_eq!(files.submission_watermark, 42);
    assert_eq!(files.domain_transaction_watermark, 73);
    files.revoke();
    files.reset_authenticated_root(ConnectionId(1));
    assert!(matches!(files.attach(&context(1)), Err(Errno::EACCES)));
}

#[test]
fn wm_auth_qids_share_the_role_allocator_and_fail_closed_at_exhaustion() {
    let mut files = files();
    let first = files.allocate_auth_qid().unwrap();
    let second = files.allocate_auth_qid().unwrap();
    assert!(first >= files.base + 8 && second > first);
    *files.qids.0.lock().unwrap() = u64::MAX;
    assert_eq!(files.allocate_auth_qid(), Err(Errno::ENOSPC));
}
