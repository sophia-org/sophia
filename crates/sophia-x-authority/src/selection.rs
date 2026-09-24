use std::collections::BTreeMap;

use sophia_protocol::NamespaceId;

use crate::{XResourceId, XWindowTable};

pub type XAtom = u32;
pub type XTimestamp = u32;

pub const X_ATOM_NONE: XAtom = 0;
pub const MAX_CLIPBOARD_TEXT_HANDOFF_BYTES: usize = 64 * 1024;

/// `CurrentTime` on the wire. A client sends it to mean "whatever the server
/// time is right now", and a server never reports it back as a real instant.
pub const X_CURRENT_TIME: XTimestamp = 0;

/// Whether `later` names an instant after `earlier` on the server clock.
///
/// A server timestamp is a 32-bit millisecond counter that wraps about every
/// 49.7 days, so a plain `>` is wrong twice: right after a wrap every honest
/// new time looks older than everything before it, and a client can name a
/// time far in the future that would compare as older. X11 resolves this by
/// reading the difference as a signed quantity: two times are ordered by
/// which half of the counter's range separates them, which is correct for any
/// pair less than about 24.8 days apart and is the only ordering a wrapping
/// clock can support.
#[must_use]
pub fn x_time_is_after(later: XTimestamp, earlier: XTimestamp) -> bool {
    later != earlier && later.wrapping_sub(earlier) < 0x8000_0000
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XSelectionChangeKind {
    SetOwner,
    ClearOwner,
    SelectionWindowDestroyed,
    SelectionClientClosed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XSelectionEvent {
    pub selection: XAtom,
    pub owner: Option<XResourceId>,
    pub timestamp: XTimestamp,
    pub selection_timestamp: XTimestamp,
    pub kind: XSelectionChangeKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XSelectionOwnerRecord {
    pub selection: XAtom,
    pub namespace: Option<NamespaceId>,
    pub owner: Option<XResourceId>,
    pub generation: u64,
    pub timestamp: XTimestamp,
    pub selection_timestamp: XTimestamp,
    /// The client that made the request, when known: ownership ends with
    /// that client's connection even when the owner window is another
    /// client's and survives it (t226).
    pub requester: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XSelectionOwnerUpdate {
    pub previous: Option<XSelectionOwnerRecord>,
    pub current: XSelectionOwnerRecord,
    pub kind: XSelectionChangeKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XSelectionMonitor {
    owners: BTreeMap<(XAtom, Option<NamespaceId>), XSelectionOwnerRecord>,
    generation: u64,
}

impl XSelectionMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn owner(
        &self,
        selection: XAtom,
        namespace: Option<NamespaceId>,
    ) -> Option<XSelectionOwnerRecord> {
        self.owners.get(&(selection, namespace)).copied()
    }

    pub fn current_owner_for_selection(&self, selection: XAtom) -> Option<XSelectionOwnerRecord> {
        self.owners
            .values()
            .filter(|record| record.selection == selection && record.owner.is_some())
            .max_by_key(|record| record.generation)
            .copied()
    }

    pub fn apply_event(
        &mut self,
        event: XSelectionEvent,
        windows: &XWindowTable,
    ) -> XSelectionOwnerUpdate {
        self.apply_event_in_namespace(event, windows, None)
    }

    pub fn apply_event_in_namespace(
        &mut self,
        event: XSelectionEvent,
        windows: &XWindowTable,
        namespace: Option<NamespaceId>,
    ) -> XSelectionOwnerUpdate {
        let namespace_from_owner = event
            .owner
            .and_then(|owner| windows.get(owner).map(|window| window.namespace));
        let namespace = namespace_from_owner
            .or(namespace)
            .or_else(|| self.namespace_for_existing_selection(event.selection));
        let key = (event.selection, namespace);
        let previous = self.owners.get(&key).copied();
        self.generation = self.generation.saturating_add(1);
        let current = XSelectionOwnerRecord {
            selection: event.selection,
            namespace,
            owner: event.owner,
            generation: self.generation,
            timestamp: event.timestamp,
            selection_timestamp: event.selection_timestamp,
            requester: None,
        };

        self.owners.insert(key, current);

        XSelectionOwnerUpdate {
            previous,
            current,
            kind: event.kind,
        }
    }

    /// Remember which client took a selection, once the ownership stands.
    pub fn set_requester(
        &mut self,
        selection: XAtom,
        namespace: Option<NamespaceId>,
        requester: u64,
    ) {
        if let Some(record) = self.owners.get_mut(&(selection, namespace))
            && record.owner.is_some()
        {
            record.requester = Some(requester);
        }
    }

    /// Drop every ownership `requester` took, returning what changed: "when
    /// the owner's client terminates, the selection reverts to having no
    /// owner", whichever client's window it named.
    pub fn clear_requester_ownerships(
        &mut self,
        requester: u64,
        windows: &XWindowTable,
        kind: XSelectionChangeKind,
    ) -> Vec<XSelectionOwnerUpdate> {
        let owners = self
            .owners
            .values()
            .filter(|record| record.owner.is_some() && record.requester == Some(requester))
            .copied()
            .collect::<Vec<_>>();
        owners
            .into_iter()
            .map(|owner| {
                self.apply_event_in_namespace(
                    XSelectionEvent {
                        selection: owner.selection,
                        owner: None,
                        timestamp: owner.timestamp,
                        selection_timestamp: owner.selection_timestamp,
                        kind,
                    },
                    windows,
                    owner.namespace,
                )
            })
            .collect()
    }

    /// Drop `window`'s selection ownerships, returning what changed.
    ///
    /// The returned updates are what watchers are owed. Each keeps the
    /// timestamps of the ownership being ended, because an event reporting
    /// that an owner went away still reports when that ownership began.
    pub fn clear_window_owner(
        &mut self,
        window: XResourceId,
        windows: &XWindowTable,
        kind: XSelectionChangeKind,
    ) -> Vec<XSelectionOwnerUpdate> {
        let owners = self
            .owners
            .values()
            .filter(|record| record.owner == Some(window))
            .copied()
            .collect::<Vec<_>>();
        let mut cleared = Vec::with_capacity(owners.len());
        for owner in owners {
            cleared.push(self.apply_event_in_namespace(
                XSelectionEvent {
                    selection: owner.selection,
                    owner: None,
                    timestamp: owner.timestamp,
                    selection_timestamp: owner.selection_timestamp,
                    kind,
                },
                windows,
                owner.namespace,
            ));
        }
        cleared
    }

    fn namespace_for_existing_selection(&self, selection: XAtom) -> Option<NamespaceId> {
        self.owners
            .iter()
            .find_map(|((record_selection, namespace), record)| {
                if *record_selection == selection && record.owner.is_some() {
                    *namespace
                } else {
                    None
                }
            })
    }
}
