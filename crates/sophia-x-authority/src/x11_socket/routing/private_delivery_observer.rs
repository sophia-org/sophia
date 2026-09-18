// The one thing a receipt consumer is allowed to do.
//
// NARROW ON PURPOSE. The call that marks a delivery observed lives on
// `XAuthorityRoutedInputSender`, which also sends routed input. Handing that to
// whoever consumes receipts would give a consumer the ability to inject input,
// which is the opposite of what consuming a receipt is. This carries only the
// recovery ledger and offers only the observation, so a receipt consumer holds
// no broker, no owner, no lease and no way to send anything.
//
// WHY THE OBSERVATION MATTERS AT ALL. A delivery's ticket is pruned only once
// it is both routing-finished and observed. Publishing a receipt to a consumer
// that never observes it leaves the ticket in place for good, so the bound on
// live deliveries is consumed permanently rather than cycled. The failure that
// produces is not unbounded growth -- the tickets are bounded -- but a service
// that quietly stops accepting deliveries once enough receipts have gone
// unobserved. Observation is what gives the place back.

/// Marks a delivery receipt observed, and nothing else.
#[cfg(unix)]
#[derive(Clone)]
pub struct PrivateDeliveryObserver {
    recovery: InputRecovery,
}

#[cfg(unix)]
impl PrivateDeliveryObserver {
    fn over(recovery: InputRecovery) -> Self {
        Self { recovery }
    }

    /// Mark this exact receipt observed.
    ///
    /// `false` MEANS THIS RECEIPT DID NOT RELEASE ANYTHING, and the caller
    /// still holds it. The ledger refuses an observation whose ticket it does
    /// not have, one already observed, and one whose terminal receipt is not
    /// the receipt offered -- so a consumer cannot free a place by presenting
    /// an answer that belongs to a different delivery.
    pub fn observe(&self, receipt: XAuthorityClientInputDelivery) -> bool {
        self.recovery.observe(receipt)
    }
}

#[cfg(unix)]
impl core::fmt::Debug for PrivateDeliveryObserver {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("PrivateDeliveryObserver")
    }
}
