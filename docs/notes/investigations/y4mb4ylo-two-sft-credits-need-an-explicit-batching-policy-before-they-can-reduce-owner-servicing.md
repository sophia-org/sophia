---
id: y4mb4ylo
date: 2026-11-09
kind: investigation
status: resolved
tags: [investigation, performance, custody, scheduling]
---
# Two SFT credits need an explicit batching policy before they can reduce owner servicing

## Question

Niltempus selected T5: evaluate two bounded in-flight SFT frames, retaining the
renderer, validation worker and submission owner. First identify ready work
that could share servicing at the same 60 Hz. Reject queueing without eliminated
servicing. Model credits, immutable capture, revocation, failure, Presented order
and predecessor retirement before implementation. A surviving implementation
needs device-free proof and native CPU/presented-frame, latency, cadence and
rendered/presented-count qualification.

This note is the scheduling screen, not the two-frame custody model. Source is
Sophia `be77902c3938e2966fb84311ea1af4a0189584a9`, with the lane's peer
`273d98c2efe9999ce082dc4200e20c67a471f2d2`. No runtime change is selected.

## Evidence

The disposable recount at `~/.local/state/sophia-t5-scheduling-screen-01/`
checks svc-02's sealed result manifest and the raw client-journal hashes against
its sealed package file list. It uses only timestamps from the peer's own
monotonic clock, generations 121–2120 inclusive (2000 per arm). It does not
subtract kernel completion timestamps from peer stamps or price any CPU work.

| Peer observation, microseconds | svc-02 control | svc-02 candidate (post-B3) |
|---|---:|---:|
| Next fill minus previous Presented receipt: min / median / max | 5 / 11 / 102 | 5 / 11 / 41 |
| Next offer stamp minus previous Presented receipt: min / median / max | 28 / 45 / 137 | 27 / 45 / 93 |
| Presented receipt minus same offer stamp: min / median / max | 16476 / 16625 / 16768 | 16492 / 16623 / 16744 |
| Fills beginning before previous Presented receipt | 0 | 0 |

These are peer observation intervals, not scanout latency, input-to-photon
latency or comparative performance effects. The offer is logged after the SDK
submission call. Source, rather than that log stamp alone, establishes the
producer dependency. Both arms have the same pinned peer.

In the [post-lean decomposition](q2m7v8ka-what-the-post-lean-decomposition-says-about-the-next-native-frame-change.md),
every whole row records exactly one wait and falls in one of the four reported
cause classes; this does not prove no concurrent work or enumerate all ready
sources. In particular, ring|service contains two bits for one Service-path wait.
Those instrumented costs are not an untraced baseline for T5. The accepted svc-02
cadence already meets 60 Hz with one credit; no second credit is needed to meet
that captured workload's cadence. This is not a guarantee for other workloads.

### Current source constraints

Paths are relative to Sophia unless explicitly marked peer.

| Source | What a second credit does not change |
|---|---|
| Peer `tools/sft-bar/src/main.rs:239–253`; `sft.rs:344–380` | `frame()` must finish before the next iteration. It waits for acceptance, Presented, predecessor release, source release and credit. No independent per-frame timer drives this branch. The final sleep is the post-run hold. |
| Peer `sft.rs:137–156`; `main.rs:84–89` | Each frame retires the preceding resource and alternates two resource IDs with increasing generations. Source ring slots and accepted resource lifetimes are different obligations. |
| `crates/sophia-runtime/src/frame_credits.rs:106–128` | The ledger supports 1–2 credits, but this is not end-to-end support for two native frames. |
| `.../shell_content/candidates/credits/control.rs:12–35`; `.../shell_transport/frame_events.rs:21–36` | Native Open selects one credit; the wire response translator refuses an Opened event with a different count. SDK/protocol negotiation must change with any profile change. |
| `.../shell_content/candidates/credits.rs:268–280`; `credits/commit.rs:106–134` | There is one assembly per output, selected by output and generation. A second Begin during assembly is stale; ready commit is not a queue of two immutable scenes. |
| `.../shell_content/resources/frame.rs:33–63,81–102`; `resources/retire_on_present.rs:93–123` | A new captured resource remains private to its frame until exact Presented transfer. N+1 cannot simply name N's still-private image as its retire predecessor. An earlier credit is not permission to bypass this refusal. |
| `.../shell_transport/owned_copy/frame.rs:24–26,69–105` | This call proposes one queued commit and one reserved copy, then flushes events. Two buffered Twrites do not establish two admissions in a single owner visit. |
| `crates/sophia-backend-live/src/production_visual_runtime/native.rs:601–651` | The current pending-output path refuses an in-flight or cleanup-pending output before starting another tick. Validation ahead of presentation is not permission to run a second render/submit through this path. |
| `crates/sophia-session/src/live_session/owner_loop/authority_receive.rs:42–85`; `authority_service_wait.rs:48–72` | Both waits subscribe component descriptors. An early next commit can cause its own wake; it is not automatically held until native completion. Control, authority and deadline obligations must remain serviced. |

The table lists current constraints, not immutable restrictions on a future
protocol. The selected goal permits changing the protocol, but requires a
concrete scheduling benefit before implementing those changes.

## Finding and resolution

### Three distinct proposals

1. **Credit count only, producer unchanged:** no early next frame. The unused
   credit cannot expose another operation to batch. Reject as an optimization.
2. **Eager one-frame lookahead:** change the producer and stores so N+1 can be
   captured while N is awaiting presentation. This exposes possible overlap
   between validation of a later frame and rendering/submission of an earlier
   frame. It does not guarantee their notifications arrive together or eliminate
   a visit. Neither the source nor these saved one-credit runs demonstrates a
   removed service. Do not build or launch an eager queue on that premise alone.
3. **Deliberate pair batching:** collect/admit two ready commits in one bounded
   owner service, and potentially settle two completed validations together.
   This names real work to amortize, but it is a new batching policy, not merely
   two credits. The owner intake and assembly paths need changes; validation
   notifications would still be separate unless a specific safe batch-completion
   rule is defined. Rendering and physical presentation remain ordered, and no
   reduction of their visits follows. This is a candidate for further design,
   not a selected implementation or demonstrated saving.

For this specific paired-intake/paired-settlement proposal, the named ceiling is
two service entries saved per pair: two wire-intake services become one, and two
validation settlements become one only if both results are available when the
owner services them, or a separately justified batch-completion rule ensures it.
The proposal retains separate render-result and flip services for the two
ordered frames. Relative to the four-class sequence, that is at most two of
eight named services per pair. It is not a 25% CPU prediction, a bound on all
possible T5 designs, or proof that total owner passes fall by two: other ready
work, control traffic, retries and deadlines still require service.

For the third proposal, an illustrative steady-state schedule makes the
freshness trade-off visible. Let P = 1/60 second and omit small service offsets:

```text
Serial, current feedback-paced producer:
  prepare A at 0 -------- display A at P
                          prepare B at P -------- display B at 2P
  ages at display: A ~P, B ~P

Hypothetical pair prepared together at 0:
  prepare A+B at 0 ------ display A at P -------- display B at 2P
  ages at display: A ~P, B ~2P
```

At unchanged cadence, this particular pair schedule adds P (~16.7 ms) to B's
age and P/2 (~8.3 ms) to the pair's mean age. These are arithmetic under explicit
assumptions, not measured latency or universal bounds for all two-credit
schedulers. If B's actual update is not available early, waiting to form the
pair instead delays A. Future input cannot be invented by granting a credit.
Just-in-time/phase-aligned admission is a different possibility, but no such
schedule, measured timing margin or safe coalescing rule has been established
here. Do not infer that two credits necessarily add one frame of latency.

**Decision, November 9:** niltempus selected "preserving responsiveness" after
the reviewed scheduling screen. Retain current frame freshness; do not pursue
the deliberate pair-batching proposal that trades additional frame age for an
unpriced service reduction. Credit-only remains rejected as unused, and eager
queueing has no established eliminated service to justify implementation.

This closes the selected T5 evaluation with a documented no-change result at
the scheduling gate. It does not prove every two-frame architecture ineffective
or necessarily slower. Reopen only with a concrete schedule that names the
repeated service it removes while preserving responsiveness, including control
and revocation responsiveness. No custody model, runtime change or native package
is selected; the broader CPU optimization task remains open.

## Validation and remaining work

The recount refuses missing, duplicate and reversed peer events in small negative
controls, checks complete 2120-frame coverage and the ordering assertions before
reducing the 2000-frame spans. Source files are pinned in the evidence. No Rust
tests, build, TLC run, GPU access or native run was needed for this screen.

For any future reopening, first name precisely the service entry it
removes and how control/revocation/expiry still interrupt delayed work. Then model
two accepted frames plus the preceding displayed resource: credit/response/copy
capacity, private images, exact epochs, failures and withdrawal at every stage,
ordered Presented, and predecessor-retirement commit only at exact terminal
transfer. Reusing a two-entry credit ledger does not prove those invariants.
Model the full batch and its partial failures before source implementation.

Only a surviving mechanism goes to device-free refusal/interleaving proof and
then an owner-launched, reviewed package measuring CPU per presented frame,
latency/frame age, cadence and rendered/presented counts together. No saved
one-credit trace can qualify that design. M1/M2 stay deferred, B6 retained and
worker-side T4 parked.

## Connections

The [four-wait dependency map](edc0te2v-which-action-each-of-the-four-lean-owner-wakes-owes-and-which-transition-a-consolidation-would-remove.md)
named T5 as a hypothesis, not a pass-count prediction. The [SFT contract](../../native-frame-transactions.md)
defines the existing one-outstanding-frame profile and distinct lifetimes. The
[current CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md#current-resumption-november-9)
and `todo.md` t344 own selection and subsequent work.
