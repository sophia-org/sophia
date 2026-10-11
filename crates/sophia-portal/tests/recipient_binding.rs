//! Generic recipient bind/revoke seam through real core request dispatch.
//! This deliberately supplies admission and immutable object bytes; it does
//! not claim a production 9P application frontend or namespace compiler.
#[path = "support/recipient_binding.rs"]
mod support;
use sophia_protocol::{NamespaceId, PortalTransferId};
use support::*;

#[test]
fn only_a_current_grant_for_this_recipient_can_bind_one_object() {
    let mut w = Wire::new();
    assert_eq!(w.walk(0, 1, &[b"transfer"]).errno(), Some(2));
    assert!(w.owner.grant(1, false).is_none());
    assert_eq!(w.walk(0, 1, &[b"transfer"]).errno(), Some(2));
    let grant = w.owner.grant(2, true).unwrap();
    let mut foreign = grant.clone();
    foreign.target_namespace = NamespaceId::from_raw(3);
    assert!(
        w.owner.bind(foreign, b"secret").is_err(),
        "foreign grant cannot bind"
    );
    let mut forged = grant.clone();
    forged.source_generation += 1;
    assert!(
        w.owner.bind(forged, b"secret").is_err(),
        "changed grant cannot bind"
    );
    w.owner.bind(grant, b"secret").unwrap();
    w.open(1, b"transfer");
    assert_eq!(w.read(1).data(), b"secret");
    assert_eq!(
        w.walk(0, 2, &[b"source-private"]).errno(),
        Some(2),
        "bind does not expose source namespace"
    );
}

#[test]
fn retained_fid_is_stale_while_recipient_status_and_replacement_stay_usable() {
    let mut w = Wire::new();
    let grant = w.owner.grant(1, true).unwrap();
    w.owner.bind(grant, b"secret").unwrap();
    let old_qid = w.open(1, b"transfer");
    assert_eq!(w.walk(0, 4, &[b"transfer"]).kind, 111);
    w.open(2, b"status");
    assert_eq!(w.read(1).data(), b"secret");
    w.owner
        .lifecycle
        .executor_failed(PortalTransferId::from_raw(1))
        .unwrap();
    let before = w.owner.reads;
    assert_eq!(
        w.read(1).errno(),
        Some(116),
        "revoked bound fid must return ESTALE"
    );
    assert_eq!(
        w.owner.reads, before,
        "revocation is checked before recipient read"
    );
    let mut b = 1u32.to_le_bytes().to_vec();
    b.extend(u64::MAX.to_le_bytes());
    assert_eq!(
        w.one(24, 5, b).errno(),
        Some(116),
        "retained metadata is fenced too"
    );
    // Walking an open fid is EBADF before owner dispatch. Keep a walked,
    // unopened fid to test the retained-node authority check instead.
    assert_eq!(w.walk(4, 5, &[]).errno(), Some(116));
    assert_eq!(w.walk(0, 5, &[b"transfer"]).errno(), Some(116));
    assert_eq!(
        w.read(2).data(),
        b"ready",
        "recipient's own authority survives"
    );
    let fresh = w.owner.grant(2, true).unwrap();
    w.owner.bind(fresh, b"new").unwrap();
    assert_ne!(
        w.open(3, b"transfer"),
        old_qid,
        "replacement has a fresh Qid"
    );
    assert_eq!(w.read(3).data(), b"new");
    assert_eq!(
        w.read(1).errno(),
        Some(116),
        "same pathname cannot revive an old fid"
    );
    assert_eq!(w.one(120, 6, 1u32.to_le_bytes().to_vec()).kind, 121);
    assert!(w.owner.released.contains(&Node::Object(old_qid)));
}

#[test]
fn each_grant_lifetime_boundary_fences_an_open_recipient_fid() {
    for boundary in [
        "source",
        "deadline",
        "source-disconnect",
        "target-disconnect",
        "broker",
        "lock",
        "lock-unlock",
    ] {
        let mut w = Wire::new();
        let grant = w.owner.grant(1, true).unwrap();
        w.owner.bind(grant, b"secret").unwrap();
        w.open(1, b"transfer");
        assert_eq!(w.read(1).data(), b"secret");
        match boundary {
            "source" => {
                w.owner.generation += 1;
                w.owner
                    .lifecycle
                    .source_owner_changed(NamespaceId::from_raw(1), w.owner.generation);
            }
            "deadline" => {
                w.owner.now = 1000;
                w.owner.lifecycle.expire(1000);
            }
            "source-disconnect" => {
                w.owner
                    .lifecycle
                    .namespace_disconnected(NamespaceId::from_raw(1));
            }
            "target-disconnect" => {
                w.owner
                    .lifecycle
                    .namespace_disconnected(NamespaceId::from_raw(2));
            }
            "broker" => {
                w.owner.lifecycle.broker_restarted(2).unwrap();
            }
            "lock" => w.owner.fence.block(),
            "lock-unlock" => {
                w.owner.fence.block();
                w.owner.fence.unblock();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            w.read(1).errno(),
            Some(116),
            "{boundary}: retained portal fid must be stale"
        );
    }
}

#[test]
fn revocation_ends_a_waiting_bound_read_before_any_bytes_are_delivered() {
    let mut w = Wire::new();
    let grant = w.owner.grant(1, true).unwrap();
    w.owner.bind(grant, b"secret").unwrap();
    w.open(1, b"transfer");
    w.owner.ready = false;
    assert!(w.send(116, 42, Wire::<Recipient>::read_body(1)).is_empty());
    assert_eq!(w.connection.waiting_count(), 1);
    w.owner
        .lifecycle
        .executor_failed(PortalTransferId::from_raw(1))
        .unwrap();
    w.owner.ready = true;
    let before = w.owner.reads;
    w.connection.retry_waiting(&mut w.owner).unwrap();
    let r = w.drain();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].tag, 42);
    assert_eq!(
        r[0].errno(),
        Some(116),
        "waiting bound read must end with ESTALE"
    );
    assert_eq!(
        w.owner.reads, before,
        "retry checks revocation before sampling the object"
    );
    assert_eq!(w.connection.waiting_count(), 0);
}
