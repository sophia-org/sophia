---
id: kgrl6al7
date: 2026-10-17
kind: concept
status: draft
tags: [concept, 9p, shell, lock, rendering, architecture]
---
# Generalising the shell and lock image resource toward an application surface

On 2026-10-17 niltempus accepted an exploration of where the t337 image
resource could go if Sophia later wants a native application surface. This
note records that exploration. It is not a decision, a contract or a task. It
allocates no opcodes, record kinds, capability bits or file names, and it
changes no normative text. The
[descriptor custody review](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
remains the proposed design for shell and lock images, the
[descriptor channel ADR](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md)
remains proposed, and the
[fast image path investigation](../investigations/cerml352-what-a-fast-shell-and-lock-image-path-looks-like-beside-9p.md)
holds the measured evidence and the prior-art reading behind both.

The question is narrow: if the sealed and DMA-BUF image resources built for
the shell and lock ever grow toward an application surface, what shape would
that growth take, and what must the t338 shell-image work avoid foreclosing
in the meantime? Input, text, focus and scale are out of scope here. They
belong to Engine routing and to the protocol authority contract named in the
[9P application frontend design](../../sophia-9p-authority.md), and nothing
in this note advances or blocks them.

## The generalisation

The t337 resource is already most of a surface, seen from the side. It has
admission against a fixed per-connection identity, negotiated constraints
before allocation, an immutable accepted image, a journal of state changes,
and an exactly-once release. A surface adds repetition and timing to that:
the same client presents many images in sequence, cares when each one
reached the screen, and reuses its rendering buffers between presents. The
additions below are ordered so that each stands on the one before it, and
each later step is gated on evidence, not enthusiasm.

First, registered DMA-BUF pools. A client registers a small pool of GPU
buffers once, through the descriptor channel, and later presents name a
buffer in that pool instead of carrying fresh descriptors. This is the
pattern the prior-art reading found everywhere it looked: Wayland attaches
a `wl_buffer` by object, Android cycles BufferQueue slots, and Fuchsia
streams a pool of images. The custody model does not change. Each present
is still captured into a renderer-owned image before acceptance, and the
client buffer is released when its capture completes. CPU content does not
join the pool. A sealed memfd is one immutable image by construction, and
the investigation records why a sealed backing cannot become a reusable
slot; showing the same accepted image again already costs no descriptor,
because candidates name retained resources by identifier.

Second, sync timelines. The first DMA-BUF phase accepts only
producer-completed buffers, so no fence crosses any socket. A surface that
presents at frame rate cannot ask its producer to finish on the CPU before
every present. The mature shape, settled independently by Wayland's syncobj
protocol and PipeWire's timelines, is one timeline registered per surface
at setup, with each present carrying an acquire point and each release
carrying a release point as ordinary numbers in records. Descriptors cross
once; points cross per frame. This step waits on the renderer's
safe-completion seam, because a release point is only honest when the
capture it names has provably finished.

Third, presentation feedback. A surface client paces itself from facts, not
timers: when the image was presented, the refresh interval it landed in,
whether it was composed or reached scanout directly, and when its buffer
may be reused. Today the journal reports resource custody. A surface needs
it to report presentation as well, as a record distinct from acceptance and
from source release, because the three events happen at different times
and a client that confuses them either stalls or tears.

Fourth, and last, direct scanout. Composing from the client's buffer
instead of a renderer-owned capture removes one GPU copy, and reopens
everything the
[renderer import boundary](../../renderer-import-boundary.md) closed on
purpose: a producer writing while the display reads, revocation of a buffer
already on screen, cross-device import, and per-head residency. The
protocol could describe it long before the renderer should attempt it. It
is justified only by a measurement showing the capture copy costs real time
at the sizes and rates Sophia actually presents.

Under all four steps the wire dialect stays plain 9P2000.L. The speed of
the fast systems comes from pre-registered buffers named by small integers
and from explicit release, not from their opcode encodings, so nothing here
needs a Sophia message type. Per frame, the steady state is one bounded
record naming what to present; tags already let a client pipeline without
waiting for each reply, and admission remains the backpressure.

## What t338 must not preclude

t338 implements sealed-memory shell resources, with the sealed memfd per
image as its only CPU data plane and no launch-time shared window. Nothing
in this note reopens that. The following are not features for t338; they
are shapes its records, names and SDK custody rules must leave open, so
that a later surface is an extension by negotiated revision rather than a
retrofit that reinterprets shipped bytes.

- **Separate the offer, the buffer and the present in naming.** The draft
  already keeps an offer ID distinct from a resource ID and its generation.
  Keep it that way, and keep both distinct in kind from any future pool
  slot. A slot names a reusable registered buffer; a slot generation names
  one present of that buffer; neither is a resource, and a resource is not
  consumed by being presented. Concretely: no t338 record, SDK type or
  test should state or imply that one offer, one descriptor packet and one
  presentation are the same event, or that a buffer identity ends when its
  first image is accepted.
- **Do not let the sealed path's one-descriptor-per-image become a model.**
  It follows from seals, not from the contract. SDK custody code that
  assumes every future source arrives as a fresh descriptor beside its
  record will have to be rewritten for a registered pool; custody code
  that treats descriptors as one way a named buffer becomes known will
  not.
- **Leave the events stream able to carry a presentation record.** The
  journal's records have kinds, sequence and a terminal-status discipline.
  A later revision needs a presentation feedback record that can follow
  acceptance without displacing it: acceptance, presentation and source
  release are three facts with three times. t338 must not document
  `ResourceStatus` as the last word about an image, must not let SDK
  clients treat status receipt as permission to reuse a source, and must
  keep `SourceReleased` meaning only that the producer's cooperation is no
  longer needed. The existing rule that release follows capture, not
  status, is the seed of the later release point; keep it exact.
- **Keep sync as a registration the records can grow.** Phase one forbids
  fence descriptors, and that refusal should stay a property of the
  negotiated first revision, not of the shapes themselves. Metadata and
  packet layouts that count descriptors must be written so that a later
  revision registering one timeline per surface is a new negotiated
  object, not a contradiction. Nothing in t338 should promise that no
  fence will ever be registered, only that none is accepted yet.
- **Keep every layout revision-gated, never reinterpreted.** The t337 draft
  already refuses unknown fields in version 1 and gates new records behind
  negotiated capabilities. Hold that line in the SDKs as well as the wire:
  parsers keyed to a negotiated revision can grow a present record, a
  registration object and a feedback record in a later revision; parsers
  that hard-code one layout as the meaning of the file will not. Do not
  reserve speculative fields in t338 records to fake this openness; a
  clean next revision is cheaper than a guessed one.
- **Do not spend the names.** File and record names chosen for shell images
  will be read later beside surface names. Keep image vocabulary for
  images. In particular, nothing in t338 should call an accepted image
  presented before a presentation exists to report, or call the shell
  resource a surface, a window or a buffer pool.

## What would change the answer

Two measurements, both already implied by the t339 comparison, decide
whether steps two to four are engineering or speculation: whether
per-offer descriptor passing costs anything measurable at four outstanding
offers, and whether the renderer-owned capture copy costs anything
measurable at the sizes and rates of the bulk workload. If both come back
negligible, the byte and capture paths were already competitive and the
surface work reduces to feedback records for pacing, which clients can
want without any transport change. If either comes back large, the phases
above are the order to spend effort in, because each removes exactly the
measured cost and no other.

The ecosystem limit stands regardless. X11 and Wayland arrive with Mesa,
toolkits and browsers; a native Sophia surface arrives with two SDKs, the
shell, the lock and whoever writes to the contracts next. Fuchsia could
mandate its buffer negotiation because one owner held the whole stack. The
honest Sophia version of that mandate is the desktop niltempus runs, and
this exploration should be weighed as a direction for it, not as a bid to
replace the X authority, which remains the sole active application
authority.

## Connections

The [descriptor custody review](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
holds the t337 draft this note generalises, including the limits, formats
and release discipline the surface would inherit. The
[descriptor channel ADR](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md)
records why the descriptor channel sits beside the 9P wire rather than on
it. The [fast image path investigation](../investigations/cerml352-what-a-fast-shell-and-lock-image-path-looks-like-beside-9p.md)
holds the pilot figures, the display-stack and file-protocol precedent
reading, and the corrected virtio-fs and FUSE passthrough account that the
pool and registration discussion relies on. The
[9P application frontend design](../../sophia-9p-authority.md) owns the
surface questions this note defers, and the
[renderer import boundary](../../renderer-import-boundary.md) owns the
custody that direct scanout would have to renegotiate.
