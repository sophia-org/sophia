use super::OverviewHitTarget;
use sophia_protocol::{
    DeviceId, InputEventKind, InputEventPacket, OutputId, Point, SeatId, ShellOverviewOperation,
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverviewInputPresentation {
    pub output: OutputId,
    pub epoch: u64,
    pub catalog: u64,
    pub targets: Vec<OverviewHitTarget>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverviewInput {
    pub output: OutputId,
    pub epoch: u64,
    pub catalog: u64,
    pub operation: ShellOverviewOperation,
    pub workspace: u16,
    pub window: u16,
}

/// Retirement grants input authority. Revocation leaves swallowed release debt
/// intact so closing a modal surface cannot deliver its releases to a client.
#[derive(Default)]
pub struct OverviewCapture {
    presented: Option<OverviewInputPresentation>,
    swallowed: BTreeSet<(SeatId, DeviceId, bool, u32)>,
    closing: bool,
}

impl OverviewCapture {
    pub fn active(&self) -> bool {
        self.presented.is_some()
    }

    pub fn present(&mut self, presentation: Option<OverviewInputPresentation>) {
        if self.presented != presentation {
            self.closing = false;
        }
        self.presented = presentation;
    }

    pub fn route(
        &mut self,
        event: &InputEventPacket,
        point: Point,
    ) -> Result<(bool, Option<OverviewInput>), &'static str> {
        let key = match event.kind {
            InputEventKind::Key { keycode, pressed } => {
                // Modifier delivery stays balanced with presses preceding entry.
                if matches!(keycode, 29 | 42 | 54 | 56 | 97 | 100 | 125 | 126) {
                    return Ok((false, None));
                }
                Some((false, keycode, pressed))
            }
            InputEventKind::PointerButton { button, pressed } => Some((true, button, pressed)),
            _ => None,
        };
        if let Some((button, code, pressed)) = key {
            let id = (event.seat, event.device, button, code);
            if !pressed {
                return Ok((self.swallowed.remove(&id), None));
            }
            if self.swallowed.contains(&id) {
                return Ok((true, None));
            }
            let Some(presented) = &self.presented else {
                return Ok((false, None));
            };
            if self.swallowed.len() >= 1024 {
                return Err("overview capture capacity exceeded");
            }
            self.swallowed.insert(id);
            if self.closing {
                return Ok((true, None));
            }
            let mut workspace = 0;
            let mut window = 0;
            let operation = if button {
                if code != 272 {
                    return Ok((true, None));
                }
                let target = presented.targets.iter().rev().find(|target| {
                    let g = target.geometry;
                    point.x >= f64::from(g.x)
                        && point.y >= f64::from(g.y)
                        && point.x < f64::from(g.x) + f64::from(g.width)
                        && point.y < f64::from(g.y) + f64::from(g.height)
                });
                let Some(target) = target else {
                    return Ok((true, None));
                };
                workspace = target.workspace;
                window = target.window;
                ShellOverviewOperation::Pick
            } else {
                match code {
                    1 => ShellOverviewOperation::Dismiss,
                    28 => ShellOverviewOperation::Accept,
                    105 | 35 => ShellOverviewOperation::Left,
                    106 | 38 => ShellOverviewOperation::Right,
                    103 | 37 => ShellOverviewOperation::Up,
                    108 | 36 => ShellOverviewOperation::Down,
                    102 => ShellOverviewOperation::First,
                    107 => ShellOverviewOperation::Last,
                    _ => return Ok((true, None)),
                }
            };
            self.closing = matches!(
                operation,
                ShellOverviewOperation::Dismiss
                    | ShellOverviewOperation::Accept
                    | ShellOverviewOperation::Pick
            );
            return Ok((
                true,
                Some(OverviewInput {
                    output: presented.output,
                    epoch: presented.epoch,
                    catalog: presented.catalog,
                    operation,
                    workspace,
                    window,
                }),
            ));
        }
        Ok((
            self.presented.is_some()
                && matches!(
                    event.kind,
                    InputEventKind::PointerMotion | InputEventKind::PointerAxis { .. }
                ),
            None,
        ))
    }
}
