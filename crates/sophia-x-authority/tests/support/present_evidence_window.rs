use super::*;

#[test]
fn a_success_burst_keeps_only_the_exact_recent_tail_and_counts_every_attempt() {
    let mut window = Window::new();
    for serial in 0..1000u32 {
        window.push(
            Record::Accepted {
                client: XServerFrontendClientId::from_raw(1),
                transaction: sophia_protocol::TransactionId::from_raw(u64::from(serial) + 1),
                window: XResourceId {
                    local: sophia_protocol::AuthorityLocalId::new(1, 1),
                },
                pixmap: XResourceId {
                    local: sophia_protocol::AuthorityLocalId::new(2, 1),
                },
                serial,
                pending: 1,
            },
            u64::from(serial),
        );
    }
    assert_eq!(window.attempted, 1000);
    assert_eq!(window.accepted, 1000);
    assert_eq!(window.emitted, 0, "no high-rate success was formatted");
    assert_eq!(window.coalesced, 1000 - CAPACITY as u64);
    let times = (window.next..CAPACITY)
        .chain(0..window.next)
        .map(|i| window.recent[i].unwrap().1)
        .collect::<Vec<_>>();
    assert_eq!(times, (1000 - CAPACITY as u64..1000).collect::<Vec<_>>());
}
