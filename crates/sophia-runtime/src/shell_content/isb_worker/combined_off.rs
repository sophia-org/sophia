//! Without the combined frame-job feature a job carries no route and a
//! pending row no forwarding identity.
pub(super) type RouteSlot<P> = std::marker::PhantomData<fn() -> P>;
pub(super) type PendingSlot = ();
