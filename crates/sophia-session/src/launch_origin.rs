//! Session-owned process provenance. No process or application identity reaches
//! policy; the registry copies only bookmarks published by committed policy.
use sophia_protocol::{
    ClientAdmissionContext, POLICY_MAX_SURFACES, PolicyLaunchContext, SurfaceId,
};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_ANCESTRY_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub start_time: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessSnapshot {
    pub identity: ProcessIdentity,
    pub parent: u32,
}
#[derive(Clone, Debug)]
struct Peer {
    admission: ClientAdmissionContext,
    process: ProcessIdentity,
    ancestors: Vec<ProcessIdentity>,
}
#[derive(Clone, Copy, Debug)]
struct Source {
    admission: ClientAdmissionContext,
    bookmark: Option<PolicyLaunchContext>,
    focus: u64,
}

#[derive(Default, Debug)]
pub struct LaunchOriginRegistry {
    catalog_attributions: BTreeMap<
        SurfaceId,
        (
            sophia_protocol::TransactionId,
            sophia_protocol::PolicyOutputLaunchContext,
        ),
    >,
    output_contexts: Vec<sophia_protocol::PolicyOutputLaunchContext>,
    epoch: u64,
    peers: BTreeMap<u64, Peer>,
    grants: BTreeMap<ProcessIdentity, Option<PolicyLaunchContext>>,
    sources: BTreeMap<SurfaceId, Source>,
    pending: BTreeMap<SurfaceId, PolicyLaunchContext>,
    focus_serial: u64,
    focused: Option<SurfaceId>,
}

pub fn read_process(pid: u32) -> Option<ProcessSnapshot> {
    if pid == 0 {
        return None;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (prefix, rest) = stat.rsplit_once(')')?;
    if prefix.split_whitespace().next()?.parse::<u32>().ok()? != pid {
        return None;
    }
    let fields = rest.split_whitespace().collect::<Vec<_>>();
    Some(ProcessSnapshot {
        identity: ProcessIdentity {
            pid,
            start_time: fields.get(19)?.parse().ok()?,
        },
        parent: fields.get(1)?.parse().ok()?,
    })
}

/// A bounded, verified chain. A disappearing/reused link makes the whole hint
/// unavailable, rather than attributing to a different process with that PID.
pub fn process_ancestors(
    process: ProcessIdentity,
    mut read: impl FnMut(u32) -> Option<ProcessSnapshot>,
) -> Vec<ProcessIdentity> {
    let mut chain = Vec::new();
    let mut seen = BTreeSet::new();
    let Some(mut current) = read(process.pid).filter(|s| s.identity == process) else {
        return Vec::new();
    };
    let mut observations = vec![current];
    for _ in 0..MAX_ANCESTRY_DEPTH {
        if current.parent == 0 {
            break;
        }
        if !seen.insert(current.identity.pid) {
            return Vec::new();
        }
        let Some(parent) = read(current.parent) else {
            return Vec::new();
        };
        if parent.identity.start_time == 0
            || parent.identity.start_time > current.identity.start_time
        {
            return Vec::new();
        }
        chain.push(parent.identity);
        observations.push(parent);
        current = parent;
    }
    if current.parent != 0 {
        return Vec::new();
    }
    if observations
        .iter()
        .any(|old| read(old.identity.pid) != Some(*old))
    {
        return Vec::new();
    }
    chain
}

impl LaunchOriginRegistry {
    pub fn publish_outputs(
        &mut self,
        epoch: u64,
        contexts: &[sophia_protocol::PolicyOutputLaunchContext],
    ) {
        if epoch == self.epoch
            && sophia_protocol::encode_wm_output_launch_contexts(contexts, epoch, 0).is_ok()
        {
            self.output_contexts = contexts.to_vec();
        }
    }

    pub fn output_contexts(&self) -> &[sophia_protocol::PolicyOutputLaunchContext] {
        &self.output_contexts
    }

    /// Call only after exact registered-process attribution. A token is not
    /// process authority and cannot authorize an unrelated client's window.
    pub fn catalog_attribution(
        &self,
        surface: SurfaceId,
    ) -> Option<(
        sophia_protocol::TransactionId,
        sophia_protocol::PolicyOutputLaunchContext,
    )> {
        self.catalog_attributions.get(&surface).copied()
    }

    pub fn register_catalog_origin(
        &mut self,
        surface: SurfaceId,
        transaction: sophia_protocol::TransactionId,
        context: sophia_protocol::PolicyOutputLaunchContext,
    ) -> bool {
        if context.epoch != self.epoch
            || context.token == 0
            || transaction.raw() == 0
            || self.epoch == 0
            || !self.sources.contains_key(&surface)
            || !self.output_contexts.iter().any(|c| {
                c.output == context.output && c.output_generation == context.output_generation
            })
        {
            return false;
        }
        self.pending.insert(
            surface,
            PolicyLaunchContext {
                surface,
                epoch: context.epoch,
                token: context.token,
            },
        );
        self.catalog_attributions
            .insert(surface, (transaction, context));
        true
    }

    pub fn set_epoch(&mut self, epoch: u64) {
        if self.epoch == epoch {
            return;
        }
        self.epoch = epoch;
        self.output_contexts.clear();
        self.catalog_attributions.clear();
        for source in self.sources.values_mut() {
            source.bookmark = None;
        }
        for grant in self.grants.values_mut() {
            *grant = None;
        }
        self.pending.clear();
    }

    pub fn admit(
        &mut self,
        admission: ClientAdmissionContext,
        process: ProcessIdentity,
        ancestors: &[ProcessIdentity],
    ) {
        if ancestors.len() > MAX_ANCESTRY_DEPTH
            || self.peers.len() >= POLICY_MAX_SURFACES
            || process.pid == 0
            || process.start_time == 0
        {
            return;
        }
        if !self.grants.contains_key(&process) {
            let mut bookmark = None;
            for ancestor in ancestors {
                let candidates = self
                    .sources
                    .values()
                    .filter(|s| {
                        s.admission.namespace == admission.namespace
                            && self
                                .peers
                                .get(&s.admission.client_id.raw())
                                .is_some_and(|p| p.process == *ancestor)
                    })
                    .collect::<Vec<_>>();
                if candidates.is_empty() {
                    continue;
                }
                let candidate = if candidates.len() == 1 {
                    candidates.first().copied()
                } else {
                    candidates
                        .iter()
                        .copied()
                        .filter(|s| s.focus != 0)
                        .max_by_key(|s| s.focus)
                };
                bookmark = candidate.and_then(|s| s.bookmark);
                break;
            }
            tracing::debug!(
                origin_available = bookmark.is_some(),
                "child launch origin captured at X admission"
            );
            self.grants.insert(process, bookmark);
        }
        self.peers.insert(
            admission.client_id.raw(),
            Peer {
                admission,
                process,
                ancestors: ancestors.to_vec(),
            },
        );
    }

    pub fn belongs_to_process(
        &self,
        admission: ClientAdmissionContext,
        process: ProcessIdentity,
    ) -> bool {
        self.peers
            .get(&admission.client_id.raw())
            .is_some_and(|peer| {
                peer.admission == admission
                    && (peer.process == process || peer.ancestors.contains(&process))
            })
    }

    pub fn revoke(&mut self, admission: ClientAdmissionContext) {
        if let Some(peer) = self.peers.remove(&admission.client_id.raw())
            && !self.peers.values().any(|p| p.process == peer.process)
        {
            self.grants.remove(&peer.process);
        }
        let removed = self
            .sources
            .iter()
            .filter(|(_, s)| s.admission == admission)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for surface in removed {
            self.withdraw(surface);
        }
    }

    /// Called on first policy-manageable top-level admission, not on an arbitrary
    /// surface update or on a connection merely modifying another client's XID.
    pub fn observe_toplevel(&mut self, surface: SurfaceId, admission: ClientAdmissionContext) {
        if self.sources.contains_key(&surface) || self.sources.len() >= POLICY_MAX_SURFACES {
            return;
        }
        if let Some(peer) = self
            .peers
            .get(&admission.client_id.raw())
            .filter(|p| p.admission == admission)
            && let Some(Some(mut origin)) = self.grants.get_mut(&peer.process).map(Option::take)
            && origin.epoch == self.epoch
            && self.epoch != 0
        {
            origin.surface = surface;
            self.pending.insert(surface, origin);
        }
        self.sources.insert(
            surface,
            Source {
                admission,
                bookmark: None,
                focus: 0,
            },
        );
    }

    pub fn withdraw(&mut self, surface: SurfaceId) {
        self.catalog_attributions.remove(&surface);
        self.sources.remove(&surface);
        self.pending.remove(&surface);
        if self.focused == Some(surface) {
            self.focused = None;
        }
    }

    pub fn focused(&mut self, surface: Option<SurfaceId>) {
        if self.focused == surface {
            return;
        }
        self.focused = surface;
        if let Some(source) = surface.and_then(|s| self.sources.get_mut(&s)) {
            self.focus_serial = self.focus_serial.saturating_add(1);
            source.focus = self.focus_serial;
        }
    }

    pub fn publish(&mut self, epoch: u64, contexts: &[PolicyLaunchContext]) {
        if epoch != self.epoch {
            return;
        }
        for context in contexts {
            if context.epoch == epoch
                && context.token != 0
                && let Some(source) = self.sources.get_mut(&context.surface)
            {
                source.bookmark = Some(*context);
            }
        }
    }

    pub fn origins(&self, live: impl IntoIterator<Item = SurfaceId>) -> Vec<PolicyLaunchContext> {
        live.into_iter()
            .filter_map(|s| self.pending.get(&s).copied())
            .collect()
    }

    pub fn committed(&mut self, surfaces: impl IntoIterator<Item = SurfaceId>) {
        for surface in surfaces {
            self.catalog_attributions.remove(&surface);
            self.pending.remove(&surface);
        }
    }
}
