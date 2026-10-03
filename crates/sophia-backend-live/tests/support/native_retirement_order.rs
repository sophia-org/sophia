use super::order_collected_retirements;
use sophia_protocol::OutputId;

#[test]
fn collected_retirements_use_the_permission_heads_ust_across_cards() {
    let a = OutputId::from_raw(1);
    let b = OutputId::from_raw(2);
    let c = OutputId::from_raw(3);
    let d = OutputId::from_raw(4);
    // A's fast sibling arrived first, but only its slow primary grants
    // logical retirement. B and C are on other card fds, read in poll order.
    let events = [
        (a, false, 10),
        (a, true, 50),
        (c, true, 30),
        (b, true, 30),
        (d, false, 20),
    ];
    assert_eq!(
        order_collected_retirements(events.into_iter()),
        [d, b, c, a]
    );
    // Reversing sibling callback collection cannot replace primary timing.
    assert_eq!(
        order_collected_retirements(events.into_iter().rev()),
        [d, b, c, a]
    );
}
