use super::events::{
    TopologyEventSource, processed_membership_event, topology_event_requires_rescan,
};
use crate::drm::seat_inventory::{SeatDrmCard, policy::is_node_name};
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// A failed comparison must not erase identities needed for prompt revocation.
/// The shared admission inventory bounds this set before it is installed here.
pub(super) struct SeatTopologyScope {
    cards: Vec<SeatDrmCard>,
    dirty: bool,
    retry_at: Option<Instant>,
}

impl SeatTopologyScope {
    pub(super) fn new(cards: Vec<SeatDrmCard>) -> Self {
        Self {
            cards,
            dirty: false,
            retry_at: None,
        }
    }

    /// Uses event paths, not lookups of possibly removed or reassigned nodes.
    fn contains(&self, path: &Path) -> bool {
        self.cards.iter().any(|card| {
            if path == card.sysfs_node {
                return true;
            }
            let Some(name) = path.file_name() else {
                return false;
            };
            if path.parent() == Some(card.sysfs_node.as_path()) {
                let Some(card_name) = card.sysfs_node.file_name().and_then(|name| name.to_str())
                else {
                    return false;
                };
                return name.to_str().is_some_and(|name| {
                    name.strip_prefix(card_name)
                        .is_some_and(|suffix| suffix.starts_with('-'))
                });
            }
            is_node_name(name, "renderD") && path.parent() == card.sysfs_node.parent()
        })
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(super) fn observe(
        &mut self,
        source: TopologyEventSource,
        action: udev::EventType,
        name: &OsStr,
        path: &Path,
        hotplug: bool,
    ) -> bool {
        if source == TopologyEventSource::Processed && processed_membership_event(action, name) {
            // Re-read the settled inventory rather than trusting event seat
            // properties, which may be obsolete by the time they are consumed.
            self.mark_dirty();
        }
        topology_event_requires_rescan(source, action, name, hotplug) && self.contains(path)
    }

    pub(super) fn refresh(
        &mut self,
        now: Instant,
        discover: impl FnOnce() -> io::Result<Vec<SeatDrmCard>>,
    ) -> io::Result<bool> {
        if !self.dirty || self.retry_at.is_some_and(|retry| now < retry) {
            return Ok(false);
        }
        let current = match discover() {
            Ok(current) => current,
            Err(error) => {
                self.retry_at = Some(now + Duration::from_millis(250));
                return Err(error);
            }
        };
        self.dirty = false;
        self.retry_at = None;
        let changed = self.cards != current;
        self.cards = current;
        Ok(changed)
    }
}

#[path = "../../../tests/support/seat_topology_scope.rs"]
mod tests;
