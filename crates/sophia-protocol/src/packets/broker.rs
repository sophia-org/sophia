#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BrokerToplevelActionGrant {
    pub token: u64,
    pub revocation_epoch: u64,
    pub target_generation: u64,
}
