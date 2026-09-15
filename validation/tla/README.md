# Visual Transition Model

**Role:** bounded validation model, not production architecture or a refinement
proof of the Rust implementation.

`VisualRetirement.tla` explores the asynchronous lifetime of immutable visual
candidates across two outputs and two generations. Each logical output is backed
by one or more heads, and one output is a two-head mirror group, so a single run
exercises per-head preparation, partial physical submission, joint retirement
within a group, and independent retirement between groups. A transaction may
name both outputs, but its feedback waits for both output retirements while each
output retains its own committed generation. This is the smallest scope that
still explores out-of-order retirement, supersession, and mirror-group member
loss. It excludes X11 objects, application metadata, pixel content, renderer
handles, and native KMS objects.
The join is stated over heads and flips rather than over buffers, which is what
lets the scanout architecture change without the model changing. It held when a
mirror group shared one framebuffer and holds unchanged when each head owns a
buffer at its own mode: what makes a group presented is that every screen shows
the frame, not that they read it from the same memory.

The model checks that:

- every submitted head belongs to a completely prepared output cohort;
- no physical head or partially submitted output cohort is in flight for two
  generations, and overlapping output cohorts cannot both be active;
- a successful commit follows retirement of every output required by that
  candidate, and a logical output retires only when every one of its heads has
  flipped;
- distinct logical outputs never share a head, so one group's flip cannot retire
  another group's output;
- a candidate whose mirror group lost a head settles as `head_lost`, never
  commits, and drops the lost lease without counting it as a flip;
- no candidate reaches a terminal outcome while a required output remains
  logically pending;
- a late generation loses an output to the newer one that already committed it,
  and loses it before the kernel rather than after: supersession reaches only
  outputs with no submitted head, a superseded output never becomes that
  output's committed or input generation, and a superseded candidate never
  publishes feedback;
- input never leads physical visual state, and a successful transaction publishes
  matching input only after every required output retires;
- successful feedback exists only for a committed generation;
- active, submitted, or committed resources cannot be released, which now also
  means no generation is released while any head still scans it out; and
- admitted work eventually reaches one terminal settlement without arbitrary
  failure being available to satisfy it.

The checked configuration explores 2,432,103 generated states and 968,679
distinct states to depth 28.

`VisualRetirementSlots.tla` is the focused Milestone 14 extension. It retains
one two-head mirror output and orders four generations through three complete
native target slots, allowing displayed, submitted, and prepared work to fill
the pool while a fourth generation defers. Slot tokens combine a stable slot
identity with a monotonically increasing incarnation. Exact release after
replacement frees a slot; reuse advances its incarnation; and one delayed old
release is explored after reuse without changing the current owner. Failure is
not release, and the first mirror callback cannot free a generation. The
default four-generation configuration checks the new safety boundary; the
parent model retains the admitted-work liveness proof, while `FairSpec` and the
focused progress formulas remain available for smaller diagnostic runs.
Scenario correspondence and implementation-only checks are recorded in
`validation/specula/native-frame-slot-retirement-modeling-brief.md`.
The bounded safety configuration explores 4,149,619 generated states and
1,100,230 distinct states to depth 34. Temporary negative controls independently
allowed acquisition of an occupied slot and let a stale return clear a reused
slot; both violated `ActiveGenerationOwnsSlot`, at depths 5 and 8 respectively.

No model expresses a timing or deadline property, and the input-latency row
does not add one. `FrameServiceArbitration` already checks the structural half
of that row -- `OneSubmissionInFlight` over a boolean `kmsInFlight`, with the
latest-wins pending frame as a boolean beside it -- so what remains is
empirical: how long input takes to reach a photon on a particular display
under a particular workload. A deadline model would introduce a clock variable
to restate a bound that only hardware can answer, and would then need its own
argument that the modelled clock resembles the one being measured. The
physical gate is the proof; this paragraph is here so the absence reads as a
decision rather than an omission.

`VisualDamageHistory.tla` is the successor model for bounded buffer-age damage
history over that promoted pool. It abstracts the output as a region partition
and content as a generation mark per region, never pixels. A scene generation
damages a nonempty set of regions whether or not it is ever rendered, so a
deferred generation that is superseded still contributes the work a later slot
write owes. A slot may be written fully, or partially against the damage
accumulated since the generation its content was written for; a rebuilt bundle
and an incomplete write both leave it with no usable history and force the next
write to be full. The property is that a slot brought up to the current scene
holds what a full repaint would have produced, stated once over the result and
once over the damage so a counterexample names the region that was owed.

The model carries neither a lease incarnation nor a second head. A slot's buffer
keeps its content across release and reacquisition, and that persistence is what
makes buffer age worth anything, so history dies with the bundle rather than
with the lease; `VisualRetirementSlots` owns the lease identity that rejects a
stale release. Per-head history is the same indexing property applied twice. An
earlier configuration carrying the incarnation dimension passed 14 million
distinct states at depth 12 without exhausting its queue, which bought state
space rather than a distinct failure mode.

The checked configuration explores 3,643,747 generated states and 415,585
distinct states to depth 12. Temporary negative controls independently narrowed
a partial write to the current generation's damage alone, and let a rebuilt
bundle keep the generation its lost pixels were written for; both violated
`RepaintMatchesFullRepaint`, at depths 6 and 4 respectively. A third control
checked `PartialWriteIsReachable` and found it violated at depth 4, confirming
the optimization is admissible in the model rather than vacuously safe.
Scenario correspondence, the implementation-only checks, and the open question
of where a slot's content age actually comes from are recorded in
`validation/specula/buffer-age-damage-history-modeling-brief.md`.

`StableBackingLease.tla` is the same shape one level down, and one property
further. `VisualDamageHistory` owns what a reused target slot already contains;
this owns what the renderer's registry copy of one stable software-rendered X
toplevel contains, given updates applied while presentations still hold the
bytes. It exists as a separate model rather than an extension because the
exclusion above is load-bearing there and false here: a slot's buffer keeps its
content across release, which is what makes buffer age worth anything, whereas a
client raster under copy-on-write is defined by a live lease deciding whether a
mutation may touch the allocation or must copy it first.

Allocations are modelled as identities so sharing and splitting are observable,
and a lease captures the content it was handed so immutability can be checked
rather than assumed. An update is modelled by the region set it covers, so a
coalesced batch is a superset cover and a full replacement covers everything;
rectangle arithmetic and the transport's rect capacity stay out. The properties
are that a held lease still reads what it captured, that a registry brought up
to the current generation holds what a full replacement would have produced,
and that live allocations never exceed one plus the number of held leases --
which is how "bound the backing storage" is stated so that an unreclaimed split
appears here rather than as growth on a physical run.

The checked configuration explores 1,270,260 generated states and 251,236
distinct states to depth 14. Six temporary negative controls each produced a
counterexample: an in-place apply under a live lease violated
`LeasedContentStable`; an update covering less than the damage it owed, and a
stale update overwriting newer content, both violated `RegistryMatchesStore`;
retirement that stopped checking for a sibling lease, and a resize that freed
the old epoch while a lease held it, both violated `LeasedAllocationsLive`. The
sixth checked `SplitIsReachable` and found it violated, confirming the
optimization is admissible in the model rather than vacuously safe -- a model
where no split can ever occur satisfies every safety property above and
describes a system that shares nothing and copies always.

Scenario correspondence and the implementation-only checks are recorded in
`validation/specula/stable-x-backing-lease-modeling-brief.md`.

`ContinuousContentPresentation.tla` owns the post-readiness software-content
pipeline that the physical terminal gate measures. It keeps unbound intake,
native-owned composition, submission, kernel flip, and exact callback reduction
as separate actions. Latest-wins applies only to the unbound identity. Multiple
composed, in-flight, or callback-owned generations may coexist, and intake may
not supersede them. Every accepted update must be presented, superseded, or
held by an exact pipeline owner; a presented update must belong to the retired
set; and every pipeline identity must have been accepted. Under weak fairness
for each productive stage and exact callback, the final bounded source update
eventually settles. Timing is intentionally absent: the schema-3 empirical gate
owns one-second source/display gaps and the two-refresh
update-to-retirement bound.

The checked configuration explores 142 generated states and 82 distinct states
to depth 16. Five retained executable negative configurations make the model's
assumptions observable rather than documentary:

- removing exact callback-drain fairness violates the temporal property;
- removing composition fairness violates the temporal property;
- accepting a successor without accounting the superseded unbound update
  violates `AllAcceptedUpdatesAccounted`;
- superseding a composed, in-flight, or callback-owned generation during intake
  violates `NativeOwnersAreNotSuperseded`; and
- allowing a stale callback to present the latest pending update violates
  `PresentedUpdatesRetired`.

`tools/check_tla.sh` requires those five runs to fail with their exact expected
TLC signatures. A negative control that starts passing is therefore a checker
failure, not a model improvement.

Fairness is per action rather than over the whole progress disjunction, and
`Settle` is outside it. That distinction is the difference between a settlement
property and a tautology: `Settle` is enabled in every non-terminal state, so a
single lumped assumption is discharged by failing, and the property holds even
with no productive action assumed fair at all. A temporary control restoring the
lumped assumption confirms this -- it passes while assuming nothing about
preparation, submission, retirement, or completion. Head loss stays outside
fairness because nothing guarantees a connector disappears; page-flip callbacks
stay inside it because a kernel that accepted a flip does report it.

Temporary negative controls independently remove the prepare-all guard,
whole-cohort ownership guard, transaction feedback join, release guard,
submit-time staleness guard, and supersession guard, and weaken joint
retirement to accept one flipped head; each violates its corresponding enabled
invariant, reaching `SubmittedOutputsAreNeverSuperseded`,
`OutputGenerationDominatesHistory`, and `OutputCommitAfterHeadRetirement`
respectively for the last three.
Removing supersession from the fairness assumption violates the settlement
property, which is what shows that property is now carried by work that
advances.

`XAuthorityShutdown.tla` joins the two sides of terminal settlement that the
continuous-content model deliberately leaves separate: producer ownership at
the X Authority/Engine boundary and retirement of the final accepted CPU
update. Producers hold at most one current transaction, exact-ticket delivery
preserves order without a relay-owned collection, and forced cancellation
remains reachable while any producer is held. Normal completion additionally
requires the frontend channel, local authority queue, pending CPU update, and
native in-flight work to be empty.

The checked configuration explores 751 generated states and 302 distinct
states to depth 19. Two retained negative configurations reproduce the original
service exit while producer work remains and unbounded relay-style ingress;
they must violate `NoUncancellableEgress` and `BoundedProducerOwnership`,
respectively. `tools/check_tla.sh` verifies both exact failures. Scenario
correspondence, Rust regressions, and implementation-only deadline checks are
recorded in
`validation/specula/x-authority-shutdown-modeling-brief.md`.

`PresentFrameOwnership.tla` isolates the output-frame association needed by
software Present. It allows an unrelated frame to submit and retire before the
Present frame, but requires feedback to remain false until the exact bound
frame retires. It also permits the backend to submit a successor DMA frame
after observing the Present retirement but before reducing its feedback. That
successor cannot steal or block the captured retirement. Under weak fairness,
waiting Present work eventually reaches settlement.

`PresentCopyOwnership.tla` isolates composited DMA-BUF ownership. Capture
creates a staged compositor image without releasing the client source. Exact
page-flip retirement promotes that image, makes the source idle, and only then
permits Copy completion. A failure before retirement instead removes the
staged image and settles Skip. The model forbids client-owned content from
becoming the displayed owner and requires every staged Present to reach Copy
or rollback under weak fairness.

`SurfaceContentStream.tla` models a Present followed by the three representative
software operations in the X11 content stream: SHM, clear, and core drawing.
It requires those operations to remain bounded and FIFO until the exact Present
retires, while unrelated work may progress. Retirement publishes the Present
generation before any deferred operation can advance the visible generation;
weak fairness then requires the complete backlog to drain.

`GeometryFeedback.tla` separates a surface's full X geometry from pixel
readiness. It explores move-only and resize commits, an unchanged proposal,
control delivery before or after logical commit, and a timeout whose FIFO
rollback follows a late target control. It requires move-only work to reuse
committed pixels, resize commits to wait for pixels, unchanged geometry to
remain silent, and every settled Engine/X Authority pair to converge. This is
the protocol invariant behind `ConfigureNotify`: position feedback is not a
resize side effect.

`PolicyConnection.tla` models one exclusive public policy role across
negotiation, bounded begin/chunk/end proposal assembly, disconnect, replacement,
and settlement of work already queued by the transport worker. It checks that
selected revisions and capabilities are mutually supported, no transfer begins
before negotiation, only complete work is admitted, stale queued work is
discarded, and responsive negotiation and transfer work eventually settle.
The bounded configuration explores 3,321 generated states and 2,177 distinct
states to depth 23.

`PolicyProjection.tla` models the first public WM interface's complete scene
snapshot and affected-output replacement semantics. It explores stale snapshot
generations, invalid focus or visibility, surface removal, timeout, disconnect,
and a replacement policy epoch. It requires live unique projected surfaces,
visible focus, one logical multi-output commit, no policy commit on rejection,
and last-projection preservation while policy is absent.
The bounded configuration explores 1,342,325 generated states and 524,396
distinct states to depth 18.

`PolicyLifecycle.tla` models the causes around that projection boundary. It
keeps distinct activation identities in FIFO order, prioritizes them over
replaceable policy-dirty and pointer updates, applies configuration only at an
idle boundary, consumes a saturated shortcut as an explicit bounded rejection,
settles opaque session operations exactly once, cancels pending interactions,
retries against a fresh scene when frontend facts change, and preserves the
last-good serial until a settled proposal commits. Repeated
opaque action tokens use distinct activation identities and therefore cannot
be collapsed into one focus, movement, view, or layout operation. The bounded
configuration explores 348,608 generated states and 65,467 distinct states to
depth 26.

`PolicySettlementRecovery.tla` isolates the public Hagia owner-loop boundary
after proposal validation. It separates non-mutating staged revalidation,
frontend settlement, one logical reducer/layout promotion, capacity-one
terminal delivery, transport loss, forced abort, and connection replacement.
It requires the last-good reducer and layout serials to remain coherent, a
failed settlement to preserve both, old-epoch terminal ownership to disappear
before restart, and each delivered terminal identity to occur at most once.
The bounded configuration explores 314 generated states and 224 distinct
states to depth 36. A temporary prepare-time reducer promotion immediately
violates `LastGoodIsCoherent`, preserving the implementation defect found by
the modeling pass as a negative control.

`PolicyOutputSettlement.tla` isolates output loss and exact-identity return
during a complete two-output policy settlement. Each output carries the head
set backing it, so a mirror group that loses one connector while keeping its
identity is a scene change like any other. It requires the canonical scene
generation to fence the staged candidate against both the output set and the
head sets, both Engine layout and reducer projections to promote as one
last-good state, and a returned output to carry a new generation. The bounded
configuration explores 2,070 generated states and 1,369 distinct states to
depth 13. Temporary negative controls independently allowed a prepared
candidate to skip its final topology recheck, allowed an output to return
without advancing generation, and allowed a head loss not to advance the scene.
TLC violated `CommittedTopologyWasCurrent`, `ReappearedOutputIsFresh`, and
`StagedHeadsCannotGoStaleSilently`, respectively.

`OutputTopologyLifecycle.tla` carries that policy-level topology rule through
the native session owner's split ownership-transfer boundary. It separates a
replaceable rescan hint, routed-input epoch revocation, old scanout retirement,
complete runtime rebuild, one logical scanout/pointer/RandR/policy publication
barrier, current policy settlement, and exact first presentation. The logical
barrier abstracts sequential cross-process settlement while input is
quarantined; it does not assert simultaneous IPC visibility. No-output and one
bounded rebuild failure remain input-quarantined and can recover after a later
valid rescan. A logical output carries a live head count, so losing one
connector of a mirror group republishes the full epoch exactly as losing an
output does; what consumers hold is checked against the head set that is
actually live. Scenario correspondence and exclusions are recorded in
`validation/specula/output-topology-lifecycle-modeling-brief.md`.
The bounded configuration explores 5,109,131 generated and 784,338 distinct
states to depth 32. A temporary negative control let a head loss leave the
observed epoch alone; TLC violated `PublishedHeadsAreCurrent`.

`PolicyRefreshLifecycle.tla` isolates the revision-1 refresh contract added for
native Hagia policy. It admits only strictly increasing private generations,
coalesces idle dirty output scopes, retains a newer dirty generation that
arrives during an in-flight relayout, requires active-output switches to cover
both old and new outputs, and promotes reducer state plus visible layout as one
settlement. The bounded configuration explores 175 generated states and 71
distinct states to depth 8. A temporary candidate that promoted the active
output without the corresponding layout immediately violated
`ActiveOutputSettlesAtomically`.

`ShellWorkAreaCoordination.tla` is a target pre-schema model for future native
shell reservations. It binds a ready shell candidate and reservation to the
exact derived work-area request, WM connection epoch, and answering projection
before one logical presentation. Stale, torn, or unready attempts are rejected,
and normal shell or WM loss preserves the previous complete bundle. The model
does not promise progress while either optional desktop component is absent and
does not correspond to a production shell runtime. Its bounded configuration
explores 189,816 generated states and 12,278 distinct states to depth 23.

`ShellDescriptorLifecycle.tla` fixes the first experimental shell transport's
descriptor-switcher lifecycle. It separates retained pixels from live target
authority, binds preparation and capture to exact shell, broker, revocation,
and snapshot generations, burns ambiguous activations on disconnect, and
revokes a saturated recipient epoch rather than replaying ordered input. Its
bounded configuration explores 23,582,243 generated states and 241,549
distinct states to depth 20.

`ShellContentLifecycle.tla` adds what a descriptor shell never needed: a content
shell uploads pixels, so accepted resident bytes must be bounded and composition
must be coherent. It models transfer admission reserving staging and resident
credit together, disjoint staging/resident/retiring classes, generation
freshness separated from reference validity, pins that survive retirement until
storage drains, revocation leaving retained pixels inert, and the terminal
result a consumed pacing permit owes. Bytes are abstracted to one unit per
generation, so it checks conservation rather than the byte arithmetic in the
design record. It does not model the liveness half of release, which needs
renderer-progress fairness, nor the reservation and WM half of a coherent
bundle, which is `ShellWorkAreaCoordination`'s. Its bounded configuration
explores 227,724 generated states and 47,979 distinct states to depth 30.

Two controls weaken exactly one rule each, and both encode a defect found while
reviewing the design record rather than an invented failure.
`ShellContentLifecycleRetireIgnoresAssembly.cfg` lets a retire proceed against a
candidate that is still assembling and must violate
`Invariant CandidateReferenceValidity`.
`ShellContentLifecycleSilentAssemblyTimeout.cfg` lets an assembly deadline
adjust bookkeeping without emitting a terminal result, stranding a peer that may
demand its next frame only after an outcome, and must violate
`Invariant NoAcceptedObligationLost`. `tools/check_tla.sh` verifies both exact
failures.

`ShellGpuLaunchAdmission.tla` models the startup-scoped render-device grant for
an admitted content shell. It checks that operator policy, an explicit request,
device availability, connection epoch, and exact device generation remain one
authority decision; device loss or policy withdrawal cannot leave an ambient
grant, and every replacement process receives a fresh grant epoch. Its negative
controls retain a grant across device loss and reuse an epoch so the two
security properties remain executable rather than prose-only.

`ShellContentBundleComposition.tla` models the seam between the two shell
models rather than merging them. The work-area model carries `candidateReady`
as an opaque boolean and proves a presented bundle was ready, coherent and
generation-exact; the content model proves what makes a candidate's pixels
legitimate. Between them sits an assumption neither states, that readiness
stays true. It does not: readiness latches when content, reservation and the WM
answer agree, and content may be retired or revoked afterwards. The model checks
that presentation revalidates content liveness at commit, which is the only way
a bundle cannot be spliced from live geometry and dead pixels. Its bounded
configuration explores 883 generated states and 161 distinct states.

`ShellContentBundleCompositionLatchedReadiness.cfg` trusts the latch instead of
revalidating, and must violate `Invariant PresentedBundleHasLiveContent`.
`tools/check_tla.sh` verifies that exact failure.

`ShellPresentedContentAction.tla` owns the discrete-input seam after a complete
content candidate reaches native presentation. Prepared and presented target
identities are separate, and capture records the complete shell, output,
allocation, candidate, presentation, interaction and target identity. A new
presentation, interaction revocation or shell replacement invalidates capture;
the later physical release is consumed but cannot emit a stale shell action or
fall through to an application. Content-action acknowledgement settles when the
shell reply is reduced, independently of the later authorized WM activation,
and an `Accepted` activation outcome is possible only after WM admission.

The checked configuration explores 3,115 generated states and 1,476 distinct
states to depth 23. Six retained controls make each boundary executable:

- capturing the prepared candidate violates
  `CapturesNameExactPresentedContent`;
- routing a release after interaction revocation violates
  `RevokedReleaseIsSuppressed`;
- routing a release after presentation or shell replacement violates
  `ReplacedReleaseIsSuppressed`;
- allowing a suppressed release to reach an application violates
  `NoClickThrough`;
- delaying shell acknowledgement until a WM outcome violates
  `AckIndependentFromWmOutcome`; and
- publishing `Accepted` before WM admission violates
  `AcceptedOnlyAfterWmAdmission`.

The model is a bounded safety model. It does not model coordinates, native
renderer objects, transport bytes, queue capacities, action deadlines or WM
policy semantics after admission. Production-owner tests retain responsibility
for those implementation boundaries and for distinguishing queued, routed and
client-observed outcomes.

`PixelSilentAdmission.tla` distinguishes presentation intent from complete
pixels. A first timeout without a safe extent preserves the standing target,
owner loop, and one bounded retry. Later pixels may complete admission;
persistent silence withdraws it without killing the owner. The bounded
configuration explores 14 generated states and 11 distinct states to depth 5,
including both liveness properties.

`TargetResolvedInput.tla` models output-local presented interaction snapshots,
non-reusable target identities, deterministic ownership and occlusion,
per-seat/device/contact capture, modal membership, independently issued local
coordinate grants, and lifecycle cancellation. `TargetInputPacing.tla` isolates
the replaceable continuous slot from ordered begin, discrete, completion,
cancellation, endpoint-revocation, and security barriers.

`TargetInputPacing.tla` also models device acquisition, which is the producer
the compositor cannot decline to have happen: the packet already exists when
capacity is examined, so a full queue is a choice of disposition rather than an
absence of work. Three choices are modelled -- bounded deferral, which declines
to read without dropping anything and has a ceiling; scoped endpoint closure,
which drops the arrival but spends reserved capacity on the terminating
boundary; and terminal failure. Closure is per seat and names its tokens
separately from security revocation, because only revocation is entitled to
leave a stream with no boundary. The bounded configuration explores 1,969,760
generated states and 567,966 distinct states to depth 21.

What the reserve buys is now stated rather than implied. Two slots per active
seat is exactly what escalation spends, so the admission bound is what makes
escalation always possible instead of a decoration on a guard.

Seven temporary negative controls independently remove `Drain` fairness, the
deferral ceiling, the discard accounting, the boundary flush at closure, the
saturation report, the reserve in the acquisition predicate, and in-order
draining. TLC then violates `QueueEventuallyEmpties`, `DeferralIsBounded`,
`AcquisitionIsConserved`, `EndpointCloseIsScoped`, `SaturationIsRecorded`,
`BoundaryCapacityIsReserved`, and `DeliveryPreservesOrder` respectively. The
fairness control is the one that matters most here: the four new actions all sit
outside fairness, and removing the assumption on `Drain` still fails the
property, so admitting a deferral disposition did not quietly convert progress
into a tautology.

An earlier draft checked `EscalationCanAlwaysFlush` and constrained
`deferralTicks` in both `TypeOK` and `DeferralIsBounded`. Neither survived a
control: the first is a strict consequence of `BoundaryCapacityIsReserved` and
no edit can break one without the other, and the second meant `TypeOK` failed
first so the ceiling was never the invariant under test. `TypeOK` now states the
type and `DeferralIsBounded` alone states the bound.

`FrameServiceArbitration.tla` isolates how frame service arbitrates the three
owners of one output: a native pending frame, a queued GPU present, and a
waiting software present. It models the reducer's emit gate and the handler's
own precondition as separate conjuncts, because the defect it was written for
lived in the gap between them. The reducer admitted a present that the handler
then silently refused, while withholding the only effect that would have
drained what the handler was refusing over; neither owner could advance, and
nothing re-deferred the present, so the two waited on each other permanently.
The bounded configuration explores 177 generated states and 96 distinct states
to depth 14.

`EmittedEffectsAreExecutable` is the invariant that keeps the two halves from
drifting apart again: it states each handler precondition as a consequence of
the reducer gate that emits it. A handler guard that merely repeats its gate is
then provably unreachable, which is what makes a silent deferral impossible
rather than merely unlikely. Staging is the case where the mismatch is fatal
instead of silent, so `ServiceNeverCrashes` covers it separately.

Six temporary negative controls independently restore the old primary
suppression, drop the global-idle requirement from the staging gate, drop the
pending-frame requirement from the presentation gate, remove staging from
fairness, and let a present reach flight without occupying the kernel. TLC then
violates `PresentSettles`, `EmittedEffectsAreExecutable` (and
`ServiceNeverCrashes` once the stronger invariant is unlisted),
`EmittedEffectsAreExecutable`, `SoftwareSettles`, and `OneSubmissionInFlight`
respectively. The first is the production deadlock exactly: TLC reaches
`pendingFrame = TRUE` with `presentState = "queued"` and no action remains
enabled. Environment actions stay outside fairness and both producers are
budgeted, so liveness here is a question about arbitration rather than about
outrunning an unbounded producer.

`PageFlipCompletionPump.tla` isolates the KMS completion boundary that follows
frame arbitration. One action reads every ready event on a DRM card into one
bounded ledger cell per physical head; retirement prefers that exact event but
may use the affine submission's signaled `OUT_FENCE_PTR` sync file when the
event is absent. The first fence retirement makes fences authoritative for
that head, so a late event is diagnostic evidence and cannot retire a
successor. The watchdog runs only after a card pump and every currently ready
proof has been retired.

The model found an implementation race in its first run: a callback could
become ready after the ordinary pump but before the hard-stall check. Production
now performs a second card-scoped collect-and-retire pass only for a head that
has crossed the deadline, avoiding a second read on normal service cycles. The
checked two-head, two-generation configuration explores 38,410 generated states
and 9,846 distinct states to depth 24. It checks one in-flight generation per
head, exact at-most-once head-scoped retirement, no successor before its
predecessor, no watchdog while a proof is collectable, and quarantine of late
events after an out-fence fallback.

`MirrorHeadPacing.tla` states the successor rule for a mirror group's heads,
and is written before the code that will implement it. `VisualRetirement.tla`
records today's rule -- a logical output retires when every one of its heads has
flipped -- which paces a group at its slowest screen. That is wrong on ordinary
hardware: 144Hz beside 60Hz is a normal desk, and holding the fast panel back
throttles the client's frame callbacks with it. Here each head instead takes the
newest generation it has not shown, skipping what it missed, and the primary
head alone owns present feedback. Screens may disagree for a while; the bounded
configuration explores 745 generated states and 302 distinct states to depth 12.

Independent pacing is what makes the release rule load-bearing.  Under joint
retirement, "no screen is reading this generation" followed from the group
having moved on together; it no longer does, so `NoScannedGenerationIsReleased`
is stated over every head separately and the newest generation is never freed --
a head that has not caught up will take it next.  `PrimarySubmitNeverBlocked` is
provable from the submit guard as written, in the same way and for the same
reason as `EmittedEffectsAreExecutable`: it exists so that a later conjunct
about another head's progress cannot be added quietly.

Five temporary negative controls independently restore joint advance, judge
release by the primary head alone, let a head take work it has already shown,
make feedback wait for every screen, and drop the secondary head from fairness.
TLC then violates `PrimarySubmitNeverBlocked`, `NoScannedGenerationIsReleased`,
`HeadsNeverShowOlderWork`, `FeedbackIsItsOwnScreen`, and `AllHeadsConverge`
respectively. The first is today's implementation: restoring the rule the code
currently follows is what the fast head being blocked looks like in the model.

`InputAuthorityArbitration.tla` models application route leases and shell
capture. Committed, submitted, and presented choices remain separate. The
original leased target has its own eligible-presentation evidence, independent
of the application currently under the pointer. Both automatic and explicit
origins are represented; automatic provisional input remains eligible, while
explicit input requires activation. A scene revision with valid target and
scope evidence leaves routing available. Shell ownership, exact confirmation,
release acknowledgement, authority/control barriers, and stale queued-input
fencing remain intact. The t066 configuration explores 1,635,555 generated
states and 432,648 distinct states to depth 20. Restoring exact scene equality
in a temporary copy violates `EligibleLeaseRemainsRoutable`.

`PointerGrabAdmission.tla` covers the complementary map/Prepare race. Published
observations have noncontiguous transaction ids. Dequeue and effect accounting
are separate, and a request may name a later no-Engine-work observation after
its map. Preparation waits for the applied prefix, but activation permits a
client to draw before its first presentation. Output binding, held events,
absolute expiry, and cancellation are explicit. The bounded configuration
explores 184,589 generated states and 44,213 distinct states to depth 25.

Two retained negative configurations remove the prerequisite or revive a
cancelled reservation on late response. They violate `AppliedBeforeGrant` and
`CancelledHasNoOwnership`, respectively. The runner checks those exact failures;
a parser error or an unrelated invariant is not successful negative evidence.
The [scenario coverage and boundary map](pointer-grab/brief-coverage.md) record
the model's abstractions and limits. These checks validate the intended
contract, not an execution-trace refinement of the Rust implementation.

The first connection run found that `(client, connection epoch)` was not a
unique transfer identity when one connection reused it for later work. The
model now includes the transaction in that identity and rejects transaction
reuse within an epoch. The first projection run found that a client could name
a future scene generation and become accidentally current after a later scene
change. A proposal must now answer an outstanding, server-issued request for
the current generation. These are protocol requirements, not model-only
restrictions.

`SharedWorkerService.tla` states what one renderer worker owes several outputs
of a DRM device group, and is written before the code that will implement it.
Each physical head owns its own worker thread today, so a result could be
identified by position: the next message on the channel answered the only
request outstanding, and a request id that did not match was a fault. Sharing
the thread destroys that, and the three things the row asks for are exactly
what position used to provide for free. A result must name its output; an
output must not occupy the worker twice while its slots are still leased; and
neither output may be passed over indefinitely because the other keeps drawing.
The bounded configuration explores 5,095 generated states and 2,135 distinct
states to depth 16.

Slot incarnations and stale releases stay in `VisualRetirementSlots.tla`, which
already owns them and which this change does not alter; the pools simply become
per output within one worker rather than per worker. No timing property appears
here either. Skew is stated structurally, as the number of times an output that
already has a submitted request may be passed over -- one per sibling, which is
what taking the queue in order gives -- rather than as any bound in
milliseconds.

Two of the invariants earned their place by failing first. `ServiceSkewBounded`
originally compared service counts directly, and TLC refuted it in fifteen
steps with a run where one output simply had nothing to draw: an idle head
falls arbitrarily behind, and calling that skew fires the invariant on healthy
behaviour. It now counts pass-overs against an output that is actually waiting.
The environment was wrong in the opposite direction: `Compose` refused to offer
a generation while that output had a render in flight, which is not what the
code does -- the pending cell is filled without consulting the worker, and that
is the whole reason supersession exists. With the environment corrected the
state space doubled, and the submission gate's negative control became able to
fail at all.

Three temporary negative controls independently route every result to one fixed
output, drop the per-output in-flight gate from submission, and let the worker
take any queued entry instead of the head. TLC then violates
`ResponsesRouteToTheirOutput` at depth 5 (with `OneInFlightPerOutput` unlisted,
since misdelivery breaks both in the same state), `OneInFlightPerOutput` at
depth 5, and `ServiceSkewBounded` at depth 11 respectively. The third is the
one worth keeping in mind while implementing: nothing about a shared worker
forces fair service, and a scheduler that picked its next render by scanning
outputs in a fixed order would starve the second screen while satisfying every
other invariant here.

The policy models intentionally omit wire offsets, KDL schema machinery, tags,
Hagia `ViewId` values, rendering, and physical output retirement. Golden-vector
tests own byte layouts. Hagia owns tags and views. The existing visual models
own preparation, submission, and page-flip retirement after a logical
projection is accepted.

Temporary negative controls removed candidate readiness, reservation-basis
equality, and WM-epoch equality separately. TLC then violated
`PresentedBundlesWereReady`, `PresentedBundlesAreCoherent`, and
`PresentedBundlesMatchExactGenerationAndEpoch`, respectively. The clean model
restores all three checks; no unsafe-mode switch remains in the retained spec.

`PresentFlipOwnership.tla` states what a direct-scanout Present owes the
client whose buffer it puts on the plane, and is written before the code that
will implement it. `PresentCopyOwnership` owns the composited path, where a
compositor snapshot reaches glass and the client source goes idle at the flip;
this module owns its complement, where nothing is copied and the client's own
buffer is displayed. The bounded configuration explores 329 generated states
and 182 distinct states to depth 12.

Three things follow from displaying the client's buffer rather than a copy of
it. The buffer cannot be released at submission or at the flip, because the
screen is scanning it; release waits for a successor's retirement. That
successor may be a composed frame, since an overlay or effect activating makes
the next frame ineligible while the direct frame is still on glass -- the
return to mixed composition is a successor, not an eviction. And a flip is
lawful only under a proof and an atomic test that are fresh in the current
eligibility episode, because an activation ends the episode and everything
stamped before it describes a different scene.

The test stamp was unfalsifiable in the first draft. Engine proves every frame
while the backend tests only on the eligibility edge, so a frame can carry a
fresh proof over a stale test -- and without an action expressing that
asymmetry, the proof stamp blocked every path the test stamp's control would
have used, and the control could not fail. `ReProveAfterEpisodeChange` is that
action, and the model is more faithful for it: the two stamps are separate in
the code for exactly this reason.

The `~effectActive` conjunct on `DirectFlip` is provably unreachable and kept
anyway, in the same spirit as `EmittedEffectsAreExecutable`: activation
advances the episode and no action stamps a proof or test while an effect is
live, so a live effect always implies a stale stamp. Its control cannot fail
today, and that is recorded here rather than resolved by deleting a conjunct
whose absence would let a later decoupling of episode from activation make a
flip during an effect lawful.

Four temporary negative controls independently release the buffer at the flip,
release it before it is ever displayed, drop the test stamp from the flip
guard, and settle a fallback as `Flip`. TLC then violates
`DisplayedClientBufferIsNeverReleased` at depth 4, `ReleasedOnlyBySuccessor` at
depth 3, `EveryFlipWasEligible` at depth 7, and `FlipFeedbackRequiresRealFlip`
at depth 4 respectively.

## Rust Boundary Map

The model is deliberately smaller than the implementation. Its actions map to
the current owning Rust boundaries as follows:

| Model action | Current Rust boundary |
| --- | --- |
| `Propose`, `Prepare` | `HeadlessEngine::prepare_surface_transactions` and `ProductionSessionCoordinator::prepare_present_transaction` |
| `Submit` | `OutputPresentationRegistry::schedule` followed by the production presentation adapter's `submit_frame` effect |
| `Retire` | `OutputPresentationRegistry::retire` and `ProductionSessionCoordinator::settle_prepared_retirement` |
| `Settle(..., "rejected")` | `TransactionOutcome::{RejectedStaleSurface, RejectedInvalidSurface}` reducers |
| `Settle(..., "timed_out")` | authority and WM timeout reducers returning `TransactionOutcome::TimedOut` |
| `Settle(..., "disconnected")` | `AuthorityTransactionInbox::drain_ready` reports disconnect; universal visual settlement remains a gap |
| `Settle(..., "removed")` | ordered `AuthorityTransactionIntake::removed_surfaces` handling, currently on the direct-commit path |
| `LoseHead(...)/"head_lost"` | physical output loss while a head owns a native presentation lease |
| `Release` | output retirement plus backend-specific lease teardown; a universal release reducer remains deferred |
| `QueueNext` | `SurfaceContentStream::admit` carrying a complete `LiveProductionAuthorityGroup` |
| `RetirePresent` | `finish_surface_content_owner` after exact DMA or software frame retirement |
| `ApplyReady` | the next production cycle's sequential rebase, CPU update, and authority commit |
| `BeginMove`, `BeginResize` | `LiveWmLayoutManager::stage` deriving full geometry controls separately from pixel resize obligations |
| `DeliverControl` | `XAuthorityRuntime::configure_window_from_engine` followed by Present ConfigureNotify and core ConfigureNotify for a real change |
| `CommitMove`, `CommitResize` | logical layout commit, with only the resize path gated by exact candidate retirement |
| `Timeout` | layout recovery queuing the complete last-committed rectangle behind any late target control |
| `Compose`, `Supersede` | `LiveProductionNativeScanoutExporter::replace_pending_frame` and its three content entry points |
| `Submit` | `export_from_worker` taking the pending cell once `WorkerPoll::Idle`, then `NativeGbmRendererWorker::submit` |
| `BeginRender`, `FinishRender` | `run_worker`'s command loop and its `WorkerResult` reply for one `WorkerCommand::Render` |
| `Collect` | `NativeGbmRendererWorker::poll` matching a result against the requesting output's in-flight record |
| `reply` per output | the per-output bounded reply channel this row introduces; today one `Receiver` per per-head worker |
| `passedOver` | service order of the shared command queue; no Rust counterpart until the shared worker exists |
| `ProveEligibility`, `ReProveAfterEpisodeChange` | the per-frame direct-scanout verdict computed in `build_head_composition_plan` and carried on `HeadCompositionPlan` |
| `TestPass`, `TestRefuse` | a `TEST_ONLY` atomic commit of the exact client framebuffer, issued on the composition-to-direct edge |
| `DirectFlip` | the real atomic commit of a client-owned framebuffer, acquiring no renderer frame slot |
| `CommitRefused` | the fallback ladder re-queueing the same content through mixed composition |
| `SuccessorDirectRetires`, `SuccessorComposedRetires` | page-flip retirement of the following frame, which is what destroys the displayed bundle |
| `CompleteFlip` | the `Flip` Present disposition, reserved today and unreachable from the copy path |
| `effectActive`, `episode` | an overlay or resolved effect in the frame candidate; no Rust counterpart until the provider registry lands |
| `BeginFrontendGrab`, `ConfirmFrontendGrab`, `RejectFrontendGrab` | provisional `ApplicationRouteLeaseState`, `XAuthorityRoutedInput::route_lease`, and sanitized `XAuthorityRouteLeaseUpdate` feedback |
| `RequestLeaseRelease`, `AcknowledgeLeaseRelease` | exact Engine release state plus `XAuthorityRouteLeaseRelease` and frontend grab teardown acknowledgement |
| `SecurityTransition`, stale queued route rejection | shared input control epoch, `advance_security_epoch`, and epoch-stamped bounded frontend ingress |
| `BeginLayout`, `FirstSilentTimeout` | admission staging and `LiveWmLayoutManager::expire_pending` retaining a pixel-silent retry |
| `RestartAndReseed`, `WithdrawSilentAdmission` | live-session WM recovery and bounded admission retry accounting |
| `ObservePixels`, `CommitPixels` | Engine safe-pixel observation and ordinary exact-candidate layout settlement |

The public policy model is intentionally ahead of production implementation.
Its actions map to the following target boundaries:

| Model action | Target Rust boundary |
| --- | --- |
| `Connect`, `Negotiate`, `Disconnect` | session-owned role endpoint and policy transport worker |
| `BeginProposal`, `AppendProposalChunk`, `FinishProposal` | generated frame codec plus bounded transfer assembler |
| `SettleQueued` | connection-epoch intake before Engine proposal validation |
| `BeginProposal` in the projection model | immutable canonical projection candidate construction |
| `CommitProposal`, `RejectProposal`, `TimeoutProposal` | Engine-owned projection reducer and explicit transaction outcome |
| `SceneChange` | Engine surface/output lifecycle reduction and fresh complete snapshot generation |
| `EnqueueAction`, `IssueAction` | Engine shortcut router and bounded session-owned policy request queue |
| `RequestDirty`, `IssueDirty` | policy transport requesting a fresh complete Engine cycle without geometry |
| `UpdateInteraction`, `IssueInteraction` | Engine-owned grab coalescing reduced continuous geometry |
| `InstallConfig` | policy configuration generation activated at a shortcut-idle boundary |
| `FrontendSettlesChanged`, `CommitProposal` | frontend settlement retry and final canonical Engine commit |
| `IssueCrossOutputRequest`, `StageCompleteProposal` | complete affected-output request and cloned canonical reducer successor |
| `ObserveRightOutputLoss`, `ObserveRightOutputReturn` | Engine output lifecycle observation, scene advancement, and non-recyclable output generation |
| `PrepareCurrentCandidate`, `SettlePreparedCandidate` | non-mutating staged revalidation followed by one reducer/layout settlement |
| `RejectSupersededCandidate` | stale topology terminal outcome preserving the prior complete projection |
| `AdmitDirty`, `IssueRefresh`, `StageProposal`, `SettleProposal` in `PolicyRefreshLifecycle.tla` | `PolicyDirty` admission/coalescing, complete refresh issuance, and atomic staged reducer/layout settlement in the public owner |

The descriptor-shell model maps to the experimental implementation as follows:

| Model action | Rust boundary |
| --- | --- |
| `Connect`, `Disconnect`, `Saturate` | `ShellSessionTransport` protected admission, epoch reset, and bounded activation queue |
| `PublishSnapshot`, `SubmitCandidate` | strict shell snapshot/candidate codecs and `request_candidate` exact-snapshot validation |
| `PrepareCandidate`, `PresentCandidate`, `RejectCandidate` | transport-enforced candidate outcome order and the Engine descriptor projection reducer |
| `BeginCapture`, `ReleaseCapture` | last-presented chrome capture and exact opaque activation construction |
| `Acknowledge` | ordered exact-transaction activation acknowledgement |
| `RevokeBroker` | broker action grant and issuer/revocation generation validation |

This map fixes ownership before the Rust names exist. Update the right column
when implementation establishes the final module names; do not move an action
to another authority merely to match convenient code placement.

The frame-ownership model maps `QueuePresent` to
`queue_software_present_frame` and `stage_software_present_frame`,
`SubmitPresent` to `mark_software_present_frame_submitted`, and
`ObservePresentRetirement` plus `SettlePresent` to
`settle_software_present_frame`. The typed
`LiveProductionNativeFrameId` is the identity shared by those transitions.
`SubmitUnrelated` and `RetireUnrelated` represent ordinary CPU or DMA frames
whose callbacks must leave the software binding unchanged. `SubmitSuccessor`
represents callback-and-submit coalescing within one backend tick.

The copy-ownership model maps `Capture` to
`NativeGbmRenderedScanoutContext::capture_renderer_image`, `PageFlip` to the
exact mixed-frame retirement that promotes the image and retires the client
presentation, `CompleteCopy` to X Present Copy feedback, and `Rollback` to
renderer-image teardown on export, submission, or authority failure.

This table identifies correspondence, not equivalence. The current direct
authority commit and committed-snapshot replacement APIs remain implementation
gaps recorded in `docs/research-log.md`; the model must not be weakened to make
those shortcuts appear retirement-safe.

When TLC finds a counterexample that changes implementation behavior, preserve
the trace as a deterministic Rust regression before correcting the model or
code. Do not replay external effects from a TLC trace.

## Reproducible Command

Sophia pins the command-line TLA+ tools release rather than the unmaintained
graphical Toolbox. Version 1.7.4 includes the stable liveness-checking fix
called out by its release notes. Obtain its official `tla2tools.jar`, then run:

```sh
SOPHIA_TLA2TOOLS_JAR=/absolute/path/to/tla2tools.jar tools/check_tla.sh
```

The checker requires Java 11 or newer and verifies the pinned SHA-256 before
running TLC with one worker in a temporary directory. It performs no network
access and leaves no model-checker state in the repository. The pinned artifact
is:

- release: <https://github.com/tlaplus/tlaplus/releases/tag/v1.7.4>
- jar: <https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar>
- SHA-256: `936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`

TLC's deadlock error is disabled because a fully settled session with no newer
generation is valid quiescence. Safety invariants and the explicit liveness
property remain enabled.

The optional commit-pinned Specula development audit is documented under
`validation/specula`. It generates review evidence outside this repository and
adds no runtime or build dependency.

Complementary bounded relational and arithmetic models live under
`validation/architecture`. Alloy owns static role/namespace/portal and
presented-target topology; Z3 owns region and wire-bound arithmetic. They do
not replace or translate the TLA+ models: presentation epochs, route-lease
revocation, capture cancellation, pacing, and fairness remain temporal here.
The cross-model relationship is a documented correspondence, not a refinement
chain.

`AdmissionRecovery.tla` covers exact PresentedBuffer selection, a later
backing observation, proactive safe-extent admission, timeout only when no
complete pixels have been observed, one outstanding Manage request, committed
public planning-ownership consumption, fallback admission,
temporary-constraint release, one relayout, and exact standing-target
retirement. It explores target observation both before and after fallback
retirement. Its actions map to `LayoutEpochCoordinator`, the bounded
visual-candidate tracker, public Manage ownership, pre-admission group
ownership, the production Present scheduler, and native retirement. The model
nondeterministically gives the selected fallback Present DMA or CPU storage and
requires the same retirement lifecycle for either choice. Content identity
remains distinct from geometry and storage: an extent or materialization can
choose a rendering path without choosing which candidate owns admission. The
bounded configuration explores 172 generated states and 88 distinct states to
depth 13.

## InputDeliveryRecovery

`t077` models two X connections, two input obligations, connection binding
following grab resolution, a blocked writer, exact terminal receipt observation,
release-barrier control gating and VT revocation. Its six-second absolute
input deadline includes the existing five-second subscription grace. A separate
half-second VT grace revokes all still-pending old-seat input before handoff.

`InputDeliveryRecovery.cfg` checks safety plus delivery, healthy-control and seat
progress with weak fairness of owner service and time. It assumes socket shutdown
returns independently of the output mutex; the Unix-socket regression checks
that concrete mechanism. No fairness of client reads or writer completion is
assumed. `InputDeliveryRecoveryNoDeadline.cfg` must fail delivery liveness;
`InputDeliveryRecoveryEarlyBarrier.cfg` must violate `BarrierSound`.
The brief and source mapping are in `input-delivery-recovery/`. This is bounded
model checking, not a physical session or native-client trace acceptance.

`ShellContentOutbox.tla` models the owned content response boundary for two
outputs in one grant. `Accept` reserves producer credits; native completion makes
the exact Presented response eligible but does not send it. `Transfer` corresponds
to `prepare_content_frame`, producer validation, FIFO insertion and the subsequent
infallible producer pop. Allocation/refusal before insertion is a stutter; peer
loss settles the remaining inventory as lost, never delivered. The implementation
has no fallible operation between successful FIFO insertion and producer removal.

The model keeps complete frame charges through partial writes, includes bulk in
the aggregate record and byte budget, and reserves both Action and Cancel before
admission. A held resource consumer does not gate an otherwise admissible action.
It abstracts fixed response envelopes to two units and one bulk frame to three;
its five-record/twelve-unit bounds explore contention, not the negotiated numeric
profile. Exact codec sizes and negotiated limits are Rust regression obligations.
The model does not simulate GPU execution, toolkit state, complete reference
counts, a reconnecting grant, or native latency; those remain separate evidence.
No fairness or driver-progress guarantee is inferred from its safety checks.

Five registered negative controls require their precise invariant failure:
`DuplicateTransfer` -> `OneCustodian`, `EarlyCredit` ->
`ChargedUntilLastByte`, `ActionOvertakes` -> `PresentedBeforeAction`,
`BulkStealsCredit` -> `AggregateBudget`, and `ActionWaitsForRelease` ->
`ReleaseDoesNotGateAction`. The last is an enabled-transition check, not a claim
that a peer or GPU will make progress. `ShellPresentedContentAction` now models
WM activation request separately from ACK receipt, matching the independently
authorized production paths; its existing six controls remain required.
