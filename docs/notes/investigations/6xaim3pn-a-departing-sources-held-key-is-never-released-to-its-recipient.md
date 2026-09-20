---
id: 6xaim3pn
date: 2026-09-20
kind: investigation
status: investigating
tags: [investigation, x11, input]
---
# A departing source's held key is never released to its recipient

## Question

An XTEST client presses a key, the recipient receives the KeyPress, and the
client disconnects while the key is still down. The reference server releases
it: the recipient is sent a KeyRelease, because a key that nobody is holding
any more is not held. This instance sends nothing, and the recipient's
keyboard stays down for ever. Is the departure not noticed, is the ledger not
releasing, or is the release not delivered?

## Evidence

Measured 2026-09-20 against the private conformance host, XTEST profile,
little-endian, from the `xtest_disconnect_release` and `xtest_two_injectors`
cases, with prints at the connection's exit, at grant revocation, and at the
executor's release path.

The departure is noticed and the ledger does release. `xtest_disconnect_release`:

```text
XDBG key kc=30 pressed=true grant=GrantId { slot: 1, generation: 3 } holds=0
XDBG conn-exit t=29205
XDBG revoke grant=GrantId { slot: 1, generation: 3 }
      debt=RetiredDebt { owed_releases: 1, survivors: 0 }
XDBG conn-exit t=32204          <- the observer, three seconds later, at the
                                   case deadline, having received nothing
```

The connection exits immediately on its peer's close, its admission is
revoked on that same thread, `revoke_grant` retires the source, and the
ledger reports one owed release. Nothing further happens.

`xtest_two_injectors` shows the other face of it. Two sources hold the same
key; the first departs; the second then releases:

```text
XDBG key kc=42 pressed=true grant=slot 1   holds=0
XDBG key kc=42 pressed=true grant=slot 2   holds=1
XDBG join key input=Input { slot: 42 } grant=slot 2
XDBG key kc=42 pressed=false grant=slot 2  holds=1
XDBG release outcome=SurvivorRemains
```

The join is correct and emits nothing, which is right: a second source taking
a key that is already down changes no aggregate. But the second source's
release answers `SurvivorRemains`, because the first source's departure had
not yet been applied to the ledger at that moment, so the aggregate still
believed two sources held the key. The recipient is owed a KeyRelease that
never comes, for the same reason and from the other direction.

## Finding

**The release is computed and then discarded.** `revoke_grant`
(`sophia-input-authority/src/registry.rs`) retires each source the grant
owns, `retire_source` releases every input that source held, and each
release that ends the aggregate is counted into `RetiredDebt.owed_releases`.
The only production caller, `close_and_retire` in
`x11_socket/routing/private_participant.rs`, binds that debt to `_debt` and
drops it. `RetiredDebt` has no consumer anywhere in `sophia-x-authority`,
and `AuthorityInstance::next_debt`, which exists to hand a scheduler the
records whose holders have reached zero, has no production caller either --
only tests.

So the machinery for deciding the release is complete and the machinery for
delivering one is complete, and nothing joins them. A release that a request
asks for goes through `resolve_and_apply_key`, which takes an
`ExecutionPermit`, calls `permit.release`, builds the event from the
keyboard's before and after state, and pushes a `PrivateSettlingRelease` that
the terminal delivers. A release that a departure causes has no request, no
permit, and no route, and there is no path for it.

`reconcile_key` is not that path, though it is the closest thing: it records
the native half of a retired hold -- the key up in XKB, the modifiers
observed, the proof set -- and deliberately builds no client event. It
answers what the keyboard now holds, not what the recipient is owed.

## What a repair has to supply

1. A release the ledger has already applied, consumed rather than re-applied.
   The aggregate went to zero inside `revoke_grant`; calling `permit.release`
   again would be wrong and there is no permit to call it with.
2. The event, built the way `release_key` builds it -- the keyboard's state
   before and after, the pointer's buttons, the query modifiers -- with the
   time taken from the authority rather than from a request that does not
   exist.
3. A `PrivateSettlingRelease` pushed for the hold, so the delivery, the
   receipt and the custody accounting are the ones every other release uses.
4. The terminal visit that drives it, bounded and cursored like the other
   visits, finding its work through `next_debt` or through the revocation
   record rather than by sweeping.

The recipient may itself have departed, which is the case `reconcile_key`
already handles, so the two paths meet and the order between them matters.

## The repair, read out of the source rather than sketched

Every piece this needs already exists, and the design below was taken as far
as reading can take it before any of it was written. What looked like the
obstacle -- a release with no request, no permit, no route and no delivery
identity -- turns out to be three separate questions, and two of them are
already answered in the tree.

**A delivery identity is not needed.** `input_recovery.bind` takes an
`Option<XAuthorityInputDeliveryId>` and answers `Ok(!revoked)` when it is
`None`, so a release with no delivery binds end to end and still says
whether its recipient may be reached. `PrivateOrderedEmission::key(hold,
delivery, payload)` takes the same `Option`. A `StateOnly` release already
carries a custody with no completion cell, so the storage shape exists too.
Nothing has to be invented and nothing has to be minted.

**Delivery is already the terminal's.** A settling release's capsule is
built inside the terminal, from the emission the native release left on the
hold: `private_terminal_native.rs` takes it with `take_release_emission`,
wraps it, carries the finalizer, and hands it to the writer. So a visit that
installs a release emission on a hold and moves that hold into `settling` has
done everything delivery needs; there is no second path to build and no
runner decision to fabricate.

**What is genuinely new is one native method.** `release_key` does the whole
job today but begins with `permit.release`, which the ledger has already done
inside `revoke_grant`; calling it again would release one thing twice. What
is wanted is the rest of that function -- the keyboard's state before and
after, the evdev key up, the observed modifiers, the activation retirement,
the residual, the plan's child and coordinates from the pointer path, and the
emission -- under a `NativeReconciliationPermit` rather than an
`ExecutionPermit`, with the incarnation taken from the permit and the time
from the authority rather than from a request that does not exist. Most of
the body is shared with `release_key` and should be factored rather than
copied.

`reconcile_key` is **not** that method, though it is the closest thing. It
records the native half of a retired hold and deliberately builds no client
event, and it requires the namespace's ordered query scope to be retired,
which is true at namespace teardown and false when one source among several
departs.

**Where the visit goes.** The terminal driver cycles a phase cursor modulo
nine, with the last two reserved for invocation completion and control
cleanup and the rest dispatched by `visit_terminal`. This becomes modulo ten
with one new phase, bounded and cursored like its siblings. It finds its work
by asking the ledger for the next record whose holders have reached zero and
matching that incarnation against a hold that is still in `holds`: a release
a request made has already left `holds` for `settling`, so a held incarnation
with no holders is one the ledger ended without a request, which is exactly
this.

## Two things measured that constrain it

**It must not wait for the departed source's row.** A departed connection's
row stays open until the service stops (t134), so a visit that required the
source to be collected would never run, and t136 and t134 would each be
waiting for the other.

**The recipient may have departed too.** Its row is open as well, so the
delivery comes back `WriteFailed` or `ClientDisconnected` from a socket that
is gone, rather than a clean `TargetGone`. Settling has to treat those as
terminal or the release never finishes and the instance cannot stop.

## Connections

- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  M5's `cancellation_half_close` obligation class, whose last two wire cases
  this is.
- [A full per-client control queue ends the whole private input service](1lv1gg5u-a-full-per-client-control-queue-ends-the-whole-private-input-service.md) --
  the other departure-adjacent defect found the same day, and separate: that
  one is about a queue, this one about a debt.
