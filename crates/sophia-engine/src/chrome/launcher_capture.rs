use sophia_protocol::{DeviceId, InputEventKind, InputEventPacket, OutputId, Point, Rect, SeatId};
use std::collections::BTreeSet;
mod native;
pub use native::NativeLauncherCommand;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LauncherInput {
    Text(String),
    Backspace,
    Clear,
    Next,
    Previous,
    Activate(u16),
    Dismiss,
    Native {
        binding: sophia_protocol::NativeLauncherBinding,
        command: NativeLauncherCommand,
    },
    CaptureCapacityExceeded,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LauncherInputEvent {
    pub output: OutputId,
    pub presentation_epoch: u64,
    pub input: LauncherInput,
}

/// Modal input is armed only after retirement. A query or navigation edit
/// immediately disarms activation until matching results have retired.
#[derive(Default)]
pub struct LauncherCapture {
    identity: Option<(OutputId, u64)>,
    native: Option<sophia_protocol::NativeLauncherBinding>,
    targets: Vec<(u16, Rect)>,
    selected: u16,
    blocked: bool,
    swallowed: BTreeSet<(SeatId, DeviceId, bool, u32)>,
    pointer_press: Option<(u16, u64)>,
    wheel: i32,
}
impl LauncherCapture {
    pub fn present(
        &mut self,
        identity: Option<(OutputId, u64)>,
        selected: u16,
        targets: &[(u16, Rect)],
        blocked: bool,
    ) {
        self.native = None;
        if identity != self.identity {
            self.pointer_press = None;
            self.wheel = 0;
            self.blocked = blocked;
        } else {
            self.blocked |= blocked;
        }
        self.identity = identity;
        self.selected = selected;
        self.targets.clear();
        self.targets.extend(
            targets
                .iter()
                .copied()
                .take(sophia_protocol::SOPHIA_SHELL_MAX_LAUNCHER_ROWS),
        );
    }
    pub fn active(&self) -> bool {
        self.identity.is_some() || self.native.is_some()
    }
    pub fn route(
        &mut self,
        event: &InputEventPacket,
        text: Option<&str>,
        pointer: Option<Point>,
        clear: bool,
        command_modifier: bool,
    ) -> (bool, Option<LauncherInputEvent>) {
        if let Some(binding) = self.native {
            return self.route_native(binding, event, text, clear, command_modifier);
        }
        let id = match event.kind {
            InputEventKind::Key { keycode, pressed } => Some((false, keycode, pressed)),
            InputEventKind::PointerButton { button, pressed } => Some((true, button, pressed)),
            _ => None,
        };
        if let Some((button, code, pressed)) = id {
            if !button && matches!(code, 29 | 42 | 54 | 56 | 97 | 100 | 125 | 126) {
                return (false, None);
            }
            let key = (event.seat, event.device, button, code);
            if !pressed {
                let consumed = self.swallowed.remove(&key);
                if button
                    && consumed
                    && let (Some((slot, epoch)), Some((output, current))) =
                        (self.pointer_press.take(), self.identity)
                    && epoch == current
                    && !self.blocked
                    && self.hit(pointer) == Some(slot)
                {
                    self.blocked = true;
                    return (
                        true,
                        Some(LauncherInputEvent {
                            output,
                            presentation_epoch: epoch,
                            input: LauncherInput::Activate(slot),
                        }),
                    );
                }
                return (consumed, None);
            }
            if self.swallowed.contains(&key) {
                return (true, None);
            }
            let Some((output, epoch)) = self.identity else {
                return (false, None);
            };
            if self.swallowed.len() < 1024 {
                self.swallowed.insert(key);
            }
            if button {
                if code == 272 && !self.blocked {
                    self.pointer_press = self.hit(pointer).map(|slot| (slot, epoch));
                }
                return (true, None);
            }
            let input = match code {
                1 => Some(LauncherInput::Dismiss),
                14 => Some(LauncherInput::Backspace),
                103 if !command_modifier => Some(LauncherInput::Previous),
                108 if !command_modifier => Some(LauncherInput::Next),
                28 | 96
                    if !command_modifier
                        && !self.blocked
                        && self.targets.iter().any(|(s, _)| *s == self.selected) =>
                {
                    Some(LauncherInput::Activate(self.selected))
                }
                _ if clear => Some(LauncherInput::Clear),
                _ => text
                    .filter(|t| !t.is_empty() && sophia_protocol::shell_launcher_text_valid(t, 256))
                    .map(|t| LauncherInput::Text(t.to_owned())),
            };
            if input.is_some() {
                self.blocked = true;
            }
            return (
                true,
                input.map(|input| LauncherInputEvent {
                    output,
                    presentation_epoch: epoch,
                    input,
                }),
            );
        }
        if let (Some((output, epoch)), InputEventKind::PointerAxis { vertical_v120, .. }) =
            (self.identity, event.kind)
        {
            self.wheel = self.wheel.saturating_add(vertical_v120);
            let input = if self.wheel >= 120 {
                Some(LauncherInput::Next)
            } else if self.wheel <= -120 {
                Some(LauncherInput::Previous)
            } else {
                None
            };
            if input.is_some() {
                self.wheel = 0;
                self.blocked = true;
            }
            return (
                true,
                input.map(|input| LauncherInputEvent {
                    output,
                    presentation_epoch: epoch,
                    input,
                }),
            );
        }
        (
            self.active() && matches!(event.kind, InputEventKind::PointerMotion),
            None,
        )
    }
    fn hit(&self, pointer: Option<Point>) -> Option<u16> {
        let p = pointer?;
        self.targets
            .iter()
            .find(|(_, r)| {
                p.x >= f64::from(r.x)
                    && p.y >= f64::from(r.y)
                    && p.x < f64::from(r.x) + f64::from(r.width)
                    && p.y < f64::from(r.y) + f64::from(r.height)
            })
            .map(|(slot, _)| *slot)
    }
}
