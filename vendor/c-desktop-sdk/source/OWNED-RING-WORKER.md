# Direct rendering on a worker

The existing `sophia_ss_image_ring_pixels` / `sophia_ss_image_ring_offer`
pair separates filling from publication. A worker can render directly into
that mapping without staging pixels or adding a protocol operation. This is
**behavioural parity under documented caller ownership**, not enforced loan
safety: C does not reserve a slot on pointer access, bounds-check pointer writes,
or make concurrent SDK calls safe.

1. The service thread owns the session and image adapter. Wait for a READY ring
   and choose a FREE slot. Validate nonzero dimensions, resource limits and
   checked `width*height*4 <= ring_capacity`. Retain the adapter at its original
   address for the job; do not reattach or initialize its storage again.
2. Pass only the pointer and logical length to one worker, retaining slot,
   dimensions and current grant/output/allocation/render identity in the job.
   Allow one outstanding fill per image adapter. No other pixels/offer calls
   on any slot until completion or abandonment is synchronized. The worker
   calls no SDK function, even getters, and never receives the header or spare
   capacity. Do not publish while it can still write.
3. Continue owner-only session/image dispatch, deadlines and acknowledgement.
   Dispatch and disconnect never unmap the ring. Protocol custody can fail while
   the worker retains valid access to its pixels.
4. Synchronize completion with `pthread_join`, a mutex handoff, or equivalent
   release/acquire synchronization. An eventfd or pipe notification alone is not
   the memory-ordering proof. Revalidate current grant epochs, allocation, output,
   geometry and render identity. Offer exactly the slot and dimensions rendered.
   The SDK checks protocol state and geometry, not the application's render
   identity or that metadata matches the bytes rendered.
5. Cancelled, failed or stale work is discarded without Offer; no ID or generation
   is spent. A local refusal or `AGAIN` also spends nothing and retains the pixels.
   Retry only after current state and identity are revalidated, never on a failed
   connection. After success, stop writes until exact SlotReleased, terminal
   validation and `ring_forget` make the slot FREE. Accepted and elapsed deadlines
   do not permit reuse.
6. On timeout/shutdown, request cancellation if supported but keep mapping and job
   storage alive until the worker actually stops. Protocol service can close;
   defer `sophia_ss_image_close`, image destruction and reinitialization until
   worker join. Image close unmaps immediately. If a worker cannot be stopped,
   retain storage until process exit. Discard every unoffered result before
   close/init; a new image at the same address must never accept the old result.
   Deliver worker failures to and wake the owner explicitly. C has no automatic
   drop notification or instance-bound loan token.

Slice callers, DMA mode, wire records, capabilities and storage layouts are
unchanged. The library creates no worker. Session's custody capture copy stays;
direct fill removes only the client-side copy into the slot.

## Independent proof and limits

`src/tests/owned_ring_worker.h`, included in `shell_image_test`, keeps a live
pthread parked mid-fill while the owner drives a scripted 9P peer and real
SEQPACKET image endpoint, then lets the worker finish before joining it.
It checks payload and untouched header through the independently transferred
ring FD. Five scenarios cover owner dispatch during fill and exact-release reuse,
local abort/validation refusal without ID spending, control backpressure/retry,
session loss, and image-channel closure while the worker retains the payload.
Existing owned-ring tests cover mismatched releases, deadlines and contradictions.

Rust pair: `5d89565266c3a066c32c2eebac6fad40dc1d0a66`, with parity note
`4d81630c5125c88cb74c1b45c1a0fe890a12a64a`, based on
`0706ea2c78cd16ba59ed209620354f30a2c78906`. C base:
`3ccc7d04c28309ebed6fd53f399af7c7a5682d80`.
Rust's loan excludes competing mapping access, binds instance and byte length,
outlives producer destruction, and faults the producer when dropped. C instead
requires the caller to enforce exclusion, geometry, identity and deferred
destruction. Fixtures prove the valid C sequence, not misuse refusal. Every C
consumer must qualify its own completion, stale-result rejection, cancellation,
wake and destruction paths. No new public helper is needed for this sequence.

This is device-free SDK evidence only: no Lom renderer, protected launch, native
presentation, default adoption or performance claim.

For explicit SFT r13, the owner calls `sophia_ss_frame_submit_ring` after the
worker join, supplying complete frame intent. Queue pressure preserves the bytes
and source identity for retry; do not re-render or issue a separate legacy Offer.
Prepared/Presented cannot release the payload; status and exact SlotReleased
still decide reuse. `frame_image_worker` exercises this sequence independently.
