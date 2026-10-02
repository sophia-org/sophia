use sophia_engine::{WmShortcutOutput, WmShortcutRouter};
use sophia_protocol::{DeviceId, SeatId, WmActionId};

/// Route one key event and accept it, as Session does when no capture takes
/// it. Returns whether the router consumed the event, the action it fired,
/// and every output in order, chord events included.
pub(crate) fn route_test_key(
    router: &mut WmShortcutRouter,
    seat: SeatId,
    device: DeviceId,
    keycode: u32,
    pressed: bool,
    now: u64,
) -> (bool, Option<WmActionId>, Vec<WmShortcutOutput>) {
    let event = router.key_event(seat, device, keycode, pressed, now);
    let consumed = event.consumed();
    let outputs = event.accept();
    let action = outputs.iter().rev().find_map(|output| match output {
        WmShortcutOutput::Activation(activation) => Some(activation.action),
        WmShortcutOutput::Chord(_) => None,
    });
    (consumed, action, outputs)
}
