//! Semantic capture bound to an actual native focus lease, never a row guess.
use super::*;
use sophia_protocol::{NativeLauncherBinding, NativeLauncherInputKind as Kind};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeLauncherCommand {
    Input { kind: Kind, text: String },
    Dismiss,
}
impl LauncherCapture {
    pub fn present_native(&mut self, binding: Option<NativeLauncherBinding>) {
        if self.native != binding {
            self.wheel = 0;
        }
        self.native = binding;
        self.identity = None;
        self.targets.clear();
        self.pointer_press = None;
        // Preserve swallowed sequences through replacement/revocation.
    }
    pub const fn native_active(&self) -> bool {
        self.native.is_some()
    }

    pub const fn native_binding(&self) -> Option<NativeLauncherBinding> {
        self.native
    }

    /// Content target resolution runs first. Only otherwise unclaimed pointer
    /// events reach this modal barrier; no application click-through is allowed.
    pub fn route_native_pointer_fallback(
        &mut self,
        event: &InputEventPacket,
    ) -> Result<(bool, Option<LauncherInputEvent>), &'static str> {
        let Some(binding) = self.native else {
            return Ok((false, None));
        };
        if let InputEventKind::PointerButton { button, pressed } = event.kind {
            let key = (event.seat, event.device, true, button);
            if pressed {
                if self.swallowed.len() >= 1024 && !self.swallowed.contains(&key) {
                    return Err("native launcher capture capacity exhausted");
                }
                self.swallowed.insert(key);
            } else {
                self.swallowed.remove(&key);
            }
        }
        let command = if let InputEventKind::PointerAxis { vertical_v120, .. } = event.kind {
            self.wheel = self.wheel.saturating_add(vertical_v120);
            let kind = if self.wheel >= 120 {
                Some(Kind::Next)
            } else if self.wheel <= -120 {
                Some(Kind::Previous)
            } else {
                None
            };
            if kind.is_some() {
                self.wheel = 0;
            }
            kind.map(input)
        } else {
            None
        };
        Ok((
            matches!(
                event.kind,
                InputEventKind::PointerButton { .. }
                    | InputEventKind::PointerMotion
                    | InputEventKind::PointerAxis { .. }
            ),
            command.map(|command| LauncherInputEvent {
                output: OutputId::from_raw(binding.output.id),
                presentation_epoch: binding.presentation_epoch,
                input: LauncherInput::Native { binding, command },
            }),
        ))
    }

    pub(super) fn route_native(
        &mut self,
        binding: NativeLauncherBinding,
        event: &InputEventPacket,
        text: Option<&str>,
        clear: bool,
        command_modifier: bool,
    ) -> (bool, Option<LauncherInputEvent>) {
        let command = match event.kind {
            InputEventKind::Key { keycode, pressed } => {
                if matches!(keycode, 29 | 42 | 54 | 56 | 97 | 100 | 125 | 126) {
                    return (false, None);
                }
                let key = (event.seat, event.device, false, keycode);
                if !pressed {
                    return (self.swallowed.remove(&key), None);
                }
                if self.swallowed.contains(&key) {
                    return (true, None);
                }
                if self.swallowed.len() >= 1024 {
                    return (
                        true,
                        Some(LauncherInputEvent {
                            output: OutputId::from_raw(binding.output.id),
                            presentation_epoch: binding.presentation_epoch,
                            input: LauncherInput::CaptureCapacityExceeded,
                        }),
                    );
                }
                self.swallowed.insert(key);
                if keycode == 1 {
                    Some(NativeLauncherCommand::Dismiss)
                } else if clear {
                    Some(input(Kind::DeleteToStart))
                } else if command_modifier {
                    None
                } else {
                    match keycode {
                        28 | 96 => Some(input(Kind::Accept)),
                        14 => Some(input(Kind::Backspace)),
                        111 => Some(input(Kind::Delete)),
                        105 => Some(input(Kind::Left)),
                        106 => Some(input(Kind::Right)),
                        102 => Some(input(Kind::Home)),
                        107 => Some(input(Kind::End)),
                        103 => Some(input(Kind::Previous)),
                        108 => Some(input(Kind::Next)),
                        104 => Some(input(Kind::PagePrevious)),
                        109 => Some(input(Kind::PageNext)),
                        _ => text
                            .filter(|v| {
                                !v.is_empty() && sophia_protocol::shell_launcher_text_valid(v,
                            sophia_protocol::SOPHIA_SHELL_NATIVE_LAUNCHER_MAX_TEXT_BYTES)
                            })
                            .map(|v| NativeLauncherCommand::Input {
                                kind: Kind::Text,
                                text: v.to_owned(),
                            }),
                    }
                }
            }
            InputEventKind::PointerButton { button, pressed } => {
                let key = (event.seat, event.device, true, button);
                return if pressed {
                    (self.swallowed.contains(&key), None)
                } else {
                    (self.swallowed.remove(&key), None)
                };
            }
            _ => return (false, None),
        };
        (
            true,
            command.map(|command| LauncherInputEvent {
                output: OutputId::from_raw(binding.output.id),
                presentation_epoch: binding.presentation_epoch,
                input: LauncherInput::Native { binding, command },
            }),
        )
    }
}
fn input(kind: Kind) -> NativeLauncherCommand {
    NativeLauncherCommand::Input {
        kind,
        text: String::new(),
    }
}
