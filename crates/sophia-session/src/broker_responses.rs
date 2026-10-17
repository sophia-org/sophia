//! Session-side preparation of an entire broker response. Transport validation
//! precedes this boundary; routing and current descriptor state are checked here.

use sophia_engine::ChromeDescriptorTable;
use sophia_protocol::{
    AttentionState, BrokerToplevelActionGrant, ChromeDescriptor, IconTokenId, MetadataDisclosure,
    MetadataDisclosureRule, SanitizedChromeMetadata, SurfaceId, TrustLevel,
    broker_files::{BrokerFileRequest, BrokerFileResponseRow, BrokerFileResponseSet},
};

/// A snapshot borrowed while the Session owner exclusively services one request.
/// The caller must reserve downstream capacity before committing the result and
/// must not change these inputs between preparation and commit.
pub struct BrokerResponseContext<'a> {
    pub epoch: u64,
    pub request_id: u64,
    pub request: BrokerFileRequest<'a>,
    pub admitted: bool,
    pub retired: bool,
    pub rule: Option<MetadataDisclosureRule>,
    pub grant: Option<BrokerToplevelActionGrant>,
    pub descriptors: &'a ChromeDescriptorTable,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PreparedBrokerResponse {
    Rejected(u16),
    NoChange,
    Effects {
        rule: Option<MetadataDisclosureRule>,
        descriptor: Option<(ChromeDescriptor, BrokerToplevelActionGrant)>,
        retire: Option<SurfaceId>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerResponseFailure {
    Correlation,
    Route,
    Rule,
    Generation,
    Grant,
    Descriptor,
}

impl BrokerResponseContext<'_> {
    pub fn prepare(
        &self,
        set: &BrokerFileResponseSet<'_>,
    ) -> Result<PreparedBrokerResponse, BrokerResponseFailure> {
        use BrokerResponseFailure as Failure;
        if set.epoch != self.epoch || set.request != self.request_id {
            return Err(Failure::Correlation);
        }
        let surface = self.request.surface();
        if self.retired
            || (!self.admitted
                && !matches!(self.request, BrokerFileRequest::SurfaceAdmitted { .. }))
        {
            return Err(Failure::Route);
        }
        let current = self.descriptors.get(surface);
        let mut rule = None;
        let mut descriptor = None;
        let mut retire = None;
        for row in set.rows() {
            if matches!(row.kind, 1..=3) && row.surface != surface {
                return Err(Failure::Correlation);
            }
            match row.kind {
                1 => {
                    let next = rule_from_row(row)?;
                    if !surface.is_valid()
                        || next.generation != current.map_or(0, |d| d.generation)
                        || self.rule.is_some_and(|old| old.icon != next.icon)
                    {
                        return Err(Failure::Rule);
                    }
                    rule = Some(next);
                }
                2 => {
                    let action = BrokerToplevelActionGrant {
                        token: row.grant_token,
                        revocation_epoch: row.grant_revocation_epoch,
                        target_generation: row.grant_target_generation,
                    };
                    if let Some(old) = self.grant {
                        if old.token != action.token
                            || old.revocation_epoch != action.revocation_epoch
                        {
                            return Err(Failure::Grant);
                        }
                    } else if current.is_some() {
                        return Err(Failure::Grant);
                    }
                    match self.request {
                        BrokerFileRequest::CandidateReduced { generation, .. } => {
                            if row.generation != generation {
                                return Err(Failure::Generation);
                            }
                        }
                        _ => {
                            if current.is_none_or(|d| d.generation != row.generation)
                                || self.grant != Some(action)
                            {
                                return Err(Failure::Generation);
                            }
                        }
                    }
                    let effective_rule = rule.or(self.rule).ok_or(Failure::Rule)?;
                    if row.icon.map(IconTokenId::from_raw) != effective_rule.icon
                        || trust(row.trust)? != effective_rule.trust_level
                        || (!effective_rule.disclosure.discloses_text() && row.label.is_some())
                    {
                        return Err(Failure::Rule);
                    }
                    if rule.is_some() {
                        let old = self.rule.ok_or(Failure::Rule)?;
                        if effective_rule.disclosure >= old.disclosure {
                            return Err(Failure::Rule);
                        }
                    }
                    let metadata = SanitizedChromeMetadata {
                        surface,
                        label: row.label.map(str::to_owned),
                        label_redacted: row.redacted,
                        icon: effective_rule.icon,
                        trust_level: effective_rule.trust_level,
                        attention: match row.attention {
                            1 => AttentionState::None,
                            2 => AttentionState::Notice,
                            3 => AttentionState::Critical,
                            _ => return Err(Failure::Descriptor),
                        },
                        generation: row.generation,
                    };
                    let prepared = self
                        .descriptors
                        .prepare_metadata(metadata)
                        .map_err(|_| Failure::Descriptor)?;
                    descriptor = Some((prepared, action));
                }
                3 => retire = Some(surface),
                4 => return Ok(PreparedBrokerResponse::Rejected(row.value)),
                5 => return Ok(PreparedBrokerResponse::NoChange),
                _ => return Err(Failure::Correlation),
            }
        }
        // Lowering after a descriptor exists must clear its retained label in
        // the same response; accepting only the rule would leave old disclosure.
        if let (Some(next), Some(old), Some(_)) = (rule, self.rule, current)
            && next.disclosure < old.disclosure
            && descriptor.is_none()
        {
            return Err(Failure::Rule);
        }
        Ok(PreparedBrokerResponse::Effects {
            rule,
            descriptor,
            retire,
        })
    }
}

fn trust(value: u16) -> Result<TrustLevel, BrokerResponseFailure> {
    match value {
        1 => Ok(TrustLevel::Unknown),
        2 => Ok(TrustLevel::Trusted),
        3 => Ok(TrustLevel::Untrusted),
        4 => Ok(TrustLevel::Isolated),
        _ => Err(BrokerResponseFailure::Rule),
    }
}
fn rule_from_row(
    row: &BrokerFileResponseRow<'_>,
) -> Result<MetadataDisclosureRule, BrokerResponseFailure> {
    Ok(MetadataDisclosureRule {
        surface: row.surface,
        disclosure: match row.disclosure {
            1 => MetadataDisclosure::None,
            2 => MetadataDisclosure::ClassOnly,
            3 => MetadataDisclosure::Full,
            _ => return Err(BrokerResponseFailure::Rule),
        },
        trust_level: trust(row.trust)?,
        icon: row.icon.map(IconTokenId::from_raw),
        generation: row.generation,
    })
}
