//! Which retained surfaces have no drawable source, where, and why (t306).
//!
//! A committed surface normally draws from its retained renderer image or a
//! CPU layer. After a topology change an image may have no store yet, or none
//! on the output that now shows the surface, and after a forced revocation
//! there may be no image at all. Such a surface is left out of that output's
//! display list exactly as a surface whose first frame has not landed, so it
//! is neither drawn nor hit-tested there; its own next Present is never left
//! out.
//!
//! `Pending` waits for an image to reach the output's stores and clears when
//! it does. `Lost` has no image to wait for and clears only when a Present of
//! the surface commits. Output-scoped entries describe one topology: they are
//! dropped when the outputs are replaced and derived again from the restore.

use sophia_protocol::{OutputId, SurfaceId};
use sophia_renderer_live::LiveRendererImageId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveSourceUnavailableReason {
    /// The image exists but is not in this output's stores yet.
    Pending(LiveRendererImageId),
    /// No image remains; only a new frame from the client brings content.
    Lost,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Scope {
    All,
    Outputs(BTreeSet<OutputId>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    reason: LiveSourceUnavailableReason,
    scope: Scope,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveSourceAvailability {
    entries: BTreeMap<SurfaceId, Entry>,
}

impl LiveSourceAvailability {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether `surface` may be drawn on `output`. `presenting` is exempt:
    /// the surface whose Present is being composed draws its new source.
    pub fn available_on(
        &self,
        surface: SurfaceId,
        output: OutputId,
        presenting: Option<SurfaceId>,
    ) -> bool {
        if presenting == Some(surface) {
            return true;
        }
        self.entries
            .get(&surface)
            .is_none_or(|entry| match &entry.scope {
                Scope::All => false,
                Scope::Outputs(outputs) => !outputs.contains(&output),
            })
    }

    /// Marks `surface` unavailable on `outputs`, or on every output when
    /// `outputs` is `None`. A lost source overrides a pending one, and scopes
    /// of the same reason widen.
    pub fn mark(
        &mut self,
        surface: SurfaceId,
        reason: LiveSourceUnavailableReason,
        outputs: Option<BTreeSet<OutputId>>,
    ) {
        let scope = outputs.map_or(Scope::All, Scope::Outputs);
        match self.entries.get_mut(&surface) {
            Some(entry) if entry.reason == reason => {
                entry.scope = match (std::mem::replace(&mut entry.scope, Scope::All), scope) {
                    (Scope::Outputs(mut held), Scope::Outputs(added)) => {
                        held.extend(added);
                        Scope::Outputs(held)
                    }
                    _ => Scope::All,
                };
            }
            Some(entry)
                if reason != LiveSourceUnavailableReason::Lost
                    && entry.reason == LiveSourceUnavailableReason::Lost => {}
            _ => {
                self.entries.insert(surface, Entry { reason, scope });
            }
        }
    }

    /// A Present of `surface` committed: it has a source again everywhere.
    /// Only a committed retirement may call this; a skipped, cancelled or
    /// stale one leaves the surface as it was.
    pub fn committed(&mut self, surface: SurfaceId) {
        self.entries.remove(&surface);
    }

    /// `image` reached `output`'s stores: surfaces pending on it there draw
    /// again. Returns the surfaces that became available somewhere.
    pub fn image_local(&mut self, image: LiveRendererImageId, output: OutputId) -> Vec<SurfaceId> {
        let mut released = Vec::new();
        self.entries.retain(|surface, entry| {
            if entry.reason != LiveSourceUnavailableReason::Pending(image) {
                return true;
            }
            match &mut entry.scope {
                // An image pending everywhere has no store at all; reaching
                // one output narrows it to the rest only when the caller
                // re-marks them, so it is released here and re-derived.
                Scope::All => {
                    released.push(*surface);
                    false
                }
                Scope::Outputs(outputs) => {
                    if outputs.remove(&output) {
                        released.push(*surface);
                    }
                    !outputs.is_empty()
                }
            }
        });
        released
    }

    /// `image` reached a store: surfaces waiting on it anywhere draw again,
    /// any output still lacking it served by the ordinary cold migration.
    pub fn image_placed(&mut self, image: LiveRendererImageId) -> Vec<SurfaceId> {
        let mut released = Vec::new();
        self.entries.retain(|surface, entry| {
            let waiting = entry.reason == LiveSourceUnavailableReason::Pending(image);
            if waiting {
                released.push(*surface);
            }
            !waiting
        });
        released
    }

    /// The (image, output) pairs still waiting for an image to arrive.
    pub fn pending_destinations(&self) -> BTreeSet<(LiveRendererImageId, Option<OutputId>)> {
        self.entries
            .values()
            .filter_map(|entry| match entry.reason {
                LiveSourceUnavailableReason::Pending(image) => Some((image, &entry.scope)),
                LiveSourceUnavailableReason::Lost => None,
            })
            .flat_map(|(image, scope)| match scope {
                Scope::All => vec![(image, None)],
                Scope::Outputs(outputs) => outputs
                    .iter()
                    .map(|output| (image, Some(*output)))
                    .collect(),
            })
            .collect()
    }

    /// After a replacement published, an image held by some store reaches
    /// the outputs that sample it through the ordinary cold migration, which
    /// defers their frames meanwhile. Only images no store holds stay marked.
    pub fn release_output_scoped_pending(&mut self) {
        self.entries.retain(|_, entry| {
            !matches!(entry.reason, LiveSourceUnavailableReason::Pending(_))
                || entry.scope == Scope::All
        });
    }

    /// Surfaces that left the session take their entries with them.
    pub fn prune(&mut self, removed: &[SurfaceId]) {
        for surface in removed {
            self.entries.remove(surface);
        }
    }

    /// The outputs were replaced. Entries scoped to outputs, and every
    /// pending entry, described the previous topology and are derived again
    /// from the restore; a source lost everywhere stays lost.
    pub fn outputs_replaced(&mut self) {
        self.entries.retain(|_, entry| {
            entry.reason == LiveSourceUnavailableReason::Lost && entry.scope == Scope::All
        });
    }

    /// An applied topology renumbered the outputs without a handoff. Scopes
    /// named the old outputs and are dropped; what holds everywhere stays,
    /// including images no store holds, which only a restore can place.
    pub fn outputs_rebound(&mut self) {
        self.entries.retain(|_, entry| entry.scope == Scope::All);
    }

    pub fn surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.entries.keys().copied()
    }
}

/// When pending renderer images are offered a store again (REVIEW-CODEX-06
/// R1). A deferral behind GPU work in flight is retried on the owner's short
/// service until it settles; a store that had no room waits until storage
/// progress changes, so a store that stays full is not asked again and keeps
/// the owner asleep. The token is recorded after each attempt, so an attempt
/// never wakes itself.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LiveRendererImageRetryGate {
    token: Option<u64>,
    busy: bool,
}

impl LiveRendererImageRetryGate {
    pub fn due(&self, token: u64) -> bool {
        self.busy || self.token != Some(token)
    }

    pub fn observe(&mut self, token_after: u64, busy: bool) {
        self.token = Some(token_after);
        self.busy = busy;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}
