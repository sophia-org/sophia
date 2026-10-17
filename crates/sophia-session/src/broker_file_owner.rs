//! Session custody for broker effects and the single downstream completion slot.

use crate::broker_responses::{
    BrokerResponseContext, BrokerResponseFailure, PreparedBrokerResponse,
};
use sophia_9p::Errno;
use sophia_engine::ChromeDescriptorTable;
use sophia_protocol::{
    BrokerToplevelActionGrant, MetadataDisclosureRule, SurfaceId,
    broker_files::{BrokerFileRequest, BrokerFileResponseSet},
};
use sophia_runtime::broker_files::BrokerFileEffects;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Eq, PartialEq)]
pub enum BrokerCompletion {
    Applied {
        request: u64,
        rule: Option<MetadataDisclosureRule>,
        retired: Option<SurfaceId>,
    },
    Rejected {
        request: u64,
        reason: u16,
    },
    NoChange {
        request: u64,
    },
}

struct Pending<R> {
    id: u64,
    route: R,
    // Owner validation needs kind, surface and semantic scalars only. The
    // runtime retains the complete request, including its label, for correlation.
    request: BrokerFileRequest<'static>,
}

pub struct BrokerFileOwner<R> {
    epoch: u64,
    descriptors: ChromeDescriptorTable,
    grants: BTreeMap<SurfaceId, BrokerToplevelActionGrant>,
    rules: BTreeMap<SurfaceId, MetadataDisclosureRule>,
    routes: BTreeMap<SurfaceId, R>,
    retired: BTreeSet<SurfaceId>,
    pending: Option<Pending<R>>,
    completion: Option<BrokerCompletion>,
    failure: Option<BrokerResponseFailure>,
}

impl<R: Copy + Eq> BrokerFileOwner<R> {
    pub fn new(epoch: u64) -> Result<Self, Errno> {
        if epoch == 0 {
            return Err(Errno::EINVAL);
        }
        Ok(Self {
            epoch,
            descriptors: Default::default(),
            grants: BTreeMap::new(),
            rules: BTreeMap::new(),
            routes: BTreeMap::new(),
            retired: BTreeSet::new(),
            pending: None,
            completion: None,
            failure: None,
        })
    }
    /// Reserve the completion slot before publishing. If publication fails,
    /// call `abort_unpublished`; never call it for an issued request.
    pub fn begin(
        &mut self,
        id: u64,
        request: BrokerFileRequest<'_>,
        route: R,
    ) -> Result<(), Errno> {
        if self.failure.is_some() {
            return Err(Errno::EIO);
        }
        if self.pending.is_some() || self.completion.is_some() {
            return Err(Errno::EAGAIN);
        }
        let surface = request.surface();
        if id == 0
            || self.retired.contains(&surface)
            || self.routes.get(&surface).is_some_and(|old| *old != route)
            || (!matches!(request, BrokerFileRequest::SurfaceAdmitted { .. })
                && !self.routes.contains_key(&surface))
        {
            return Err(Errno::EACCES);
        }
        self.pending = Some(Pending {
            id,
            route,
            request: owner_request(request),
        });
        Ok(())
    }
    pub fn abort_unpublished(&mut self) {
        self.pending = None;
    }
    pub fn take_completion(&mut self) -> Option<BrokerCompletion> {
        self.completion.take()
    }
    pub fn failure(&self) -> Option<BrokerResponseFailure> {
        self.failure
    }
    pub fn descriptors(&self) -> &ChromeDescriptorTable {
        &self.descriptors
    }
    pub fn grants(&self) -> &BTreeMap<SurfaceId, BrokerToplevelActionGrant> {
        &self.grants
    }
    pub fn rule(&self, surface: SurfaceId) -> Option<MetadataDisclosureRule> {
        self.rules.get(&surface).copied()
    }
    pub fn route(&self, surface: SurfaceId) -> Option<R> {
        self.routes.get(&surface).copied()
    }
    pub fn retired(&self, surface: SurfaceId) -> bool {
        self.retired.contains(&surface)
    }
}

impl<R: Copy + Eq> BrokerFileEffects for BrokerFileOwner<R> {
    type Prepared = PreparedBrokerResponse;
    fn failed(&self) -> bool {
        self.failure.is_some()
    }
    fn prepare(&mut self, set: &BrokerFileResponseSet<'_>) -> Result<Self::Prepared, Errno> {
        if self.failure.is_some() {
            return Err(Errno::EIO);
        }
        let pending = self.pending.as_ref().ok_or(Errno::EIO)?;
        if self.completion.is_some() {
            return Err(Errno::EAGAIN);
        }
        let surface = pending.request.surface();
        let prepared = BrokerResponseContext {
            epoch: self.epoch,
            request_id: pending.id,
            request: pending.request,
            admitted: self.routes.get(&surface) == Some(&pending.route),
            retired: self.retired.contains(&surface),
            rule: self.rules.get(&surface).copied(),
            grant: self.grants.get(&surface).copied(),
            descriptors: &self.descriptors,
        }
        .prepare(set);
        prepared.map_err(|failure| {
            // A semantically invalid authority response terminates this owner.
            // The driver observes the latch and closes the supervised transport.
            self.failure = Some(failure);
            Errno::EIO
        })
    }
    fn commit(&mut self, prepared: Self::Prepared) {
        let pending = self.pending.take().expect("prepared broker request");
        let request = pending.id;
        let completion = match prepared {
            PreparedBrokerResponse::Rejected(reason) => {
                BrokerCompletion::Rejected { request, reason }
            }
            PreparedBrokerResponse::NoChange => BrokerCompletion::NoChange { request },
            PreparedBrokerResponse::Effects {
                rule,
                descriptor,
                retire,
            } => {
                if let Some(rule) = rule {
                    self.rules.insert(rule.surface, rule);
                    if matches!(pending.request, BrokerFileRequest::SurfaceAdmitted { .. }) {
                        self.routes.insert(rule.surface, pending.route);
                    }
                }
                if let Some((descriptor, grant)) = descriptor {
                    self.grants.insert(descriptor.surface, grant);
                    self.descriptors.upsert(descriptor);
                }
                if let Some(surface) = retire {
                    self.descriptors.remove_surface(surface);
                    self.grants.remove(&surface);
                    self.rules.remove(&surface);
                    self.routes.remove(&surface);
                    self.retired.insert(surface);
                }
                BrokerCompletion::Applied {
                    request,
                    rule,
                    retired: retire,
                }
            }
        };
        assert!(
            self.completion.replace(completion).is_none(),
            "reserved broker completion"
        );
    }
}

fn owner_request(request: BrokerFileRequest<'_>) -> BrokerFileRequest<'static> {
    use BrokerFileRequest::*;
    match request {
        SurfaceAdmitted { surface, profile } => SurfaceAdmitted { surface, profile },
        CandidateReduced {
            surface,
            disclosure,
            redacted,
            generation,
            ..
        } => CandidateReduced {
            surface,
            disclosure,
            label: None,
            redacted,
            generation,
        },
        AttentionChanged { surface, attention } => AttentionChanged { surface, attention },
        SurfaceRemoved { surface } => SurfaceRemoved { surface },
        SetDisclosure {
            surface,
            disclosure,
        } => SetDisclosure {
            surface,
            disclosure,
        },
    }
}
