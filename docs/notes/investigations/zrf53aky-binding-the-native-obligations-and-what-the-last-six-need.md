---
id: zrf53aky
date: 2026-09-20
kind: investigation
status: investigating
tags: [investigation, validation, input]
---
# Binding the native obligations, and what the last six need

## Question

The native-input conformance profile reported twenty of its forty mandatory
obligations as having no implementation, which the gate reads as NORESULT.
Are they unproven, or merely unbound?

## What was established

**Twelve were merely unbound.** The behaviour was already proved, by tests
written for their own sake, and the manifest simply did not point at them.
They are bound as of `a4d09e34`; the profile now runs 116 test executions
rather than 109 and reports eight NORESULT rather than twenty.

**Two are genuinely unmet, and now say why.** The manifest's loader refuses
to let an obligation be silently optional -- `native obligations may not be
silently optional` -- which is the right rule, so the only honest record is
mandatory, unmet, and a reason beside it.

`native_protected_action` names a protected chord. There is no such concept
anywhere in `crates/`: the protection has not been built, so there is no
behaviour to witness and nothing adjacent may stand in for it.

`native_internal_wait` is the more interesting one. It says internal waits
must never accrue recipient nonresponse, and the meter it names --
`X_AUTHORITY_ORDERED_BLOCKED_LIMIT` in `writers/blocked_send.rs` -- says of
itself that the measurement is "exercised by controls and not yet by a
writer". Nothing accrues to it from the live path at all, so the obligation
holds today because nothing accrues to anything. A test bound now would
record a property the instance does not have, and the gate would go green on
a gap.

## The near miss, which is why every binding was read first

The first candidate for `native_transport_wait` was a watchdog control
proving that a deliberate wait before dequeue consumes no executor deadline.
It is a true test, it is well named, and it is about the wrong meter: the
obligation says *recipient nonresponse*, which is the blocked-send
allowance, not the execution deadline. Binding it would have attached a
passing test to a claim it does not make, and no gate could have seen it --
the test passes, the obligation reports PASS, and the thing the obligation
exists to protect is unproven.

The vocabulary of these obligations overlaps the vocabulary of the tests far
more than the meanings do. Reading before binding is the only defence, and a
wrong binding is worse evidence than MISSING, because MISSING is honest.

## The six that remain, and what each needs

**`native_ingress_admission` is compound and partly witnessed.** It names
eleven things, and six have candidates found:

| clause | candidate |
| --- | --- |
| authorization denial apart from capacity | `a_private_producer_is_told_denial_apart_from_saturation` |
| capacity | the same |
| sequence exhaustion never resetting identities | `a_count_that_cannot_advance_refuses_rather_than_saturating` |
| preserves accepted work | `review_owner_saturation_cannot_discard_two_already_accepted_controls` |
| queue poison is unavailable, not empty | `a_poisoned_owner_reports_unavailable_rather_than_nothing_to_do` |
| duplicate delivery identity | `a_reused_delivery_id_does_not_settle_the_debt_that_had_it_before` (about settling, not admission -- wants checking) |

Unwitnessed so far: unavailable or disconnected authority; every refusal
returning owned work; rolling back only its own reserved credit; consumer
closure rejecting later submissions while settling previously accepted work.
Binding the six and leaving the rest would make the profile report PASS for
an obligation five clauses of which nobody has proved, so it stays unbound
until the set is complete or the gap is written down as its own finding.

**`native_executor_order` is compound in the same way** and was not reached:
it names seven producer kinds sharing one runnable-admission order, plus
completed-send precedence and exact identities at consumption, plus
saturation retaining payloads.

**`native_recipient_removal`, `native_lock_order` and `native_no_fallback`
are single-clause but keyword search does not reach them.** The obligations
are phrased in the plan's vocabulary and the tests in the code's, and the
words that would join them -- "route removal", "lock order", "ambient
backend" -- do not appear in any test name. These want reading by subject:
finding the test file whose subject is the obligation's subject, and reading
it.

**`native_stalled_reader` wants a test written, and XTEST is the instrument.**
"Real private stalled socket reader is contained while healthy peer
continues" is about real sockets, and M5's `processing_barrier` group already
proves the healthy-peer half at the wire against a real private Session. The
stalled half is one more client that stops reading. It cannot be bound to
that group as it stands, because the manifest runs ordinary cargo tests and
the M5 groups carry `#[ignore]`; it needs a non-ignored test of its own.

## Connections

- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  the plan these obligations belong to.
