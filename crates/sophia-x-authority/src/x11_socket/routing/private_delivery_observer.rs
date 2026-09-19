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

/// What observing one receipt established.
///
/// FOUR ANSWERS, AND NONE OF THEM IS A PROMISE ABOUT CAPACITY. `Observed`
/// records that the consumer has taken this receipt; the delivery's place comes
/// back only once the ledger is ALSO finished routing it, and which of those
/// happens last is not this call's to know. Reading `Observed` as "a place is
/// free now" would be a guess.
///
/// `Unreadable` is not a refusal either: the ledger was never asked, nothing
/// was decided, and a consumer that treated it as a refusal would conclude a
/// receipt was rejected when it was not looked at.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateDeliveryObservation {
    /// The consumer's observation is recorded.
    ///
    /// NOT "THE PLACE IS BACK". A ticket is pruned when it is both observed and
    /// routing-finished. This is one of those two; if routing has not finished,
    /// the ticket is still there and will be pruned when it does.
    Observed,
    /// The ledger holds no ticket for this delivery.
    UnknownDelivery,
    /// This delivery had already been observed. Nothing further is recorded,
    /// and this says nothing about whether its place has come back: routing may
    /// still be unfinished.
    AlreadyObserved,
    /// The ledger's terminal answer for this delivery is not the receipt
    /// offered, so this receipt frees nothing.
    TerminalMismatch,
    /// The ledger could not be read. Nothing was decided.
    Unreadable,
}

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
    /// ANYTHING BUT `Observed` LEAVES THE RECEIPT WITH THE CALLER. The ledger
    /// refuses an observation whose ticket it does not have, one already made,
    /// and one whose terminal receipt is not the receipt offered -- so a
    /// consumer cannot free a place by presenting an answer that belongs to a
    /// different delivery -- and it reports separately when it could not be
    /// read at all, which is not a refusal.
    pub fn observe(&self, receipt: XAuthorityClientInputDelivery) -> PrivateDeliveryObservation {
        self.recovery.observe_typed(receipt)
    }

    /// The terminal answer the ledger holds for this delivery, if any.
    ///
    /// A WITNESS, NOT A CONSUMPTION. It reads the ledger and frees nothing, so
    /// a caller can establish that its own delivery has settled without taking
    /// the receipt off the channel -- which is the only way to wait for a
    /// delivery and then still be able to test what draining does.
    pub fn settled(
        &self,
        delivery: XAuthorityInputDeliveryId,
    ) -> Option<XAuthorityClientInputDelivery> {
        self.recovery.settled(delivery)
    }
}

#[cfg(unix)]
impl core::fmt::Debug for PrivateDeliveryObserver {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("PrivateDeliveryObserver")
    }
}
