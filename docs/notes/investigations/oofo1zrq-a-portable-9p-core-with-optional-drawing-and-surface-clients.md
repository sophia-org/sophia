---
id: oofo1zrq
date: 2026-10-20
kind: investigation
status: proposed
tags: [investigation, architecture, plan9, 9p, draw, portability, security, t331]
---
# A portable 9P core with optional drawing and surface clients

## Direction and decision status

On October 20 niltempus asked for “a plan9 core that is performant and the
bottom half of the plan9/inferno kernal that is portable across posix.”
Claude's subsequent direction proposal develops the
[parked brainstorm](akr07m5j-hosting-plan-9-on-sophia-and-a-draw-peer-class-brainstorm-of-2026-10-20.md).
This evaluation records that goal and the proposed decisions; it accepts no
new protocol, trust contract, default client class or Engine object model.

The useful architectural direction is to separate namespace and role semantics,
content production, presentation, and host enforcement. One core can serve
different admitted client classes without making each class a second compositor.
Calling that core the device-and-namespace half of Plan 9 describes its intended
role; it does not import Plan 9's process, security or drawing contracts.

## Decisions proposed by the handback

| Item | Proposal | Evaluation and outstanding decision |
| --- | --- | --- |
| D1 | Make command drawing the native default, with surface clients alongside it. | Investigate an optional draw class first. Making it the default requires separate approval and evidence across representative clients. |
| D2 | Define an explicit host capability contract for process identity/lifetime, immutable storage, device buffers and display. | A useful design decomposition. POSIX portability does not establish equivalent enforcement on every host. |
| D3 | Import sealed CPU buffers through Linux udmabuf. | Direct import of current write-sealed ISBs is blocked by upstream Linux 6.18's seal checks; see the correction below. Any alternative needs its own immutability proof. |
| D4 | Permit reusable producer-writable buffers with fences. | Reopens immutable acceptance and consumer ownership, not just journal wording. Keep existing ISB requirements until that decision is explicit. |
| D5 | Prioritize plain 9P2000, with 9P2000.L adapters. | Changes the accepted delivery order. Determine the first real client and its dialect needs before changing that order. |
| D6 | Host Plan 9 or Inferno. | Remains shelved as a product/ecosystem decision. |

The handback proposes vello_cpu first, then Vello and Blend2D, against a
Vello-inspired scene vocabulary. These are candidate implementations, not a
selected dependency or a stable wire ABI. Claims about their relative speed,
JIT configuration or cross-platform support need pinned verification before
selection. Plan 9 raster exactness is reported as unnecessary; compatibility
with libdraw therefore needs a translator and a stated supported subset.

## Preserve the existing owners

The [one-core ADR](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
already assigns role semantics to exports, derives trees from immutable
admission contexts and grants, and retains checks on fids and revocation.
It keeps the X authority in process, with third-party translators out of process.
Descriptors stay off the 9P stream. Engine and runtime_driver retain composition
and sequencing.

A candidate draw export should produce bounded content for an existing admitted
presentation transaction. It must not acquire independent stacking, focus,
placement, presentation or input authority. The proposal's “one object: LAYER”
is an additional object-model decision, not a consequence of adding drawing.
Likewise, an accepted batch or completed raster job is not yet a presented
outcome; existing publication, native completion and release owners remain.

The actual no-draw rationale in the one-core ADR is that a draw-style file
“would wrap a command stream around composition that Engine already owns.”
A bounded content producer could be evaluated against that concern without
immediately replacing composition or declaring drawing the default. The
stronger historical wording about server-side drawing not surviving GPUs is
not the wording of that ADR passage.

Local process custody, connection admission and delegated namespace identity
must remain distinct. “One process = one immutable admission context” is too
strong: the accepted context belongs to the connection, and a translator can
carry sub-identities under its own authority. Sharing custody evidence does not
merge grants, connection epochs or side-channel bindings.

## A draw class changes the work and its security boundary

Commands can avoid client-produced full-frame buffers for content expressible
through those commands. They move rasterization, resource retention and
potentially expensive work into Sophia's service boundary. The total cost must
include client command generation, validation, rasterization, composition,
memory, wakeups and latency.

Per-connection IDs need generation/lifetime rules and checks on every reference.
Budgets must cover retained images/fonts/scenes, batch bytes, expansion and
tessellation work, and outstanding jobs. Small input can request expensive work.
Resource retirement, cancellation, revocation and renderer failure must preserve
charges and references until consumers actually finish. Executing a synchronous
rasterizer on an authority owner would need a demonstrated bound; otherwise the
job must run behind an owned, bounded execution boundary.

There is an unresolved resource-ingress question. A client with no descriptor
path still needs a way to load bitmap images and fonts. Plan 9's draw loads send
pixel data; “no pixels on the stream” cannot also describe that compatibility
path. Choose explicitly between bounded resource uploads, existing image-service
resources, or a restricted vocabulary. Uploading only changed resources differs
from using the stream as a per-frame surface transport.

The accepted one-core ADR already allows bounded screenshot bytes over 9P.
The invariant is not a universal ban on pixel bytes; the current ISB amendment
requires sealed sources for its production image-producing roles. A new draw
resource path needs its own contract and cannot silently restore the withdrawn
byte-upload fallback.

## Portability is an enforcement contract

A portable client protocol can hide host APIs from clients. Drawing still
depends on rendering and presentation services, which ultimately use the host.
An OS capability interface should specify stable identity, exit observation,
failure behavior, immutable acceptance, completion, release and resource charges.
Each host implementation must demonstrate those semantics or refuse the
unsupported capability.

The handback's Linux/BSD/macOS API table is a research checklist, not an
implemented portability matrix. BSD and macOS are not one backend. Equivalent
names for process events or credentials do not prove equivalent race behavior.
The broader claim that only image and X DRI3 receive descriptors also needs an
ingress inventory: existing role and portal descriptor rules remain authoritative.

The [presentation/execution ADR](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
already defines portability through capability detection and documented
prerequisites. It explicitly declines portable hard GPU-time or memory isolation.
Its predecessor [odjw4jav](../decisions/odjw4jav-confine-the-lom-gpu-domain-with-cgroup-dmem.md)
is superseded and must not be treated as a current requirement.

## Mutable pools require more than changing the journal sentence

The [image-channel ADR and its ISB amendment](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md)
retain immutable acceptance and completion-based source release. A producer
fence does not prevent later writes; a session-owned allocation does not by
itself revoke a producer's retained writable mapping.

Allowing mutation would require identifying every reader, validation boundary,
capture/snapshot operation and retained generation that presently relies on
stable content. “Only the producer's layer can be spoiled” is not established
by connection ownership alone, especially for shared scheduling and resource
availability. A design would have to state exactly what is guaranteed about
observed pixels and metadata, and how it contains hostile mutation.

Fresh sealed objects are the current implementation, not a proof that no other
kernel-enforced immutable design could exist. Conversely, a mutable pool is not
an equivalent implementation of the existing guarantee. D3 can be investigated
while preserving immutability; it need not wait for acceptance of D4.

### Verified correction to the udmabuf proposal

On October 20, checking
[upstream Linux v6.18 udmabuf.c](https://github.com/torvalds/linux/blob/v6.18/drivers/dma-buf/udmabuf.c#L273-L289)
established that `check_memfd_seals` requires `F_SEAL_SHRINK` but rejects
`F_SEAL_WRITE` and `F_SEAL_FUTURE_WRITE`. Therefore the proposed direct
conversion of Sophia's already write-sealed ISB to a DMA-BUF is not supported
by that implementation. The handback's claim that SHRINK alone establishes
compatibility omitted the denied seal set. This is an upstream source check,
not a test of the installed kernel or driver.

Creating an export before sealing must not be assumed to preserve the existing
guarantee: writable access through the exported object and the legality of
later sealing require independent proof. DMA-BUF synchronization and immutable
acceptance remain different contracts. D3 is a research question, not a ready
zero-copy path for current ISBs.

## Evidence and next discriminator

Historical c19 counts motivate investigation; they do not partition all current
files/IPC excess or predict the cost of a new draw protocol. In particular,
neither client class can be assigned “approximately zero” liveness cost from
the evidence today, and neither inherits a fixed 35-syscall/frame wire cost.

[C28/C40/C41/C42](tt0k1yqv-how-should-one-isb-presentation-drive-bounded-work-across-owners.md)
remain a separate investigation. C40 aggregates a fresh observation within
bounded scopes; it is not a permanent kernel-maintained live flag. C41 found no
resolved native CPU benefit and its frozen analyzer refused one major fault.
C42 is a coverage diagnostic, not a cost or promotion gate. At the October 20
follow-up inspection, its package had no launch receipt or cc42 stages and no
capture was running; completion is not established.

The recommended sequence is:

1. Establish C42's launch outcome and actual coverage before changing custody
   bounds or making another performance claim.
2. Specify the host capability contract and a narrowly scoped draw experiment
   in parallel as design work. Keep the surface path and Engine ownership fixed.
3. If D1 is authorized, compare equal visible content, update cadence and
   completion semantics; include dynamic image/text work as well as a favorable
   command-only panel. Measure both processes, raster work, memory, CPU,
   wakeups and latency. Add malformed-input, resource exhaustion, cancellation,
   revocation and failed-render controls.
4. Evaluate D3 independently under existing immutable acceptance. Make D4 an
   explicit trust/ownership decision with its own evidence.
5. Prioritize D5 when a concrete client needs it; retain D6 as parked.

This direction supports a smaller public core with explicit host proofs and
optional content producers. It does not yet justify a new default client class,
a new Engine layer abstraction or weaker immutable-content claims.
