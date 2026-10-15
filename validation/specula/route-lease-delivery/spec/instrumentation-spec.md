# Action map

Paths below are relative to `crates/` and use symbols so source movement does
not silently repoint a line reference.

| Action | Implementation boundary | Values to observe |
| --- | --- | --- |
| Reserve | `sophia-x-authority/src/x11_socket/routing/route_lease_outbox.rs`, `reserve`, after credit reservation | reserved and pending counts, immutable identity at caller |
| Effect | `routing/registry/delivery.rs`, routed input or lease release after reservation | original identity and effect kind; do not infer cancellation from later notification failure |
| Abort | `RouteLeaseReservation::drop` | reserved count decreases, no accepted notification |
| Publish | `RouteLeaseReservation::commit` | accepted increments, pending or explicitly abandoned obligation |
| Transfer | outbox `flush`, successful `try_send` | original notification, published count, pending count |
| Consume | `sophia-session/src/live_session/input.rs`, `drain_application_route_lease_updates` | exact identity, accepted or stale result |
| Advance | existing Session input epoch owner | old and new lease identities/epoch |
| Disconnect | outbox failed send or terminal teardown | abandoned notifications; effects remain historical facts |

The model's consumed/abandoned sets include the receiver's abstract custody;
the outbox's `published` counter stops at channel admission, not Session
observation. Rust accounting is accepted = published + pending + abandoned.
No production trace instrumentation is added in this repair. Trace.tla is an
explicit design fixture; its checks cannot establish runtime refinement.
