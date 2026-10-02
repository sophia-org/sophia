//! The shortcut shapes a registry matches (t277 D): immediate chords, hold
//! variants beside them, lone modifier taps, key sequences, and the leaders
//! on sequence prefixes. Session resolves a profile into a plan; this module
//! refuses any plan whose shapes could not all be matched unambiguously.

use super::WmShortcutRegistryError;
use super::ledger::modifier_bit;
use crate::prelude::*;
use sophia_protocol::{WM_MAX_BINDINGS, WmActionId, WmBindingRegistration, WmModifierMask};

/// One step of a key sequence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct WmKeyStep {
    pub keycode: u32,
    pub modifiers: u32,
}

/// A lone modifier tap: `modifier` is one WmModifierMask class bit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmModifierTapBinding {
    pub modifier: u32,
    pub action: WmActionId,
}

/// The hold variant of a chord, fired when it is still held after `hold_ms`.
/// An immediate binding on the same chord is its tap variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmHoldBinding {
    pub step: WmKeyStep,
    pub hold_ms: u32,
    pub action: WmActionId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmSequenceBinding {
    pub steps: Vec<WmKeyStep>,
    pub action: WmActionId,
}

/// An action fired when a pending sequence reaches `steps`, a proper prefix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmSequenceLeader {
    pub steps: Vec<WmKeyStep>,
    pub action: WmActionId,
}

/// The modifier-tap window and the sequence step timeout, in milliseconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmShortcutTiming {
    pub tap_ms: u32,
    pub sequence_ms: u32,
}

impl Default for WmShortcutTiming {
    fn default() -> Self {
        Self {
            tap_ms: 400,
            sequence_ms: 1000,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WmShortcutPlan {
    pub immediate: Vec<WmBindingRegistration>,
    pub taps: Vec<WmModifierTapBinding>,
    pub holds: Vec<WmHoldBinding>,
    pub sequences: Vec<WmSequenceBinding>,
    pub leaders: Vec<WmSequenceLeader>,
    pub timing: WmShortcutTiming,
}

pub(crate) type Chord = (u32, u32);

/// A node of the sequence trie. A leaf has an action and no children.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Node {
    pub(crate) children: BTreeMap<Chord, usize>,
    pub(crate) leaf: Option<WmActionId>,
    pub(crate) leader: Option<WmActionId>,
    /// Every leaf and leader action at or below this node, sorted.
    pub(crate) reachable: Vec<WmActionId>,
}

/// A plan, validated and indexed for matching.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Shapes {
    pub(crate) immediate: BTreeMap<Chord, WmActionId>,
    pub(crate) holds: BTreeMap<Chord, (WmActionId, u32)>,
    pub(crate) taps: BTreeMap<u32, WmActionId>,
    /// Sequence first steps; node 0 is an unused root.
    pub(crate) roots: BTreeMap<Chord, usize>,
    pub(crate) nodes: Vec<Node>,
    pub(crate) timing: WmShortcutTiming,
    pub(crate) count: usize,
}

const HOLD_MS: core::ops::RangeInclusive<u32> = 100..=5000;
const TAP_MS: core::ops::RangeInclusive<u32> = 50..=2000;
const SEQUENCE_MS: core::ops::RangeInclusive<u32> = 200..=10000;
const MAX_STEPS: usize = 4;
const KEYCODE_LIMIT: u32 = 0x300;
const ESCAPE: u32 = 1;

/// Whether `modifier` is exactly one modifier class.
fn modifier_class(modifier: u32) -> bool {
    [
        WmModifierMask::SHIFT,
        WmModifierMask::CONTROL,
        WmModifierMask::ALT,
        WmModifierMask::SUPER,
    ]
    .contains(&modifier)
}

/// The rules every matched key chord obeys, in any step of any shape.
fn check_step(keycode: u32, modifiers: u32) -> Result<Chord, WmShortcutRegistryError> {
    if keycode == 0 || keycode >= KEYCODE_LIMIT {
        return Err("invalid WM binding");
    }
    // A modifier key only ever changes the mask; a lone modifier is bound
    // as a modifier tap.
    if modifier_bit(keycode) != 0 {
        return Err("modifier key is not a WM key step");
    }
    if modifiers & !WmModifierMask::SUPPORTED != 0 {
        return Err("unsupported WM modifier");
    }
    let control_alt = WmModifierMask::CONTROL | WmModifierMask::ALT;
    if modifiers & control_alt == control_alt {
        // Ctrl-Alt-Backspace belongs to emergency recovery, and Ctrl-Alt-F1
        // through F12 to virtual-terminal switching, with any further
        // modifiers. Neither is available to a policy client.
        if keycode == 14 {
            return Err("reserved emergency chord");
        }
        if matches!(keycode, 59..=68 | 87 | 88) {
            return Err("reserved virtual-terminal chord");
        }
    }
    Ok((keycode, modifiers))
}

fn check_action(action: WmActionId) -> Result<WmActionId, WmShortcutRegistryError> {
    if action.is_valid() {
        Ok(action)
    } else {
        Err("invalid WM binding")
    }
}

impl Shapes {
    pub(crate) fn build(plan: &WmShortcutPlan) -> Result<Self, WmShortcutRegistryError> {
        let count = plan.immediate.len()
            + plan.taps.len()
            + plan.holds.len()
            + plan.sequences.len()
            + plan.leaders.len();
        if count > WM_MAX_BINDINGS {
            return Err("too many WM bindings");
        }
        if !TAP_MS.contains(&plan.timing.tap_ms) || !SEQUENCE_MS.contains(&plan.timing.sequence_ms)
        {
            return Err("invalid WM shortcut timing");
        }
        let mut shapes = Self {
            nodes: vec![Node::default()],
            timing: plan.timing,
            count,
            ..Self::default()
        };
        for binding in &plan.immediate {
            let chord = check_step(binding.keycode, binding.modifiers.bits)?;
            if shapes
                .immediate
                .insert(chord, check_action(binding.action)?)
                .is_some()
            {
                return Err("duplicate WM chord");
            }
        }
        for hold in &plan.holds {
            let chord = check_step(hold.step.keycode, hold.step.modifiers)?;
            if !HOLD_MS.contains(&hold.hold_ms) {
                return Err("invalid WM hold");
            }
            if shapes
                .holds
                .insert(chord, (check_action(hold.action)?, hold.hold_ms))
                .is_some()
            {
                return Err("duplicate WM chord");
            }
        }
        for tap in &plan.taps {
            if !modifier_class(tap.modifier) {
                return Err("invalid WM modifier tap");
            }
            if shapes
                .taps
                .insert(tap.modifier, check_action(tap.action)?)
                .is_some()
            {
                return Err("duplicate WM chord");
            }
        }
        // In step order, so the trie, and the node a pending sequence names,
        // depend only on which sequences are bound, not on their order.
        let mut sequences = plan.sequences.iter().collect::<Vec<_>>();
        sequences.sort_by(|left, right| left.steps.cmp(&right.steps));
        for sequence in sequences {
            shapes.insert_sequence(sequence)?;
        }
        for leader in &plan.leaders {
            shapes.insert_leader(leader)?;
        }
        shapes.check_leaders()?;
        shapes.collect_reachable(0);
        Ok(shapes)
    }

    fn insert_sequence(
        &mut self,
        sequence: &WmSequenceBinding,
    ) -> Result<(), WmShortcutRegistryError> {
        if !(2..=MAX_STEPS).contains(&sequence.steps.len())
            || sequence.steps[1..]
                .iter()
                .any(|step| step.keycode == ESCAPE)
        {
            return Err("invalid WM sequence");
        }
        let action = check_action(sequence.action)?;
        let first = check_step(sequence.steps[0].keycode, sequence.steps[0].modifiers)?;
        if self.immediate.contains_key(&first) || self.holds.contains_key(&first) {
            return Err("WM sequence extends a binding");
        }
        let mut node = match self.roots.get(&first) {
            Some(&node) => node,
            None => {
                self.nodes.push(Node::default());
                let node = self.nodes.len() - 1;
                self.roots.insert(first, node);
                node
            }
        };
        for step in &sequence.steps[1..] {
            let chord = check_step(step.keycode, step.modifiers)?;
            if self.nodes[node].leaf.is_some() {
                return Err("WM sequence extends a binding");
            }
            node = match self.nodes[node].children.get(&chord) {
                Some(&child) => child,
                None => {
                    self.nodes.push(Node::default());
                    let child = self.nodes.len() - 1;
                    self.nodes[node].children.insert(chord, child);
                    child
                }
            };
        }
        if self.nodes[node].leaf.is_some() {
            return Err("duplicate WM chord");
        }
        if !self.nodes[node].children.is_empty() {
            return Err("WM sequence extends a binding");
        }
        self.nodes[node].leaf = Some(action);
        Ok(())
    }

    fn insert_leader(&mut self, leader: &WmSequenceLeader) -> Result<(), WmShortcutRegistryError> {
        if !(1..MAX_STEPS).contains(&leader.steps.len()) {
            return Err("WM leader names no sequence prefix");
        }
        let action = check_action(leader.action)?;
        let first = check_step(leader.steps[0].keycode, leader.steps[0].modifiers)?;
        let mut node = *self
            .roots
            .get(&first)
            .ok_or("WM leader names no sequence prefix")?;
        for step in &leader.steps[1..] {
            let chord = check_step(step.keycode, step.modifiers)?;
            node = *self.nodes[node]
                .children
                .get(&chord)
                .ok_or("WM leader names no sequence prefix")?;
        }
        if self.nodes[node].children.is_empty() {
            return Err("WM leader names no sequence prefix");
        }
        if self.nodes[node].leader.replace(action).is_some() {
            return Err("duplicate WM leader");
        }
        Ok(())
    }

    /// At most one leader on any path, and a leader's action used by nothing
    /// else, so its chord is the only one with that action on a seat.
    fn check_leaders(&self) -> Result<(), WmShortcutRegistryError> {
        let mut uses = BTreeMap::<WmActionId, usize>::new();
        let actions = self
            .immediate
            .values()
            .chain(self.holds.values().map(|(action, _)| action))
            .chain(self.taps.values())
            .chain(self.nodes.iter().filter_map(|node| node.leaf.as_ref()))
            .chain(self.nodes.iter().filter_map(|node| node.leader.as_ref()));
        for action in actions {
            *uses.entry(*action).or_default() += 1;
        }
        for (index, node) in self.nodes.iter().enumerate() {
            if let Some(action) = node.leader {
                if uses[&action] > 1 {
                    return Err("WM leader action is reused");
                }
                if self.subtree_has_leader(index, false) {
                    return Err("nested WM leaders");
                }
            }
        }
        Ok(())
    }

    fn subtree_has_leader(&self, node: usize, include: bool) -> bool {
        (include && self.nodes[node].leader.is_some())
            || self.nodes[node]
                .children
                .values()
                .any(|&child| self.subtree_has_leader(child, true))
    }

    fn collect_reachable(&mut self, node: usize) -> Vec<WmActionId> {
        let mut reachable = Vec::new();
        let children = if node == 0 {
            self.roots.values().copied().collect::<Vec<_>>()
        } else {
            self.nodes[node].children.values().copied().collect()
        };
        for child in children {
            reachable.extend(self.collect_reachable(child));
        }
        reachable.extend(self.nodes[node].leaf);
        reachable.extend(self.nodes[node].leader);
        reachable.sort_unstable();
        reachable.dedup();
        self.nodes[node].reachable.clone_from(&reachable);
        reachable
    }
}
