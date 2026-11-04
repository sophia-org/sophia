# Native FrameCommit: remove rendezvous from the hot path

Status: design model and default-off Rust credit foundation; no production wire
change or protocol allocation.

The maintained [architecture proposal](../../../docs/native-frame-transactions.md)
and [zk decision](../../../docs/notes/decisions/dwm37bn6-submit-native-frames-as-bounded-transactions-over-the-existing-shell-transport.md)
connect this model to the native CPU plan. The proposal explicitly retains 9P
for non-pixel control and the owned-ring/image path for CPU pixels.

The fresh changing-strip comparison measured combined client+server CPU:
Sophia **1032.5–1075**, Niri **415–455**, XLibre **180–220 µs/frame**.
Accounting bounds, not confidence intervals. These are specific matched lanes,
not rankings of all uses of Smithay or XLibre. XLibre ran without a separate
compositor. Both external servers opened an additional GPU, while their actual
rendering/output was qualified on the same iGPU. Source evidence:
`~/.local/state/sophia-igpu-reference-result-01`, seal `2b1d8658…`.

The current Sophia owner still runs about **12.5 passes/frame**, mostly on FD
readiness. The 1 ms timers do not explain most of those passes. Per-step CPU costs
are not measured; multiplying message counts by an invented syscall price would
not give a valid budget.

```text
SETUP / CHANGES (existing authentication; new bounded stream contract)
  producer <--> Session
    grant + output/facts/allocation/ring identities
    validated scene binding + finite credits + reserved close response
    facts/authority change closes admission; old physical work drains

STEADY FRAME
  PRODUCER                         SESSION                         DISPLAY
  fill owned-ring slot
  FrameCommit -------------------> fresh authority + whole-input reservation
    slot/version                   credit + worker + responses
    complete scene intent                      |
    predecessor intent             one private copy + pixel validation
                                               |
  <--- source released             read ticket finished
       (reuse allowed)                         |
                                   revalidate binding; atomic scene handoff
                                   Prepared; prepare all required head plans
                                               |
                                   existing GL cache / damage / render
                                               +-----------------> submit
                                                                  fence/flip
                                   exact native completion <------+
  <--- Presented + returned credit  terminal transfer
                                   commit predecessor retirement
  <--- ResourceReleased            last relevant lease gone; publish release

  Event-directed bounded service; batch already-ready work and responses.
  Keep real deadline/descriptor-less fallbacks until coverage is proved.
```

This removes the producer's image-settlement rendezvous before candidate intent,
the per-frame demand/permit exchange, and the explicit predecessor-retire request.
It does **not** remove source-copy validation, current authority, exact release
identity, resource limits or native fences. There may be several response writes;
source reuse must not wait unnecessarily for presentation.

## Performance budget and kill criterion

Set an engineering budget of **350 µs combined CPU/frame** for this changing-strip
lane: initially 280 Session + 70 producer. This is a target, not an estimate from
the profile and not a promised result. It requires roughly two thirds less CPU
than today's mean. The measured profile does not yet establish that the proposed
mechanism can achieve it.

Use one outstanding frame first, with the same two source slots, pixel work and
output cadence. Do not buy the result by accumulating latency or changing the
security contract. Qualify against a fresh matched Niri block: candidate's upper
accounting bound below Niri's lower bound, same delivered frames/cadence and an
explicit latency acceptance. The 350 budget leaves headroom below the currently
observed Niri interval; XLibre's 180–220 is a separate stretch target.

## Smallest implementation path after the design gates

1. Finish the wire-admission/refusal and bounded resource-lifetime proof seams.
   Reuse W3's ledger and W2's retirement rules; do not add a second credit engine.
2. Add one opt-in FrameCommit grammar and adapter behind negotiation; keep the
   existing generic path. Bind all identity fields, reserve before input, join
   capture with scene intent internally, and preserve cancellation/error behavior.
3. Wire the real Lom producer as well as the reference peer. Keep C/Rust protocol
   parity and deterministic failure tests. Record runtime transitions for model
   conformance; do not instrument production timing runs.
4. Test adversarial cancellation/revoke/backpressure and retained multilayer scenes
   device-free. Count actual eliminated exchanges and owner passes outside timing.
5. Run one matched native comparison. If the structural counter moves but combined
   CPU does not, stop this candidate and use the updated profile; no chain of tiny
   ABBA experiments to defend the design.

No benchmark-only authorization shortcut, new transport stack, Vulkan rewrite,
timer deletion or general-purpose scene framework is part of this proposal.

The protocol is named **Sophia Frame Transactions (SFT)**; `FrameCommit` is its
per-frame submission. See the [canonical contract](../../../docs/native-frame-transactions.md).

The runtime-only `experimental-native-frame-transaction` feature now compiles
the existing W3 ledger, imported from `33ff35990` with its original tests.
Two further tests cover all eight ledger key fields and the external drain gate
before reopening, including rejection of old completions and candidate replay.
No FrameCommit codec or opener exists yet. The ledger does not check ring/slot
identity or implement physical resource ownership; those remain adapter duties.

The subsequent default-off store adapter reuses W2 retirement and W3 candidate
lifecycle code, with credit return moved to successful terminal output transfer.
Its typed input seam reserves a nonfatal refusal before consuming a Begin;
continuations use the accepted reservation. This is a store boundary only:
connection-wide byte/record capacity, wire decoding/trailing discard and physical
copy admission are still separate integration gates. There is no wire opener or
runtime trace-conformance claim.

## Model checks

See [modeling brief](modeling-brief.md) for requirements, assumptions and exclusions,
and [coverage](brief-coverage.md) for the configured checks. Reproduce with the
repository's pinned development TLC jar:

```sh
SOPHIA_SPECULA_TLA2TOOLS_JAR=/absolute/path/to/tla2tools.jar \
  tools/check_native_frame_transaction_model.sh
```

TLC artifacts and counterexamples stay outside the repository. A model result is
design evidence; implementation trace validation and native performance remain
separate gates.
