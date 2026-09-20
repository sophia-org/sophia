---
id: cfko9jxl
date: 2026-09-20
kind: milestone
status: recorded
tags: [milestone, x11, input, tooling]
---
# M6 accepted: the whole of t093 on one source

## Result or change

M6 adds no behaviour. It establishes that everything t093 built holds on
one exact committed source, produced by gates rather than by whoever
remembered, and composed into one verdict that reads NORESULT for anything
absent. The source is `f564b744`, master at the end of 2026-09-20 with t130 closed,
and the composition is `cargo xtask check m6-evidence` on it with both citations
supplied:

| component | verdict | identity |
| --- | --- | --- |
| `m3-acceptance` | PASS | the gate's own report, same source by digest |
| `m4-acceptance` | PASS | the gate's own report, same source by digest |
| `m5-acceptance` | PASS | the gate's own report, same source by digest |
| `x11-profile` | PASS | the gate's own report, same source by digest |
| `native-input` | PASS, 40 of 40 | inside `x11-profile` |
| `xtest` | PASS, 42 of 42, both byte orders | inside `x11-profile` |
| core baseline (t057) | cited PASS, 120 of 120 | `.artifacts/x11-core-f564b744/report.json`, names `f564b744725e` clean |
| canonical workspace check | cited PASS, full check executed | `.artifacts/offline-check-f564b744/report.json`, names `f564b744725e` clean |
| **overall** | **PASS** | run `11074-1789936609423203772`, `.artifacts/m6-evidence-f564b744/report.json` |

Source `f564b744725edaa328c95031086c7a0b129c3baf`, tree `e1483fa7ed9d0c58a6f97be6a177fbf5c27b53d1`,
archive sha256 `51afbdc75402681b909dc8f56b71f737f419982fe8b70bdfea1ef9b99a0b309c`,
content sha256 `e2e27288deea79efd1d83642b91a7397b343c129211d126cf583e8383e57f025`; every component ran on that
content by digest, and the source was unchanged when the run ended.
XTS5 stays BLOCKED and unrun, which is reported and not counted.

Every one of the forty native obligations is bound to a test whose
assertions are the obligation's claim, each with a scope that says what it
does not prove; the XTEST wire profile passes forty-two of forty-two
subcases in both byte orders; M3, M4 and M5 pass again on the same bytes;
the core baseline and the contained canonical workspace check are cited by
report and name this commit on a clean tree.

## Evidence and decisions

**The count, and how it got to forty.** The morning of 2026-09-20 the
profile read eight of forty NORESULT after the adapter lane's twelve
bindings. By the evening all forty read PASS, in this order, each landing
merged and gated (identities as they exist after the day's linearisation of
master, see [njr7sd2q](../investigations/njr7sd2q-binding-the-last-native-obligations-what-each-proves-and-what-stays-unmet.md)):

| obligation | how it closed | commit |
| --- | --- | --- |
| `native_lock_order` | five existing tests read by subject: guarded lock APIs under real contention, two contending writers holding one order, the order's mark above common | `849a9b53` |
| `native_recipient_removal` | two existing tests: the recipient window removed while its client lives, the query lifecycle retired, the targeted debt kept | `849a9b53` |
| `native_stalled_reader` | written: one real service, two real clients, one stops reading; its send blocks, the healthy peer is served inside the allowance, the stalled delivery reads TimedOut and its socket ends | `4c6cdca2` |
| `native_no_fallback` | the refusal half bound to two tests; the ambient half is the M4 group, gate-only, run by this composition on the same source | `2b4dee11` |
| `native_ingress_admission` | five witnesses written for the clauses nobody had, then bound to nineteen tests | `1bfe3659` |
| `native_internal_wait` | written: the output lock held past the whole allowance while a key is delivered, and the delivery still reads Flushed | `ae98d134` |
| `native_executor_order` | reworded by decision to the two producers that exist and bound to six tests | `8c01b8ca` |
| `native_protected_action` | built: the executor refuses the synthetic press that would complete the reserved chord; wire case `xtest_reserved_chord` | `b3d23edb` |

**Three decisions, Mason's, 2026-09-20.** The selected lock rank is the
one the code documents per edge, and the plan's paragraph was corrected to
it. `native_executor_order` names the producers that exist rather than
report PASS on five that never enter the order. t139 closes by refusing the
chord in the executor, where the seat's modifier state is readable, with the
guard's physical-only recognition witnessed by construction and the
constants pinned equal across crates.

**What was observed red before its change.** These are not a mutation
exercise; they are the controls that failed as written until the code they
witness existed, which is the same evidence read from the other side:

- The executor-refusal wedge witness: before the fix, the submission after a
  refused one was saturated for three seconds and then failed the control;
  after it, the refused delivery reads RouteRejected within a second and the
  next submission is accepted.
- The reserved-chord wire case on its first commit: the refusal was stored
  but the waiter never woken, so the adapter slept on it until the client
  departed and the dispatch ended unpublished; the case read FAIL in both
  orders until the wake was raised.
- The stalled-reader test's first shape asked a key of the focused surface
  while the pointer was over the other client's window and was refused by
  the route, which became t140, closed the same evening by the lane.

## Defects this slice repaired

- **t136**, a departed source's held key never released to its recipient:
  four gates behind the one the first build named; the release is owed and
  delivered, xtest forty of forty (`21d3fced`).
- **A refused request wedged its producer.** A request the executor refused
  before entering the authority had no completion, so the grant's one cell
  stayed held and the next submission was saturated for good; one refused
  injection wedged the injector. Refusals made on the request's own terms are
  now answered and free the grant (`b3d23edb`, `719ccb89`).
- **A synthetic source could hand Ctrl-Alt-Backspace to a client** as key
  events, which the engine forbids for a policy binding; refused (t139).
- **Fifty-two stale dead-code markers** on the ordered writer chain said it
  was not attached; thirty-seven removed, fifteen kept with truthful comments
  (`fcfc98ee`).
- **The profile gate demoted a PASS for a child the subreaper had already
  reaped**; it now records collection facts and demotes only what lingered,
  and the three collection predicates in the gate family are documented as
  deliberately different (`86c3b0c5`, `e87a0381`).
- **t140**, a focused key refused for where the pointer was, closed by the
  lane (`98a9bf41`).
- **t130, a full per-client control channel ended the private input
  service.** The canonical citation on `d57254d2` failed on it, one run in
  three, where the earlier PASS on `7fe5b296` was the same coin landing the
  other way. A control a full channel will not take is now kept with its
  credit and sent on a later turn, in order, and the invocation is never
  ended for it; what is deferred is the message and not the operation, so a
  focus change's FocusOut that already went out is never repeated. The
  public broker's arm stays fatal.
- **A reap ahead of a destruction decision changed what the decision
  recorded.** The first composition, on `7fe5b296`, read M3 nineteen of
  twenty: `D.start_failures` failed one run in ten from the t138 commit
  onward and never before it, bisected by running the single control at
  four commits. The idle-window reclaim reaped any started custody whose
  thread had finished, and a permit-refused worker finishes at once, so the
  reap could take the handle before the departure decided and the decision
  then recorded `WorkerHandedOn` for a worker that was running. The reap now
  takes only a departure already decided as deferred, the arm the discharge
  itself requires, and the control reads handle-or-reaped, refusing only an
  empty slot with no join, which is the one thing that would mean startup
  lost the handle (`d57254d2`, the lane's). Not rerolled: a control that
  fails one in ten is not evidence until it is fixed.

## Limits of this result

- Headless. Nothing here is physical acceptance; every physical source is a
  fixture with physical origin, and the guard's recognition of the emergency
  chord from real devices is witnessed by construction and by constants, not
  by a device.
- The scopes are part of the evidence. `native_no_fallback`'s ambient half
  lives in the M4 group and is complete only in this composition;
  `native_internal_wait` does not separately time queue, delay or frozen
  waits; `native_lock_order` does not execute the whole rank as an audit;
  `native_ingress_admission` does not drive the order's own sequence counter
  to its end; `native_executor_order` is the producers that exist.
- The reserved chord is refused only to synthetic sources and only for the
  press that completes it; a physical press never enters the private
  executor and is not touched by it.
- XTS5 is BLOCKED and unrun, as it has been throughout: no XTS checkout on
  this host.
- Discovery stays disabled. M6 does not enable it, does not authorise
  installation or default enablement, and does not substitute for physical
  acceptance under t094, t077, t060 or t062. Closing t093 is a separate
  decision.

## Remaining work

- Closing t093 itself, with discovery disabled and t094 acknowledged open.
- t138 (the lane's), the continuation place returned during the run; the
  evidence-custody layer recorded rather than built.
- t131, t132, t115's twelve-run bar, now that t130 is closed.

## Connections

- [Binding the last native obligations, what each proves, and what stays unmet](../investigations/njr7sd2q-binding-the-last-native-obligations-what-each-proves-and-what-stays-unmet.md) --
  the day's readings, the decisions, and the identities.
- [M5 XTEST adapter accepted and the one obligation left open](vlcrn30a-m5-xtest-adapter-accepted-and-the-one-obligation-left-open.md) --
  the predecessor and the obligation it left, closed as t136.
- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  the plan, and the M6 contract this fulfils.
- [A departing source's held key is never released to its recipient](../investigations/6xaim3pn-a-departing-sources-held-key-is-never-released-to-its-recipient.md) --
  t136.
