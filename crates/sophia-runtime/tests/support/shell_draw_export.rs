use super::*;
use sophia_draw::{Batch, Command};

fn batch() -> Batch {
    Batch {
        transaction: 10,
        grant: ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        },
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        width: 24,
        height: 24,
        commands: vec![Command::Rect {
            x: 0,
            y: 0,
            width: 24,
            height: 24,
            rgb: [1, 2, 3],
        }],
    }
}
fn export() -> ShellFiles {
    ShellFiles::awaiting_negotiation(
        1,
        "bar",
        false,
        JournalBounds {
            bytes: 65536,
            reserve_bytes: 12288,
        },
        100,
        Instant::now(),
    )
}
#[test]
fn draw_requires_admission_bounds_epoch_and_exact_retry() {
    let mut f = export();
    let mut b = batch();
    let bytes = b.encode().unwrap();
    assert!(
        !f.root_entries()
            .iter()
            .any(|(n, _)| *n == b"experimental-draw-v1")
    );
    assert_eq!(f.write_draw(&bytes), Err(Errno::EACCES));
    f.complete_negotiation(Some((vec![], ContentLimits::prototype(b.grant))), 0)
        .unwrap();
    assert!(
        f.root_entries()
            .iter()
            .any(|(n, _)| *n == b"experimental-draw-v1")
    );
    assert_eq!(f.write_draw(b"malformed"), Err(Errno::EINVAL));
    assert_eq!(
        f.write_draw(&vec![0; sophia_draw::MAX_BATCH_BYTES + 1]),
        Err(Errno::EINVAL)
    );
    b.grant.connection_epoch += 1;
    assert_eq!(f.write_draw(&b.encode().unwrap()), Err(Errno::ESTALE));
    b = batch();
    assert!(f.inbound_is_empty());
    assert_eq!(f.write_draw(&bytes), Ok(bytes.len() as u32));
    assert!(!f.inbound_is_empty());
    let deadline = f.pending_draw().unwrap().1;
    assert_eq!(f.write_draw(&bytes), Ok(bytes.len() as u32));
    assert_eq!(
        f.pending_draw().unwrap().1,
        deadline,
        "retry cannot extend custody"
    );
    b.resource.id += 1;
    assert_eq!(f.write_draw(&b.encode().unwrap()), Err(EALREADY));
    b.transaction += 1;
    assert_eq!(f.write_draw(&b.encode().unwrap()), Err(Errno::EAGAIN));
    assert_eq!(f.take_draw().unwrap().0, batch());
    assert_eq!(f.write_draw(&bytes), Ok(bytes.len() as u32));
    assert!(
        f.pending_draw().is_none(),
        "retry after consumption must not enqueue"
    );
    assert!(f.write_draw(&b.encode().unwrap()).is_ok());
    f.revoke();
    assert!(f.pending_draw().is_none());
    assert_eq!(f.write_draw(&bytes), Err(Errno::EACCES));
}
#[test]
fn cancellation_waits_for_draw_admission_but_other_resources_progress() {
    let mut f = export();
    let b = batch();
    f.complete_negotiation(Some((vec![], ContentLimits::prototype(b.grant))), 0)
        .unwrap();
    f.write_draw(&b.encode().unwrap()).unwrap();
    let cancel = |id| {
        Inbound::Content(
            TransactionId::from_raw(20 + id),
            Box::new(ShellContentRecord::ResourceCancel(ContentResourceCancel {
                grant: b.grant,
                resource: ContentResourceId { id, generation: 1 },
            })),
        )
    };
    f.inbound.push_back(cancel(1));
    f.inbound.push_back(cancel(2));
    assert_eq!(f.take_content(|_| true).unwrap().0.raw(), 22);
    assert!(f.peek_content(|_| true).is_none());
    f.take_draw();
    assert_eq!(f.take_content(|_| true).unwrap().0.raw(), 21);
    assert!(f.inbound_is_empty());
}
