---
id: l4ruwq60
date: 2026-11-08
kind: investigation
status: investigating
tags: [investigation, performance, custody, native, tla]
---
# Worker-side native submission needs a new authority and lifetime contract

## Question

Can T4 remove the successful render-result → owner → DRM-submit handoff
without changing fresh authority, completion identity or physical custody?
The outbound Render request remains. Niltempus authorized progress until a
critical scope decision or physical run; this checkpoint reaches the former.

## Finding

Moving the ioctl alone is insufficient. Current submission prepares resources,
checks capacity and installs custody synchronously on the owner. A worker that
also submits would need:

- Submission authority serialized with revocation, replacement and competing
  submissions. The model's worker-side gate is new shared authorization state,
  not an existing scheduling primitive or permission to cache authority.
- Exact submitted/uncertain ownership independent of reply delivery, output
  facade and worker lifetime. Today's GBM/EGL ownership is worker-local; a
  dropped reply or joined worker does not prove a submitted buffer is reusable.
- Completion coverage before submission can finish, including a flip before
  the result is consumed. Existing out-fences are produced by the commit and
  do not supply this coverage in advance.

The [contract and model](../../../validation/specula/worker-native-submit/README.md)
map these obligations to source at `2c77581d8`. This is proposed-design
analysis, not an identified bug in current Sophia.

## Checks and limits

Corrected model checkpoint `1195d002b`: complete safety search, 889 generated /
364 distinct states, seven invariants pass under the proposed assumptions.
Eight unsafe variants fail their expected invariants. A separate temporal probe
of the protected design fails when a worker stalls holding submission authority:
a withdrawal request cannot become effective while that gate remains held.

The result does not prove current owner-side ioctls have stronger stall progress.
It does not qualify a Rust implementation or a CPU saving. One frame is modeled;
submit N+1 racing retirement N, real memory ordering and physical DRM behavior
remain outside the proof. Unknown ioctl outcome is retained until known failure
or established physical disposition, never released using the model's kernel
truth as owner knowledge.

Evidence: `~/.local/state/sophia-t4-model-02/`, SHA256SUMS
`beb6721faf571240787df7c7b9db5f9cfbb1d9026e9fd0836196c6a924307132`.
Model-01 and its held review stay frozen; model-02 corrects the unknown-outcome
boundary and records the amended scope. The reproduction helper accepts the
expected progress counterexample and explicitly does not qualify implementation.

Independent review accepts this as design input only:
`~/.local/state/sophia-t4-model-review-claude-02/`, SHA256SUMS
`31b9e91f1c2b3a7f9fcd257f665e19947ab42a949fd164226abd62f6304cfa94`.
Its two clarifications are carried into the model README: the started record
must precede the syscall, and exact completion need not unblock withdrawal in
this gated variant. The stronger trace is a reason to question the gate design,
not proof that every worker-submission design must fail.

## Recommendation and decision needed

Recommend parking worker-side submission at this checkpoint. Expanding it means
designing a bounded submission owner with a new authorization and progress
contract, not completing a small rendering optimization. That expansion is
unselected and requires niltempus's scope decision.

If the wake-removal investigation continues, the alternative raised by review is
owner-side submission with an explicit GPU input fence. It is a hypothesis, not
a selected implementation or known smaller complete solution. A bounded source
study must first answer buffer/framebuffer availability, when the fence can be
exported without another worker return trip, required driver support, and how
render errors and validation results that currently precede submission are
handled. No wake reduction or CPU saving is established for that alternative.

No native run is requested. M1/M2 remain deferred, B6 remains before guarded
actions, M3 stays parked, and T1/T2/T3/T5 remain unselected.

## Connections

The [four-wait dependency note](edc0te2v-which-action-each-of-the-four-lean-owner-wakes-owes-and-which-transition-a-consolidation-would-remove.md)
defines the T4 handoff and keeps source associations apart from observations.
The [CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
and `todo.md` t344 own selection and task state; the
[SFT contract](../../native-frame-transactions.md) still requires fresh submit
authority and exact retirement.
