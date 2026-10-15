# Bounded route-lease notification custody

Category B: concurrent runtime handoff. This is the approved t330 repair,
based on Sophia ad624948e, not an audit of unrelated input paths.

The X routing registry updates pointer state and then tells Session whether a
lease was confirmed, rejected or released. The two notification sites in
`registry/delivery.rs` use blocking sends and ignore disconnect. Session drains
the queue in `live_session/input.rs` and checks the exact lease identity. The
existing timeout covers release, not every provisional confirmation.

## Scenarios

1. Full receiver queue: retain notifications in a bounded FIFO. Reserve space
   before effects; reservation exhaustion is an explicit service error before
   further effects, not a successful delivery or a dropped update.
2. Receiver disconnects between reservation and publication: a completed effect
   is not undone. Account its notification as abandoned and fail explicitly.
   Queued notifications are not mislabeled delivered.
3. Delayed drain and epoch change: retry preserves FIFO and identity. Session
   consumes a stale notification without granting current authority.
4. Reservation abort: return credit without publishing a fictitious update.

## Model scope

Model three distinct notifications, two reservation/backlog slots, one channel
slot, and an epoch transition. Separate reserve, effect, publish, transfer and
consume. Check custody conservation, capacity, at-most-once consumption and
current-epoch authority. Fault variants drop a full-queue update or accept stale
identity. This is a design safety check, not weak-memory, whole-input-system or
runtime trace validation. Wake-driven retry is checked separately in Rust.

The narrowed audit confirms both existing send sites and the compensating
release timeout. No external issue discussion or prior incident is cited as
evidence. Broader t032 work and native latency qualification are excluded.
