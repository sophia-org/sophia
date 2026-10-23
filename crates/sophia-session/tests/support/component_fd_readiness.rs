use super::*;

const WIRE: FdRole = FdRole::Transport(ShellPollRole::Wire);
const SOCKET: FdRole = FdRole::Transport(ShellPollRole::ImageSocket);
const WAKE: FdRole = FdRole::Transport(ShellPollRole::WireWake);

#[test]
fn roles_count_once_per_wait_and_may_overlap() {
    let counts = ComponentFdReadiness::default()
        // Two components' wires fire with a worker: wire once, both roles.
        .add_wait([(WIRE, true), (FdRole::ImageWorker, true), (WIRE, true)])
        // A quiet or timed-out wait counts only as a wait.
        .add_wait([(WIRE, false), (SOCKET, false)])
        .add_wait([(SOCKET, true), (FdRole::ImageWorker, false)])
        // A journal self-notification is not a peer exchange.
        .add_wait([(WAKE, true), (WIRE, false)]);
    assert_eq!(
        counts,
        ComponentFdReadiness {
            waits: 4,
            any: 3,
            wire_wake: 1,
            wire: 1,
            image_socket: 1,
            image_worker: 1,
            ..Default::default()
        }
    );
}

#[test]
fn a_wait_without_component_descriptors_is_not_counted() {
    let counts = ComponentFdReadiness::default().add_wait([]);
    assert_eq!(counts, ComponentFdReadiness::default());
}

#[test]
fn counters_saturate() {
    let full = ComponentFdReadiness {
        waits: u64::MAX,
        any: u64::MAX,
        wire: u64::MAX,
        ..Default::default()
    };
    assert_eq!(full.add_wait([(WIRE, true)]), full);
}
