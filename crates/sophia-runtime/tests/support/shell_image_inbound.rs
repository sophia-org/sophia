use super::*;
use sophia_protocol::ContentResourceRetire;

#[test]
fn submitted_offer_ids_cannot_be_reused() {
    let mut offers = image_offers::ImageOffers::default();
    assert!(offers.accepts_id(7));
    offers.note_submitted(7);
    assert!(!offers.accepts_id(7));
    assert!(!offers.accepts_id(6));
    assert!(offers.accepts_id(8));
}

#[test]
fn resource_cancel_cannot_overtake_its_image_admission() {
    let mut files = ShellFiles::awaiting_negotiation(
        1,
        "bar",
        false,
        JournalBounds {
            bytes: 65536,
            reserve_bytes: 12288,
        },
        100,
        Instant::now(),
    );
    let metadata = ShellImageMetadata {
        offer_id: 7,
        resource: 1,
        resource_generation: 1,
        admission_epoch: 1,
        output_id: 1,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: 1,
        source_format: 1,
        width: 2,
        height: 1,
        stride: 8,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 8,
        modifier: 0,
    };
    let cancel = |id| {
        Inbound::Content(
            TransactionId::from_raw(id),
            Box::new(ShellContentRecord::ResourceCancel(ContentResourceCancel {
                grant: ContentGrant {
                    connection_epoch: 1,
                    content_grant_epoch: 1,
                },
                resource: ContentResourceId {
                    id: 1,
                    generation: 1,
                },
            })),
        )
    };
    files.inbound.push_back(cancel(6));
    files.inbound.push_back(Inbound::ImageBegin(metadata));
    files.inbound.push_back(cancel(7));
    assert!(files.pending_image_offer().is_none());
    assert_eq!(files.peek_content(|_| true).unwrap().0.raw(), 6);
    assert_eq!(files.take_content(|_| true).unwrap().0.raw(), 6);
    assert_eq!(files.pending_image_offer(), Some(metadata));
    assert!(files.peek_content(|_| true).is_none());
    assert!(files.take_content(|_| true).is_none());
    // Unrelated controls and a Retire behind the offer still make progress.
    // In particular, Retire can release the backing needed by ImageBegin.
    files.inbound.push_back(cancel(8));
    files.inbound.push_back(Inbound::Content(
        TransactionId::from_raw(9),
        Box::new(ShellContentRecord::ResourceRetire(ContentResourceRetire {
            grant: ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            },
            resource: ContentResourceId {
                id: 2,
                generation: 1,
            },
        })),
    ));
    assert_eq!(files.peek_content(|_| true).unwrap().0.raw(), 8);
    assert_eq!(files.take_content(|_| true).unwrap().0.raw(), 8);
    assert_eq!(files.take_content(|_| true).unwrap().0.raw(), 9);
    assert_eq!(files.take_inbound(), Some(Inbound::ImageBegin(metadata)));
    assert_eq!(files.take_content(|_| true).unwrap().0.raw(), 7);
    assert!(files.inbound_is_empty());
}
