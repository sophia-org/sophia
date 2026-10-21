# Experimental owned-copy ring

This is the default-off D4 option B experiment under
[oofo1zrq](notes/investigations/oofo1zrq-a-portable-9p-core-with-optional-drawing-and-surface-clients.md).
It is not a stable SDK protocol or a new meaning for sealed source kind 1.
The `experimental-owned-copy` build feature forwards from CLI through Session
to runtime. `SOPHIA_DISABLE_OWNED_COPY=1`, sampled when an export is constructed,
disables experimental disclosure and admission in the same binary.

## Authority and custody

The existing owner remains the sequencing authority. A file write proposes
registration or an offer; it does not accept pixels or authorize composition.
The persistent image owner validates proposals through the current allocation
and resource registry, executes bounded next actions, and consumes tagged
worker results. This uses the limited SAM analogy in
[state and transition discipline](state-and-transition-discipline.md), with
explicit Rust states and move-only source and budget ownership. It introduces
no reactor, worker thread, scheduler, or authorization cadence.

The prototype shares each component's existing validation worker and its four
outstanding work credits across sealed ISB, draw and owned-copy jobs. Credits
remain occupied through terminal release publication. This is a per-component
bound; resource and source storage also consume Session-wide registry budgets.
The image-channel job and backing ceilings still apply. Registration reserves
the whole ring; every accepted resource reserves its own immutable destination.

## Private transport

With content and the image channel ready, an enabled export discloses the
write-only `experimental-owned-ring-v1` file. Normal export checks apply.
The peer must choose this mode before any sealed-image or draw submission on
that connection. Version 1 permits one registration, with no ring replacement.
Other connections retain the ordinary sealed path. Candidates, allocation
controls, resource outcomes and ResourceReleased use their existing records.

All experimental records begin with a little-endian u32 total byte length and
a four-byte magic; remaining fields are canonical little-endian u64 words.
The exact encoding and validation live in `owned_ring_protocol.rs`.

1. The peer writes D4C1 Register with connection/grant/channel identities,
   ring identity/generation, slot count and pixel bytes per slot. The write
   reserves both registration-event credits and fixes the deadline.
2. The owner reserves source storage and journals D4E1 RingAdmitted.
3. Only then may the peer send the matching D4F1 registration plus exactly one
   descriptor on the existing authenticated image-channel socket. All fields,
   including geometry, must match. Duplicate descriptors are refused.
4. The owner admits the source and journals D4E1 RingReady, or RingRefused.
5. The peer writes D4C1 Offer with the exact ring identity, offer/slot/generation,
   resource identity, allocation/output generations, topology and dimensions.
   Its successful write transfers slot custody and reserves ResourceStatus and
   SlotReleased journal credits. The producer stops writing that slot.
6. The owner reserves destination and worker credit, then submits the read.
   Terminal ResourceStatus precedes the exact D4E1 SlotReleased. Pre-read
   rejection also releases the slot obligation established by the write.

Only final resource-store events carry internal settlement provenance through
output custody. A reply to a failed control cannot spend a sealed or owned transfer's
status reservation, even if the client reuses the offer transaction. Cancellation
may legitimately settle under its own transaction; the flagged event matches
its exact grant and resource. Pre-read refusal retains its separate pending-offer
custody. A backpressured refusal retries on existing service/authorization turns,
without repeatedly waking on an already-expired deadline.

D4E1 events use the existing event journal, sequence, byte offsets and ACK.
They are not ResourceReleased and must not be decoded as stable SDK events.
Experimental peers must parse both framings. Exact latest-control retries are
idempotent and do not renew deadlines or generate a second outcome. Older or
changed retries are refused. Accepted offer IDs and slot generations advance
at write custody, including offers later rejected by the resource owner.

## Source and destination lifetime

There are one to four slots. Each slot has a 4096-byte header followed by a
tightly packed pixel payload rounded up to 4096 bytes. Payload length is a
positive multiple of four, at most 4 MiB. Only the little-endian u64 generation
at header offset zero is read; geometry and identity come from trusted admitted
control state. Ready generations are nonzero, even and increasing per slot.

The first live adapter requires Linux F_GET_SEALS to report GROW and SHRINK,
and requires the exact regular-file extent. WRITE is not required. This
qualifies backing for this experiment; it does not establish a syscall timing
bound. The internal seal-free pread probe remains separate. Other hosts need
their own backing and read-behavior qualification before live admission.

The worker never maps the source. It reads generation, performs one payload
pread, and checks generation again. Short, failed or changed-generation reads
are rejected without silently retrying into a presented frame. Equal
producer-controlled generation words do not prove a coherent snapshot against
a malicious writer. Cooperative slot handoff prevents tearing; ownership of
the validated destination isolates all subsequent producer writes. No pixel
authentication or zero-copy claim follows.

Source read tickets retain the descriptor and its storage charge through
disconnect and cancellation. The descriptor closes before its charge returns.
SlotReleased permits reuse only for its exact ring/offer/slot/generation.
ResourceReleased independently follows the accepted copy's normal resource
and consumer lifetime. Eight retained resources can therefore be populated
through two repeatedly released source slots.

This implementation copies into owned CPU storage and retains the existing
renderer upload path. Replacing that upload with a Session-owned GPU-visible
destination is still measurement and implementation work.

## Qualification still required

Automated controls cover the private codec, journal reservations, export custody
and owner lifecycle. An authenticated in-process transport fixture exercises
registration, descriptor delivery, cancellation and slot reuse with the actual
owner and worker, using a supplied test authority. Shared arrival ordinals assert
that final status precedes slot release on both cancellation and success. This
does not qualify a live Session or the native authentication setup.

A real timing-peer ring producer and paired native measurements remain required.
No native or performance acceptance follows from these automated controls.

Use the accepted four-workload Wayland/XLibre bar on crunch and x13; IPC is an
ungated diagnostic. External numerical thresholds remain unset. Retained
resource qualification and D3 cache-on/off controls remain distinct from the
per-frame lifecycle comparison. This feature is not production-default-on.
