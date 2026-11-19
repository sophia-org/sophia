//! Without the combined frame-job feature a job carries no route and a
//! pending row no forwarding identity.
pub(super) type RouteSlot<P> = std::marker::PhantomData<fn() -> P>;
// PhantomData, not (): rustc exempts it from the never-read lint.
pub(super) type PendingSlot = std::marker::PhantomData<()>;
