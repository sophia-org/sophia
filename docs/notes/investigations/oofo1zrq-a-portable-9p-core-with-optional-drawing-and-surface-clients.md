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
| D4 | Permit reusable producer-writable buffers with fences. | October 21: niltempus selected exploration of B, copying into Sophia-owned immutable resources, including a seal-free `pread` source adapter. A remains production; C and production promotion are not approved. |
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

### D4: retain A; explore B by owner direction; C is not accepted

A is the current sealed-ISB contract. B copies from a producer-writable source
into Sophia-owned immutable resource storage. C composes directly from reusable
producer-writable storage under fences. Recommendation: keep A; consider B only
by an explicit decision; do not adopt C under the current ADRs. Filing the
original memo did not authorize a D4 change. The October 21 direction below
authorizes exploration of B, not production admission.

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


## October 21 optional Vello prototype

niltempus directed proceeding with the optional Vello producer while Claude and
Muse investigate D4 B/COW independently. The isolated `experiment/t331-vello-draw`
branch implements a bounded CPU command file and feeds its raster output through
the existing worker, immutable resource store and presentation owners. The
[prototype contract and runnable headless demo](../../experimental-vello-draw.md)
record exact bounds, retry/retirement semantics, validation and remaining native
qualification. The 64-candidate fixture retains one panel and updates eight value
tiles using 693 distinct command payload bytes, with no client pixel payload.
It supplies authentication decisions and backend completions: it establishes no
native speedup, protected launch acceptance, default client class or stable ABI.
## October 21 owner direction: D4 B and the portable copy adapter

niltempus, relayed through Claude wB:pH on October 21, explicitly chose to
seriously explore option B from `tmp/d4/D4-DECISION-MEMO.txt`, section 3.
The direction and portability addendum were supplied in the conversation.
The original memo and probe were subsequently located in retained evidence at
`t331-d3-20261020/claude-handback-03/d4`; working sources are under
`~/.claude/jobs/c55837ea/tmp/d4`. Original timing claims called bounds remain
superseded by the correction above.
The live master repair/release takes priority. Do not start a D4 implementation
or the ring-producer port until the release is out and the record/lifecycle
sketch is aligned. This is a draft amendment to
[qujwzrdz](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md)
and [mn4mzcnf](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md),
not an amendment of their accepted production contracts yet.

### Proposed guarantee

The source ring remains producer-writable. The accepted resource is a separate
Sophia-owned destination, copied once for that resource and validated **after**
the copy. Consumers receive only the immutable destination and its ordinary
resource identity, never a borrowed stable Rust slice of the mutable source.
A producer's later writes cannot change accepted pixels. Source races cannot
change the trusted dimensions, offsets, ownership or memory bounds. This uses
the same containment principle as owned DMA-BUF capture in
[the image-channel contract](../../sophia-image-channel.md#dma-buf-completion-and-release),
and merits comparison with the X MIT-SHM copy-out implementation. Neither
precedent proves the proposed ring implementation correct.

The destination and any renderer leases live through the ordinary
`ResourceReleased` lifecycle. Charge every accepted generation and peak copy /
upload overlap, independently of the number of source slots. A two-slot ring
must not imply a two-resource resident limit or permit recycling a destination
still held by composition. D3's immutable texture cache can consume these
resources without changing its identity/lifetime contract.

The phrase “cannot admit a torn frame” needs an explicit threat model before
alignment. A conforming producer relinquishes a slot from offer until release;
an odd/in-progress, changed or unexpected generation rejects the offer, counts
a torn-read observation and never silently retries it into an accepted frame.
But an adversarial writer can change pixels without changing a generation word.
Two equal reads of producer-controlled metadata do not prove a coherent snapshot.
Copying still isolates subsequent writes, and validating the owned result still
protects the pixel contract. Absolute hostile-writer snapshot consistency would
need additional exclusion/snapshot enforcement; it is not supplied by B's
proposed cooperative handoff or generation check alone. Do not conflate these
claims or weaken bounds safety while resolving the distinction.

### Protocol sketch for alignment, not allocated wire records

| Operation | Required identity, bounds and obligation |
| --- | --- |
| Ring admission | Bound connection/grant and channel epochs; fresh ring identity/generation; supported backing kind; slot count, slot extent, row stride, data and generation offsets; checked total extent and arithmetic. Admit the fd on the existing authenticated descriptor channel. Copy geometry into trusted owner state once; reject incompatible descriptors and capacity before taking obligations. |
| Slot offer | Ring identity, slot index, fresh slot generation, offer transaction, resource `(id, generation)`, bounded image dimensions/format under the admitted geometry. At most one outstanding use per slot. Source bytes cannot redefine offsets or lengths. Reserve destination bytes, worker credit and terminal journal capacity first. |
| Slot release | Exact ring, slot, generation and offer identity; an explicit terminal disposition. Emit only when no source read remains. This permits rewriting that slot, not reuse of the accepted destination. Preserve pending release under live-journal backpressure. |
| Resource lifecycle | Existing ResourceStatus, candidates, placements, outcomes and ResourceReleased. Copy completion is not presentation. Retire/revoke prevents new use; backing remains charged through actual consumer release. |

Generation width, alignment, byte order, no-wrap behavior, publication ordering
and host cross-process synchronization semantics remain design requirements,
not assumed properties of an integer read. Ring replacement/close must drain
all outstanding source reads. A dead journal permits no promised slot-release
event; the client must treat unresolved custody as unknown rather than replay
or reuse. Cancellation/expiry rejects interest but does not prove a copy stopped.
No new reactor: keep ContentLimits, custody, epochs, export admission, revocation,
the 20 ms authentication cadence and runtime_driver visual sequencing intact.
Readiness remains distinct from authorization. This is an optional addition
beside sealed ISB; withdrawn byte fallback stays withdrawn.

### Two source-copy adapters to compare

* **Mapped source:** verify GROW and SHRINK seals and fixed extent before mapping;
  no WRITE seal is required. Copy through a narrowly audited mutable-source API
  into owned storage, never by granting ordinary immutable Rust borrows over
  producer-writable memory. Keep generation checks and post-copy validation.
* **Unmapped source:** Session never maps any part of the ring, including its
  generation word. Use bounded positional reads into owned buffers. Treat EOF,
  short reads, errors, generation mismatch and cancellation as explicit failed
  offers; do not concatenate partial reads across slot revisions or silently
  recopy. Truncation then does not invalidate a source mapping inside Session.
  Account for generation reads and all syscalls, not just the pixel read.

The second adapter removes a seal requirement for source truncation safety,
but “every POSIX host supports shm_open + pread” is too broad. POSIX specifies
pread in terms of read and leaves reads of shared-memory objects unspecified.
A port must select a seekable backing object with supported positional reads,
for example a suitably provisioned regular file, or verify its host-specific
shared-memory implementation. [POSIX read/pread specification](https://pubs.opengroup.org/onlinepubs/9799919799/functions/read.html).
Descriptor passing, backing provisioning/permissions, producer publication
visibility and nonblocking owner progress also require host evidence. Keep
size seals as an additional check on Linux where available; do not call their
admission/setup cost zero without measuring it. D2's immutable-storage group
can admit ownership plus a proven safe-copy adapter, not require Linux seals
universally or declare all POSIX ports qualified.

A CPU-owned destination generally still needs its renderer upload. The proposed
GPU-visible owned destination might replace that upload; this requires a
separate native proof of write visibility, validation, synchronization, layout
and retained lifetime. A linear-to-tiled GPU blit may remain. Do not advertise
one end-to-end copy, zero-copy, or vendor-independent GPU mapping from the
CPU copy probe.

### Work and measurement order

1. Claude's proposed useful design work is the temporary probe with one owned
   destination per accepted resource retained until release, plus draft ADR and
   protocol text. Compare fresh sealed ISB, mapped ring copy and positional-read
   ring copy at identical panel and bulk sizes. Include ring setup/amortization,
   producer and Session stages, page faults, bytes, peak overlapping resources,
   release backpressure, and refused/torn offers. These remain local observations.
2. Align the above lifetime, source-race, portable backing and record semantics;
   then port a ring producer for an isolated measurement harness. No production
   source-kind number or stable ABI is allocated by this sketch.
3. After the release, freeze paired native fresh-ISB versus ring captures with
   identical frames, panel/bulk geometry and completion semantics. Measure both
   processes, CPU, latency, faults, memory and source/resource release outcomes;
   apply the existing t331 +10% gates. A native harness is necessary for this
   screen; local timings cannot substitute for it. D3's retained-content
   qualification remains a different workload.
4. Only a passing native screen supports proposing a production integration
   lane, default-off build feature and same-binary kill switch. No default-on
   promotion follows from this direction or from the standalone probe.


### October 21: retained-destination probe and current sequence

Vello's default-off prototype merged as signed `2464e1d4b`; niltempus deferred
the authenticated draw client/native qualification as `t341` and directed
continuing B. Claude's COW rejection is recorded in the separate `8sjpt4pf` note;
B continues with an explicit owned copy, not page-protection snapshotting.

The new [standalone retained-resource probe](../../../tools/probes/d4_owned_ring/README.md)
compares fresh ISBs, mapped copy and sealed/unsealed pread. Three rotating source
slots and four independently retained destinations address the original lifetime
omission. Four controls pass for source reuse, changed-generation refusal, owned
pixel validation, missing seals, fixed geometry and truncation without a consumer
source mapping. Strict Clippy passes. These remain serialized local observations,
not production memory-safety or native scheduling/CPU acceptance.

Three forward/reverse/forward runs, 128 measured iterations per cell, are retained
in `t331-d4-owned-copy-20261021/probe-01`. Ranges below are the three run medians
for acceptance plus old-resource release, in microseconds; payload retention is
four resources and source-ring setup/final teardown are separate records.

| Payload | Fresh sealed | Mapped copy | Sealed pread | Unsealed pread |
| --- | --- | --- | --- | --- |
| 1920 x 24 | 53.51–53.66 | 20.90–21.35 | 26.34–26.89 | 26.25–26.83 |
| 1024 x 1024 | 1026.12–1032.71 | 523.02–526.36 | 610.85–701.67 | 610.81–697.65 |

All samples had zero major faults. The bulk pread spread shows sensitivity to
allocation/cache conditions; there is no savings confidence interval or native
benefit claim. Fresh last-close cost is charged to Session by this model, not
established as its native owner. The producer fill loop also differs from the
old probe. The raw samples include generation syscall overhead, new per-resource
destination allocation and resource retirement, rather than timing memcpy alone.

Use pread for the first integration experiment: it avoids mapping a hostile
mutable source and gives explicit short-read refusal with an owned destination.
Retain mapped copy as the local comparison; do not promote it as a production
racy-read API merely because this serialized probe succeeds. Next integrate
actual bounded ring admission/copy/slot release with immutable resource custody
in an isolated native comparison path. Source-slot release and ResourceReleased
must stay distinct, and the proposed +10% native gates remain unchanged.


### October 21 addendum: two measurement bars and external-work split

Niltempus prioritizes battery and CPU headroom on older computers. B therefore
first removes the per-frame create/fallocate/first-touch/seal/map/unmap/close
lifecycle; a retained copy is not itself a failure. The c23 retry's reported
panel excess (files 1.567–1.619 versus IPC 1.228–1.240 ms/frame) motivates the
experiment, not a transferable prediction for another CPU or proof of which
individual kernel operation dominates. Record both absolute work and ratios.

Use two separately named bars in FINDINGS:

* **IPC regression floor:** retain the frozen t331 +10% gates. Passing them
  means the replacement meets that regression screen, not external parity.
* **External reference:** work per delivered frame versus Wayland and XLibre
  with equivalent visuals, latency, throughput, buffer ownership and PID-set
  accounting. Numerical external acceptance thresholds remain to be frozen
  with the delivered gate proposal; never choose them after seeing results.

Run the same paired recipe and gates on crunch and a second, slower target
(the x13). Do not pool machine results or extrapolate crunch's microseconds.
Panel cost measures active work, not idle wakeups or battery life. Keep the
approved 20 ms authentication cadence unchanged; idle wake rate and direct
energy measurement would be separate experiments.

There are two external workloads. The whole-desktop wmbench recipe should have
Sophia, XLibre and a Wayland arm, preferring River with sway as the fallback
if required protocol coverage is missing. Freeze the common test rows after
protocol inspection; unsupported tests stay “not done”, never zero cost. Use
both heads, 120 warmup and 300 measured frames, pinned builds, explicit desktop
PID sets and capture artifacts. Refresh the prepared native retry against the
actual current master before running; old READY files are not current receipts.
Then repeat with B to measure its delta. Software QEMU results and source-only
niri review do not establish matched native GPU performance.

The separate shell-path pair uses a fresh whole-panel fill (1920 x 24 at (0,0))
and a 4 MiB bulk arm, matching frame count, cadence and heads. Record client and
server/compositor CPU per delivered frame, completion counts, faults, and
submit-to-present p50/p95/p99 where wp_presentation or XPresent supplies genuine
presentation feedback. A copy-complete/SHM-buffer-safe event is not presentation.
Document the external paths' cooperative mutable-buffer release and absence of
Sophia's evidence journal and resource identity/checksum semantics. Do not
silently normalize those differences away.

Claude wB:pH and Muse wB:pJ own temporary standalone reference clients, their
shared fill/journal/accounting code, protocol coverage and reference launch
recipe, plus the proposed gate text. Their stated boundary is job tmp only:
no desktop/server/Session launches and no production, comparator, notes or
evidence edits. This lane owns importing and reviewing receipts, native recipe
integration, gate changes and filing. Avoid duplicating their client work or
claiming their deliverables are already received. Seated runs remain pending;
no external shell-path or matched native three-way result is claimed here.


### Isolated B storage/worker foundation (not live image admission)

The runtime's default-off `experimental-owned-copy` feature now provides the
measurement foundation. `CpuPixelValidation` scans either the existing sealed
mapping or a consumed owned Box; the old ISB names remain compatibility aliases.
Acceptance moves the same allocation into the ordinary resource store and CPU
renderer leases. A shared view keeps an Arc of the Box, avoiding conversion to
Arc<[u8]> that would copy the payload. Per-resource staging/resident/retiring
charges and final journal-confirmed release use the existing store.

`OwnedRing` fixes geometry at admission and lends a non-cloneable read ticket to
the existing four-credit ISB worker. The experimental layout has 1–4 slots,
4 bytes–4 MiB of pixels per slot (a multiple of four), a 4096-byte header,
page-rounded payload, and a little-endian u64 ready generation at offset zero.
The first checkpoint checked source extent against a claimed reservation;
the source-budget amendment below replaces that claim with a registry lease.
Only an exact-size regular file is admitted. Session
maps none of it. The worker makes two bounded generation reads around one
payload pread and rejects short/error reads or changed generations, without a
retry. It scans only the copied destination. Cancellation/deadline checkpoints
surround syscalls and validation chunks; a blocking regular-file read still has
no hard duration bound. The local Linux memfd tests do not qualify all backing
files or other hosts.

The ticket retains the source fd while reads may occur. Dropping it marks only
source-read completion. The slot owner keeps its exact offer/slot/generation
release pending until journal confirmation; cancellation, resource retention
and merely observing the release do not free the slot. Offer ids increase across
the ring, each slot's even nonzero generations increase without wrap, and failed
admission leaves those high-water marks untouched. A changed-generation outcome
is distinct for future evidence counting. Equal words still do not authenticate
producer pixels or prove a hostile writer's coherent snapshot.

Tests cover destination pointer identity and retention, resident limits,
revocation/cancellation/expiry charges, invalid pixels and partial scans;
reusing two source slots while eight immutable destinations survive; source
truncation; exact-release identity and backpressure; refused submission custody;
and deterministic generation change/cancellation between source reads.

This feature is reachable by isolated tests/harness APIs only. There is no ring
wire allocation, SDK change, authenticated source admission,
journal slot-release record, native recipe, live enablement
or performance acceptance yet. The export owner must bind ring/connection epochs,
reserve source and per-resource budgets plus journal credits, retain source
charges through disconnected workers, and reconcile every terminal completion.
The next integration step is that owner and its negotiated experimental record
path on the existing descriptor channel and journal. Do not silently reuse the
sealed source kind, bypass export.check, add a reactor, or patch SDK snapshots.
Native measurements must still pass the regression floor and report external
references independently before proposing a production path.

### Source-budget custody amendment — October 21

`OwnedRing::admit` now consumes a non-cloneable reservation minted by the
Session's `ContentEpochRegistry` for the exact active content grant and fixed
geometry. It cannot accept a caller's claimed byte allowance. The full source
extent consumes both aggregate byte and backing budgets, independently of
every destination's staging/resident/retiring reservation. Existing active
resource ceilings remain reserved; source admission cannot borrow their space.
Stale or revoked grants and exhausted budgets refuse before admission.

The ring and each worker ticket share one source owner. Its descriptor closes
before its reservation drops. A disconnected epoch retains the source charge
and its retirement slot until the last potential reader is gone, and reconnect
admission counts that charge. Slot release permits reuse but does not refund
the live ring extent. A malformed descriptor refunds an unused reservation.
The read-only accounting inventory exposes source bytes separately from owned
resource bytes; neither is a claim about process RSS or physical page residency.

Four budget regressions cover independent source/destination charges, malformed
admission refunds, revoked/stale grants, disconnected readers and reconnect
pressure, and live-ring retention after slot release. The runtime feature suite
passes 725 tests (41 ignored), and strict all-target Clippy passes with owned
copy, drawing and capture enabled together. Evidence is retained under
`t331-d4-owned-copy-20261021/source-budget-01`. This closes the internal source
accounting gap; authenticated ring admission and its journal obligations remain
unimplemented. No native or external performance result follows from these tests.

### Owned-copy image job custody — October 21

The default-off experiment now connects ring reads to the persistent CPU image
job lifecycle and actual allocation/resource owners. `OwnedCopyImageRequest`
is an internal type with exact output/allocation generations, topology,
destination dimensions and resource identity, plus offer/slot/generation. It
does not serialize as sealed ISB metadata or allocate a public source kind.
The ordinary sealed path retains its existing types as aliases of the shared
job lifecycle; both use the existing validation worker and resource store.

Reservation checks the ring's actual grant and source-accounting owner, then
the live allocation, destination layout, fixed ring payload size and available
slot identity before consuming destination and worker credits. Another registry
with coincidentally identical epoch numbers cannot take a source charged to the
first registry. Submission additionally checks an opaque in-process ring identity
and the exact offer/slot/generation. Refusal returns the read ticket intact.
The identity token keeps no source descriptor alive after its readers are gone.

Completed copies stay in the bounded job table until their exact resource owner
consumes the result, including across clock regression and disconnect. Before
owned-copy acceptance, the current allocation-derived description must still
match the reserved one. Cancellation, expiry, revocation and allocation loss
retain existing store-side stop precedence. Accepted resources retain their
separate immutable allocations through consumer and journal-confirmed release.
Slot release remains a different obligation and is not consumed by resource
settlement or refusal.

Controls cover three accepted resources retained while one source slot is
reused, source truncation after copying, distinct source/resource releases,
cross-ring and wrong-generation submission, cross-registry custody, invalid
allocation/topology/dimensions, clock retry, and cancellation/expiry/topology
change/disconnect after a successful copy. Validation receipts are under
`t331-d4-owned-copy-20261021/owner-jobs-01`. The first retention fixture supplied
a regressed time on its second reservation and was corrected to use increasing
times; the runtime's clock rejection was correct.

The owned-copy runtime suite passed (729 reported passes, 41 ignored) before the
last cross-registry guard; the final 28-test allocation group includes that guard
and passes. Strict runtime Clippy with draw/capture/owned-copy enabled passes.
The first full workspace run failed in the unchanged Session stale-socket test,
which observed its just-dropped listener as active. The exact same test binary
passed in isolation; the complete `RUST_TEST_THREADS=1 cargo xtask check` then
passed, including that fixture, SDK checks and render-node regressions. Both
workspace results remain in the receipts; the parallel failure's cause is not
established by its successful retry.

This closes the internal job-custody connection, not authenticated wire ingress.
The export owner must still reserve terminal and slot-release journal credits,
perform custody/export checks, bind negotiated ring/channel identities, and
deliver distinct slot-release records. Host backing qualification, the live
feature/kill switch and native comparison remain outstanding. No live Session or
desktop was launched. Acceptance follows the later four-workload external-bar
amendment in `mvbubiv9`; older IPC +10% wording above is historical and superseded.


### Joint handback supersedes the earlier IPC-floor decision

Niltempus's subsequent explicit direction removes the four IPC +10% gates from
t331 acceptance. IPC is now an ungated diagnostic, and Wayland/XLibre is the
performance bar. The [accepted plan amendment](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md#accepted-performance-direction-amendment--october-21-2026)
supersedes this note's earlier “regression floor” and “unchanged +10% gates”
requirements. External numerical thresholds remain unset; R=2.0 is a proposal,
not an approved ceiling. Whole-desktop wmbench belongs to t289 separately.
The handback's clients are received but require review/correction and protocol
smoke before native capture. No IPC or external pass is inferred by this change.

### Private ring transport and settlement provenance — October 21

The default-off experiment now has an authenticated private control path,
existing-image-channel descriptor delivery, held registration/status/slot-release
journal credits, and a persistent owned-copy owner. The wire and lifetime
contract is documented in [experimental-owned-ring](../../experimental-owned-ring.md).
The CLI and Session forward the build feature; the same-binary
`SOPHIA_DISABLE_OWNED_COPY=1` switch suppresses disclosure and admission. This is
not a stable SDK extension. The live adapter currently requires Linux GROW and
SHRINK seals; the seal-free pread exploration remains separately qualified.

Review closed same-grant withdrawal, impossible registration budgets, descriptor
drop ordering, refusal classification and cancellation settlement gaps. Only an
authoritative resource-store final event can now spend a transfer's held status
credit. Failed control replies cannot spend it, even when their transaction equals
the offer. Cancellation may legitimately report its own transaction. Slot release
remains separate from accepted-resource release.

The authenticated in-process ring fixture uses the real owner and worker with a
supplied test authority. Withholding completion delivery until two failed retires
and cancellation are applied makes the cancellation barrier deterministic. Shared
raw arrival ordinals prove status 4 precedes the exact cancelled SlotReleased,
then same-slot reuse succeeds with status 2 before SlotReleased. This does not
qualify native authentication, a live Session or performance.

The review also reproduced two existing sealed-path defects with default features:
cancel transaction 999 left one owner slot pending after its terminal status; a
failed retire under the offer transaction spent the hold and allowed SourceReleased
before successful settlement. Shared provenance now fixes both independently of
the owned-copy feature. Original failing traces and passing run-3 are retained in
`~/.claude/jobs/c55837ea/tmp/sealed-provenance/`.

The combined runtime suite reports 779 passes and 41 ignored tests, with owned-copy,
drawing and capture enabled. Strict all-target Clippy passes after replacing a
duplicate test-helper mount with the existing shared alias. Source-layout and
format checks pass. Evidence: `t331-d4-owned-copy-20261021/wire-owner-01`, including
failed attempts; these checks precede the new timing-peer example.

Next is the native timing-peer arm and typed runner schema 5. The stable SDK
correctly rejects the private D4E1 framing, so the experimental peer must own its
post-authentication 9P/event decoding on the same connection; no SDK snapshot
patch or second-connection resource shortcut is permitted. The first panel arm
records registration, copy acceptance and slot release separately, then uses
the existing kernel-presentation join. A matched sealed arm must use equivalent
peer sequencing before attributing performance differences to the transport.
Native measurements, the four-workload external comparison and the x13 run remain
outstanding; no external threshold or production-default promotion is implied.
