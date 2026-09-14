use std::collections::{BTreeMap, BTreeSet};

use sophia_protocol::{
    ContentAllocationId, ContentGrant, ContentLogicalRect, ContentOutputId, ContentPixelRect,
    DeviceId, InputEventKind, Point, SeatId,
};

pub const MAX_CONTENT_CAPTURE_SEATS: usize = 16;
pub const MAX_CONTENT_SUPPRESSED_BUTTONS_PER_SEAT: usize = 32;
const MAX_TRACKED_BUTTON_CODE: u32 = 0x2ff;
const BUTTON_WORDS: usize = (MAX_TRACKED_BUTTON_CODE as usize + 64) / 64;
const BUTTON_CODES: usize = MAX_TRACKED_BUTTON_CODE as usize + 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresentedContentTarget {
    pub grant: ContentGrant,
    pub output: ContentOutputId,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub interaction_generation: u64,
    pub allocation: ContentAllocationId,
    pub allocation_logical: ContentLogicalRect,
    pub allocation_pixel: ContentPixelRect,
    pub target_id: u64,
    pub target_generation: u64,
    pub action_id: u64,
    pub bounds_px: ContentPixelRect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresentedContentBinding {
    pub output: ContentOutputId,
    pub candidate_generation: u64,
    pub presentation_epoch: u64,
    pub interaction_generation: u64,
    pub targets: Vec<PresentedContentTarget>,
    pub allocations: Vec<(ContentAllocationId, ContentLogicalRect, ContentPixelRect)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ContentCapture {
    device: DeviceId,
    button: u32,
    target: PresentedContentTarget,
    valid: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QuarantinedDevice {
    pressed: [u64; BUTTON_WORDS],
    clears_only_on_removal: bool,
}

impl Default for QuarantinedDevice {
    fn default() -> Self {
        Self {
            pressed: [0; BUTTON_WORDS],
            clears_only_on_removal: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GlobalQuarantine {
    pressed: [u8; BUTTON_CODES],
    clears_only_on_reset: bool,
}

impl Default for GlobalQuarantine {
    fn default() -> Self {
        Self {
            pressed: [0; BUTTON_CODES],
            clears_only_on_reset: false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContentCaptureState {
    captures: BTreeMap<SeatId, ContentCapture>,
    suppressed: BTreeSet<(SeatId, DeviceId, u32)>,
    quarantined: BTreeMap<DeviceId, QuarantinedDevice>,
    global_quarantine: Option<GlobalQuarantine>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContentPointerDisposition {
    Pass,
    Consumed,
    Captured,
    Cancelled,
    Activated(PresentedContentTarget),
}

impl ContentCaptureState {
    pub fn revoke_targets(&mut self) {
        for capture in self.captures.values_mut() {
            capture.valid = false;
        }
    }

    pub fn remove_device(&mut self, device: DeviceId) {
        self.captures.retain(|_, capture| capture.device != device);
        self.suppressed
            .retain(|(_, suppressed_device, _)| *suppressed_device != device);
        self.quarantined.remove(&device);
    }

    pub fn reset(&mut self) {
        self.captures.clear();
        self.suppressed.clear();
        self.quarantined.clear();
        self.global_quarantine = None;
    }

    fn remember_suppressed(&mut self, seat: SeatId, device: DeviceId, button: u32) {
        let seat_count = self
            .suppressed
            .iter()
            .filter(|(candidate, _, _)| *candidate == seat)
            .count();
        if seat_count >= MAX_CONTENT_SUPPRESSED_BUTTONS_PER_SEAT {
            self.quarantine(device, button, true);
            return;
        }
        self.suppressed.insert((seat, device, button));
    }

    fn quarantine(&mut self, device: DeviceId, button: u32, pressed: bool) {
        if self.quarantined.len() >= MAX_CONTENT_CAPTURE_SEATS
            && !self.quarantined.contains_key(&device)
        {
            self.enter_global_quarantine(button, pressed);
            return;
        }
        let state = self.quarantined.entry(device).or_default();
        if button > MAX_TRACKED_BUTTON_CODE {
            state.clears_only_on_removal = true;
            return;
        }
        let index = button as usize / 64;
        let mask = 1_u64 << (button % 64);
        if pressed {
            state.pressed[index] |= mask;
        } else {
            state.pressed[index] &= !mask;
        }
        if !state.clears_only_on_removal && state.pressed.iter().all(|word| *word == 0) {
            self.quarantined.remove(&device);
        }
    }

    fn enter_global_quarantine(&mut self, button: u32, pressed: bool) {
        let mut global = GlobalQuarantine::default();
        for capture in self.captures.values() {
            global_button(&mut global, capture.button, true);
        }
        for (_, _, suppressed) in &self.suppressed {
            global_button(&mut global, *suppressed, true);
        }
        for quarantined in self.quarantined.values() {
            global.clears_only_on_reset |= quarantined.clears_only_on_removal;
            for (word_index, word) in quarantined.pressed.iter().enumerate() {
                for bit in 0..64 {
                    if word & (1_u64 << bit) != 0 {
                        let code = word_index * 64 + bit;
                        if code < BUTTON_CODES {
                            global.pressed[code] = global.pressed[code].saturating_add(1);
                        }
                    }
                }
            }
        }
        global_button(&mut global, button, pressed);
        self.global_quarantine = Some(global);
    }

    fn route_existing_sequence(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        kind: InputEventKind,
        _application_owned: bool,
    ) -> Option<ContentPointerDisposition> {
        let InputEventKind::PointerButton { button, pressed } = kind else {
            return (self.global_quarantine.is_some() || self.quarantined.contains_key(&device))
                .then_some(ContentPointerDisposition::Consumed);
        };
        if let Some(global) = &mut self.global_quarantine {
            global_button(global, button, pressed);
            if !global.clears_only_on_reset && global.pressed.iter().all(|count| *count == 0) {
                self.global_quarantine = None;
            }
            return Some(ContentPointerDisposition::Consumed);
        }
        if self.quarantined.contains_key(&device) {
            self.quarantine(device, button, pressed);
            return Some(ContentPointerDisposition::Consumed);
        }
        if self.suppressed.contains(&(seat, device, button)) {
            if !pressed {
                self.suppressed.remove(&(seat, device, button));
            }
            return Some(ContentPointerDisposition::Consumed);
        }
        None
    }
}

fn global_button(global: &mut GlobalQuarantine, button: u32, pressed: bool) {
    let Ok(index) = usize::try_from(button) else {
        global.clears_only_on_reset = true;
        return;
    };
    let Some(count) = global.pressed.get_mut(index) else {
        global.clears_only_on_reset = true;
        return;
    };
    if pressed {
        *count = count.saturating_add(1);
    } else {
        *count = count.saturating_sub(1);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_content_pointer_event(
    state: &mut ContentCaptureState,
    seat: SeatId,
    device: DeviceId,
    kind: InputEventKind,
    position: Option<Point>,
    binding: Option<&PresentedContentBinding>,
    application_owned: bool,
) -> ContentPointerDisposition {
    if let Some(disposition) = state.route_existing_sequence(seat, device, kind, application_owned)
    {
        return disposition;
    }
    if let Some(mut capture) = state.captures.remove(&seat) {
        let current = binding.and_then(|binding| {
            binding
                .targets
                .iter()
                .find(|target| same_target(target, &capture.target))
        });
        capture.valid &= current == Some(&capture.target);
        let InputEventKind::PointerButton { button, pressed } = kind else {
            state.captures.insert(seat, capture);
            return ContentPointerDisposition::Consumed;
        };
        if device != capture.device || button != capture.button {
            if pressed {
                state.remember_suppressed(seat, device, button);
            }
            state.captures.insert(seat, capture);
            return ContentPointerDisposition::Consumed;
        }
        if pressed {
            state.captures.insert(seat, capture);
            return ContentPointerDisposition::Consumed;
        }
        if !capture.valid {
            return ContentPointerDisposition::Cancelled;
        }
        return if position.is_some_and(|point| target_contains(&capture.target, point)) {
            ContentPointerDisposition::Activated(capture.target)
        } else {
            ContentPointerDisposition::Consumed
        };
    }
    if application_owned {
        return ContentPointerDisposition::Pass;
    }
    let Some(binding) = binding else {
        return ContentPointerDisposition::Pass;
    };
    let Some(position) = position else {
        return ContentPointerDisposition::Pass;
    };
    let target = binding
        .targets
        .iter()
        .find(|target| target_contains(target, position));
    let occluded = binding
        .allocations
        .iter()
        .any(|(_, logical, _)| logical_contains(*logical, position));
    if target.is_none() && !occluded {
        return ContentPointerDisposition::Pass;
    }
    let InputEventKind::PointerButton { button, pressed } = kind else {
        return ContentPointerDisposition::Consumed;
    };
    if !pressed {
        return ContentPointerDisposition::Consumed;
    }
    state.remember_suppressed(seat, device, button);
    let Some(target) = target else {
        return ContentPointerDisposition::Consumed;
    };
    if button != crate::CHROME_PRIMARY_BUTTON || state.captures.len() >= MAX_CONTENT_CAPTURE_SEATS {
        return ContentPointerDisposition::Consumed;
    }
    state.suppressed.remove(&(seat, device, button));
    state.captures.insert(
        seat,
        ContentCapture {
            device,
            button,
            target: target.clone(),
            valid: true,
        },
    );
    ContentPointerDisposition::Captured
}

fn same_target(left: &PresentedContentTarget, right: &PresentedContentTarget) -> bool {
    left.grant == right.grant
        && left.output == right.output
        && left.candidate_generation == right.candidate_generation
        && left.presentation_epoch == right.presentation_epoch
        && left.interaction_generation == right.interaction_generation
        && left.allocation == right.allocation
        && left.target_id == right.target_id
        && left.target_generation == right.target_generation
        && left.action_id == right.action_id
}

fn logical_contains(rect: ContentLogicalRect, point: Point) -> bool {
    point.x >= f64::from(rect.x)
        && point.y >= f64::from(rect.y)
        && point.x < f64::from(rect.x) + f64::from(rect.width)
        && point.y < f64::from(rect.y) + f64::from(rect.height)
}

fn target_contains(target: &PresentedContentTarget, point: Point) -> bool {
    let logical = target.allocation_logical;
    let physical = target.allocation_pixel;
    if !logical_contains(logical, point) || logical.width == 0 || logical.height == 0 {
        return false;
    }
    let x = ((point.x - f64::from(logical.x)) * f64::from(physical.width)
        / f64::from(logical.width))
    .floor();
    let y = ((point.y - f64::from(logical.y)) * f64::from(physical.height)
        / f64::from(logical.height))
    .floor();
    let bounds = target.bounds_px;
    x >= f64::from(bounds.x)
        && y >= f64::from(bounds.y)
        && x < f64::from(bounds.x) + f64::from(bounds.width)
        && y < f64::from(bounds.y) + f64::from(bounds.height)
}
