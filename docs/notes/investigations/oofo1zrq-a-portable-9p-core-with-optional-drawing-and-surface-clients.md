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
| D1 | Make command drawing the native default, with surface clients alongside it. | Optional draw class first. niltempus selected Vello CPU/GPU as its implementation family on October 20; default status and a stable wire ABI remain unapproved. |
| D2 | Define an explicit host capability contract for process identity/lifetime, immutable storage, device buffers and display. | A useful design decomposition. POSIX portability does not establish equivalent enforcement on every host. |
| D3 | Import sealed CPU buffers through Linux udmabuf. | Track A is blocked on the installed kernel as well as upstream. Track B, per-resource immutable texture uploads, is ready for comparator preparation after retained-content qualification; no native benefit yet. |
| D4 | Permit reusable producer-writable buffers with fences. | Reopens immutable acceptance and consumer ownership, not just journal wording. Keep existing ISB requirements until that decision is explicit. |
| D5 | Prioritize plain 9P2000, with 9P2000.L adapters. | Changes the accepted delivery order. Determine the first real client and its dialect needs before changing that order. |
| D6 | Host Plan 9 or Inferno. | Remains shelved as a product/ecosystem decision. |

On October 20 niltempus selected the Vello implementation family for the optional
draw service: **vello_cpu for CPU and vello for GPU**. Blend2D is not selected and
is not a dependency candidate for now. This supersedes the handback-03
“backend order held open” status. The existing renderer manifest pins
`vello_cpu = "=0.2.0"`; that is an existing dependency fact, not qualification of
the proposed draw service. Vello-shaped scene vocabulary can guide the prototype,
but neither a stable wire ABI nor production dependency versions/configuration
are selected by this decision. Pinned speed, portability and configuration
verification remains required. D1 as a default client class still needs its own
evidence and decision. Surface clients, Engine ownership and immutable-content
contracts remain unchanged.

Plan 9 raster exactness was reported as unnecessary; compatibility with libdraw
therefore still needs a translator and a stated supported subset.

## Proposed GPU-native content path

The [GPU-native delivery plan and source audit](../plans/h38nf8bk-qualify-gpu-native-shell-images-through-owned-capture.md)
identify existing capture machinery, sealed-only shell integration gaps and
a three-arm GPU-producer comparison. CPU and GPU in this diagram describe pixel
production, not distinct required application classes. ISB remains available
during qualification; its long-term compatibility role is a separate decision.

Following the discussion of GPU-equipped clients, the surface class has two
producer paths. GPU-native submission should be evaluated as a first-class
path rather than requiring GPU clients to read pixels back into CPU ISBs.
This diagram is a proposed integration target, not a claim of implemented
support or measured benefit. The existing image-channel contract already
describes device-buffer capture; delivery and qualification remain separate.

```text
 CPU SURFACE CLIENT              GPU SURFACE CLIENT
 produces CPU pixels             renders into a GPU buffer
         |                                  |
     sealed ISB                          DMA-BUF
         |                                  |
         +---------- descriptor channel ----+
                            |
 Both clients: 9P control --+-- identity, epoch, image record
                            v
              +-----------------------------+
              | IMAGE SERVICE / SESSION     |
              | admit, validate, budget     |
              | own resources and releases  |
              +--------------+--------------+
                             | host operations
               +-------------+--------------+
               v                            v
    +----------------------+    +-------------------------+
    | CPU SOURCE ADAPTER   |    | GPU SOURCE ADAPTER      |
    | verify seals and size|    | validate format/device  |
    | retain/map/upload    |    | synchronize and capture |
    +----------+-----------+    | into owned GPU storage  |
               |                +------------+------------+
               +-------------+---------------+
                             | accepted immutable content
                             v
                    +------------------+
 optional draw ---->| ENGINE / RENDERER|<---- existing X authority
 job: validate,     | compose, present |      admitted transactions
 rasterize, admit   | and retire      |
      ^             +--------+---------+
 draw commands               |
 over 9P                     |
                             v
                    HOST DISPLAY BACKEND
```

The X arrow denotes its existing admitted transaction path, not adoption of
the new surface protocol. Likewise, optional draw output needs its own
resource accounting and transaction admission before Engine consumes it.

The GPU path aims to avoid CPU readback and a subsequent upload. It initially
retains capture into Sophia-owned GPU storage to preserve immutable acceptance.
An acquire/completion fence alone does not freeze a producer-writable source.
Direct composition or scanout from reusable producer buffers is a separate
ownership decision; neither is implied by this diagram. A CPU image can still
be composed by the GPU.

Neither path bypasses custody, grants, connection epochs, journal outcomes or
completion-based release. C28's authority-proof work is therefore separate.
The renderer/source adapters and display backend are host-specific; the
image service's admission and lifecycle contract belongs to the core.

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

## October 20 handback-03 disposition

The original D2 draft, D4 memo/probe, D3 diff and render-node receipts are retained
under `t331-d3-20261020/claude-handback-03` in development evidence. The original
drafts remain attributable to Claude; the qualifications below supersede their
stronger claims. Parent inspection and Muse's independent D3 review were
read-only: neither reran the builds or device controls.

### D3: retained-content qualification is sufficient for comparator preparation

The installed-kernel udmabuf probe reported WRITE- and FUTURE_WRITE-sealed export
refusal, writable aliasing for an export without those seals, and EBUSY when
adding WRITE while that export remained open. It closes track A for the current
ISB contract on the tested kernel; export-before-seal is not a demonstrated
escape. This is retained probe evidence, separate from the upstream source check
above.

Track B retains immutable content and changes only renderer upload reuse. The
resource/candidate audit confirms that later candidates can place the same
accepted resource generation without another upload. Placement may change;
opacity is not a ContentPlacement field and damage is engine-derived. The
identity-bridge controls use real sealed mappings and the resource store. The
reported render-node controls compare every recomposed frame with the uncached
path, mix unidentified fresh content, and re-offer new generations. The retained
48-frame fixture reports seven uploads and 277 hits with equal pixels; those are
fixture observations, not predictions for another workload.

Disposition: enough for comparator import/preparation; no additional render-node
fixture shapes are required before that work. This is not a launch or promotion
gate. A native fixture must actually reuse resources through demand, permit,
candidate and outcome, and its join analyzer must permit uploads independent of
presentation count. A fresh-resource-per-frame panel cannot establish the
retained-reuse benefit. Freeze separately built feature arms and identical peer,
measure actual passes/uploads/hits, both processes' CPU and latency, and retain
pixel equivalence and custody controls. The cache feature controls construction
defaults, not hard API exclusion; the setter can enable either build. The
128 MiB bound is per persistent target, not aggregate device memory. State target
count and retained generations when reporting memory. Keep the uncommitted
`sophia-d3` worktree until import and evidence are settled; no native CPU benefit
or production default is accepted here.

### Comparator import and prepared retained screen, October 20

D3 Track B is imported on `performance/t331-isb-cost` at signed commit
`001e5b45517978012598702b080b42327bfb4244`. The original dirty `sophia-d3`
worktree is preserved. Renderer/cache and real sealed-resource identity tests,
six real peer controls (including four retained resources across 64 rotated
presentations), and strict CLI Clippy pass. The layout audit still reports the
pre-existing direct-library-printing failure in unchanged Session `lib.rs`.
Import patch, hashes and logs are in development evidence
`t331-d3-20261020/comparator-import-01`.

`t331-d3-20261020/retained-native-01` freezes cache-off/cache-on ABBA then BAAB:
eight 1000-presentation runs, one identical CPU-counting peer, four 480x24
sealed tiles in a 1920x24 panel, and only the cache feature differing across
235 matched compiler units. Initial sealed pool preparation precedes measured
CPU windows; this is steady-state presentation cost, not lifecycle cost.
Shutdown records include actual cache uploads/hits, composition passes and
target count. The memory cap remains 128 MiB per persistent target. Production
setter callers are absent, and the cache environment override is removed in
both arms. Twenty-two offline launcher/analyzer controls pass; the native
screen is prepared but has not been launched.

The frozen discriminator requires combined CPU savings beyond within-arm
spread in both blocks and candidate p99 within control p99 plus 1000 usec,
after exact joins, rotated resource references and cache activity qualify.
This evaluates retained texture reuse; it cannot establish the fresh CPU
files/IPC gate. The next action is this prepared retained screen, not another
fresh-panel baseline or additional render-node fixture shape.

The intervening C43-r2 fresh-panel capture at `1bfe3ae8e` was valid but resumed
the wrong investigation. Its three files/IPC pairs reported combined CPU
excess of 27.98–28.40%, 32.85–33.20%, and 43.08–43.43%; all 6000 presentations
joined and no major faults occurred. Temporal drift prevents treating those
pairs as a pooled comparison against older captures. They establish neither
D3 benefit nor the cost of the proposed draw/GPU paths. C43 receipts and
findings remain under `t331-isb-cost-20261018/candidate-43-current-panel-pairs-r2`.

### Retained native capture disposition, October 21

`retained-native-01` completed all eight runs in 519.70 seconds: 8000 exact
presented joins, correct four-resource rotations, no major faults, and clean
PAM/VT receipts. It **does not qualify a D3 CPU benefit**. The frozen analyzer
incorrectly expected presentation-work bytes in `presentation_bytes`; the
runner defines that as non-probe uploaded bytes, which are 184320 here.
A separate post-capture audit corrects the interpretation without altering the
frozen analyzer or its manifest.

More seriously, `NativeEvidenceSnapshot::append` dropped cache and composition
counters before completion logging. All eight records therefore showed zero
hits/uploads/passes despite 4000 draws and 1006 worker completions per run.
These zeros cannot establish cache activation or inactivity. The import missed
this Session aggregation step. Repair `4f5bdd5645f5070f3e9c4ebf8969098c5ec48c60` accumulates the
lifetime counters and preserves current-owner gauge semantics. Its regression
fails before repair and passes afterward; all six evidence tests, strict
Session Clippy with tests, and four corrected analyzer controls pass.

Descriptive candidate combined-CPU means were 3.86% and 2.64% below control in
the two blocks. Neither meets the frozen savings-beyond-within-arm-spread rule;
p99 remained 17.142–17.149 ms. The missing mechanism evidence also prevents
attribution to caching. Findings and original capture hashes are preserved in
`retained-native-01/FINDINGS.txt` and `audit-SHA256SUMS`; repair receipts are in
`t331-d3-20261020/evidence-repair-01`. Captured release binaries are unchanged.
Before another long CPU screen, verify cache hits/uploads and passes through
the actual Session completion path in a short native capture. No rerun was
launched. The fresh CPU IPC gate and production defaults remain unqualified.

### D2: scoped host-proof checklist, with corrected lifetime claims

These are four host-proof groups relevant to this image/identity investigation,
not a complete inventory of everything a portable core needs. No non-Linux host
is claimed equivalent, and host differences need not leave every mechanism
unchanged.

| Group | Required proof and ownership boundary | Current status / port obligation |
| --- | --- | --- |
| Process identity and lifetime | Correlate credentials with a stable process identity; process death is observable. Continue grant, epoch, admission, revocation and expiry checks. | Linux pidfds remain handles to the exited process; they become readable, not reusable identities. Default fresh liveness checks remain; bounded custody dispatch is experimental and not promoted. A port must specify race behavior and detection/observation bounds. No ordering guarantee puts exit notification before every last socket message. |
| Immutable storage | Accepted bytes and bounds remain stable for all consumers; validation applies to those retained bytes. | Current CPU ISBs require the seal/size checks. A host without equivalent write/truncation exclusion needs an owned copy and safe source-copy API, with validation after copying. A consumer read-only mapping is insufficient. |
| Device buffers | The exact source read must complete or be proven unused before source release; retained destination storage is owned and stable. | t339 is experimental, with shell DMA admission still disabled. A copy can race hostile producer writes; containment must preserve bounds/ownership, and future composition uses the owned destination. Producer fences alone do not exclude later writes. Ports must establish completion, generation and teardown behavior. |
| Display | Keep output buffers alive through their actual uses and correlate completion to the submitted frame under stated backend semantics. | A composition-plan checksum is not a cryptographic proof of every displayed pixel. Each backend must define completion and buffer-age semantics; unknown age requires full repaint. |

Death initiates revocation; it does not prove consumers stopped or GPU source
reads ended. Uncertain custody can remain quarantined. Descriptor passing remains
a separate bounded, close-on-exec ingress obligation within the platform layer;
no table entry turns readiness into authorization. These corrections replace the
draft's claims that experimental dispatch is today's universal liveness proof,
that notifications cannot arrive after messages, or that all leases settle just
because the peer dies.

### D4: retain A; B is a decision package; C is not accepted

A is the current sealed-ISB contract. B copies from a producer-writable source
into Sophia-owned immutable resource storage. C composes directly from reusable
producer-writable storage under fences. Recommendation: keep A; consider B only
by an explicit decision; do not adopt C under the current ADRs. No D4 change has
been authorized by filing this memo.

Claude's three single-process probe runs report roughly 55 microseconds for the
fresh panel consumer stages versus 20 for ring copy/validation, and roughly 110
versus 24 for producer work. These are local wall-time observations, **not upper
bounds, native CPU savings, or a partition of the c23 files/IPC excess**. The
probe sequences producer and consumer in one process, assigns last-close work
to its consumer stage, and reuses one destination vector. It does not include
concurrent retained accepted resources, cross-process scheduling, renderer
uploads, protocol traffic or release backpressure. The actual owner of last-close
cost also depends on descriptor lifetimes. Subtracting these timings from c23
CPU measurements cannot establish the remaining bottleneck.

Before B can be screened, specify a race-safe bounded copy API, fixed source
extent, validation of the completed owned copy, and immutable destination
storage retained until its last consumer releases it. Define source-slot release
separately from ResourceReleased, per-generation identity and failure/withdrawal
accounting. The probe's single reused destination is not that implementation.
A generation re-read does not make hostile mutable input safe to expose as a
stable Rust slice. D3 texture caching can apply to B only after the accepted
copy has the same stable identity and lifetime guarantees. A native comparison
would then measure the complete ownership path; further local timing alone
cannot decide D4.

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
