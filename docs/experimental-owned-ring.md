# Owned-copy ring

This is the D4 option B implementation under
[oofo1zrq](notes/investigations/oofo1zrq-a-portable-9p-core-with-optional-drawing-and-surface-clients.md).
On 2026-10-21, after the first native ABBA panel result, niltempus selected
B as the default and sole shell CPU upload path, with no build feature or
runtime fallback switch. Those switches have been removed. The production
contract and paired SDK implementation are filed. The r10
production handshake refuses sealed shell ingress. Lock images and DMA-BUF
are separate paths.

The shell integration pins signed C SDK
`4222139b842f2fe6dc4c7e0db9c51250f69bcd94` and Rust SDK
`ce926b531b0185faf3d88f7b82a8d8803ea520f7`. Both accumulate terminal status
kinds for the exact grant/resource until SlotReleased selects the outcome:
success needs Accepted, cancellation needs Cancelled, and other refusals need
Rejected. Failed control replies cannot erase earlier evidence or release a
slot. A release without compatible status evidence faults the ring.

Lom and Provlita use the same Rust SDK pin in their manifests and lockfiles:

| Client | Migration | Lockfile follow-up |
| --- | --- | --- |
| Lom | `31bcdb365f5ec6c2031f1e5261d4b9d2c0b1a811` | `27b64c5c79e2186895178ff05cbbaac63e15c322` |
| Provlita | `37b92af56cc0d0ab21ce067ea334c0818889e7f4` | `109f5cb220eb2efb54b4c074ba4e57910952db1d` |

Both clients passed tests and strict clippy with `--locked`. Their current
slice-upload API copies into the reusable producer ring; it does not claim
direct-fill performance. These local signed revisions have not been published
or deployed. Fresh upstream builds need the SDK revision published first.

An image revision range excluding r10 ends attachment with UnsupportedRevision
and no Refused event. A range that can select r10 but requires retired sealed
or unavailable DMA capability receives capability refusal reason 4.

The measured d4p-03 result is recorded in
`~/.local/state/t331/d4p-03/FINDINGS.txt`: combined Session+peer CPU fell by
252.5–332.5 microseconds per frame in this single-head panel ABBA set. This
is an observation on crunch, not a Wayland/XLibre acceptance verdict.

The authoritative public contract is [sophia-owned-ring.md](sophia-owned-ring.md),
with its shared KDL and corpus. This page records implementation and measurement scope. Ring slots now bound capacity rather
than requiring every resource to have identical dimensions: the owner copies
and charges only each offer’s exact byte extent, within that fixed capacity.

## Authority and custody

The existing owner remains the sequencing authority. A file write proposes
registration or an offer; it does not accept pixels or authorize composition.
The persistent image owner validates proposals through the current allocation
and resource registry, executes bounded next actions, and consumes tagged
worker results. This uses the limited SAM analogy in
[state and transition discipline](state-and-transition-discipline.md), with
explicit Rust states and move-only source and budget ownership. It introduces
no reactor, worker thread, scheduler, or authorization cadence.

The implementation reuses each component's existing validation worker and its
four outstanding work credits. Credits
remain occupied through terminal release publication. This is a per-component
bound; resource and source storage also consume Session-wide registry budgets.
The image-channel job and backing ceilings still apply. Registration reserves
the whole ring; every accepted resource reserves its own immutable destination.

## Transport

With r10 content and owned-ring image capability granted and the channel ready,
the authenticated export discloses the
write-only `owned-ring-v1` file. Normal export checks apply.
Ring registration cannot share a connection with experimental draw submissions. Version 1 permits one registration, with no ring replacement.
Old sealed shell clients are refused during negotiation. Candidates, allocation
controls, resource outcomes and ResourceReleased use their existing records.

All ring records begin with a little-endian u32 total byte length and
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
They are distinct from ResourceReleased. SDK peers dispatch the documented
magic before the ordinary shell header and preserve the common journal sequence. Exact latest-control retries are
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
qualifies backing for this adapter; it does not establish a syscall timing
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

The r10 integration was checked with Runtime tests, native-enabled Session
integration tests and the Session library suite (1,056 passed, 65 intentionally
ignored). The C native launcher and dock run against the production export;
four content-conformance cases and three protected proof-loop cases retain
their malformed-record and mutation controls. The backend's three protected
popout lifecycle cases, the independent Go oracle's 96 judgments, the ring
example and the existing drawing-path regression also pass. These are local
automated checks with the fixtures' stated authority and completion boundaries.

Automated controls cover the shared codec corpus, journal reservations, export custody
and owner lifecycle. An authenticated in-process transport fixture exercises
registration, descriptor delivery, cancellation and slot reuse with the actual
owner and worker, using a supplied test authority. Shared arrival ordinals assert
that final status precedes slot release on both cancellation and success. This
does not qualify a live Session or the native authentication setup.

The `shell_timing_peer_ring` example now speaks r10 after the normal
launch attach and accepts only the owned-ring source. The same-binary sealed
comparison remains reproducible at c1e6942de with its frozen d4p-03 receipts;
it is not a second production mode. The ring smoke covers three frames through
two slots against the actual in-process transport, with supplied authority and
presentation outcomes. It does not establish native launch or pixel correctness.

The completed first paired diagnostic used 120 warmup frames and 2,000 measured completion
intervals, in sealed/ring/ring/sealed order. Both sources keep one frame in
flight and finish source settlement before demanding a candidate. The ring
slots rotate without overlapping offers; this measures amortized source
lifecycle cost, not pipelined ring throughput. Warmup covers each slot's first
mapping faults. `ready_usec - admitted_usec` includes producer ring creation
and is setup time, not Session admission latency.

The producer fills its mapped ring directly. The current Session destination
is initialized before the payload pread overwrites it, and renderer upload is
still separate. CPU observations therefore include that destination work;
this implementation does not yet demonstrate the proposed single-copy GPU
destination. Further native configurations and external comparisons remain
unmeasured. No performance acceptance follows from automated controls or serializer replay.

Use the accepted four-workload Wayland/XLibre bar on crunch and x13; IPC is an
ungated diagnostic. External numerical thresholds remain unset. Retained
resource qualification and D3 cache-on/off controls remain distinct from the
per-frame lifecycle comparison. Native qualification of the migrated clients
and the broader external measurements remain outstanding.

One non-blocking SDK follow-up remains: Rust's status routing currently drops
the content grant and matches status evidence by resource, while C also checks
the grant. SlotReleased still checks exact ring identity and controls custody.
The follow-up must carry the grant into Rust's adapter and test that a foreign
grant's Accepted status supplies no evidence for settlement.

## Production source headroom

The Session-wide 64 MiB source/backing ceilings are unchanged. With two roles,
resource envelopes are 24 MiB each (4 staging, 12 resident, 8 retiring). With
three roles they are 17 MiB each (4 staging, 8 resident, 5 retiring). This leaves
space for one default 4 MiB ring plus its 4096-byte header per role. Larger rings
remain subject to admission; no slot-count setting promises capacity.
ImageLimits includes the default ring extent in its backing ceiling, separately
from resident and retiring destinations. The registry still charges every source
and destination against the unchanged global ceilings. A retained old ring can
refuse a replacement until its worker/source custody ends; it is never refunded
early to make a successor fit. The three-role case keeps 1 MiB above the minimum
retiring envelope for small retained-resource reconnects; a larger retained set
can legitimately refuse a replacement until consumers release it.
