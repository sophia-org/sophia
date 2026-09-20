---
id: vlcrn30a
date: 2026-09-20
kind: milestone
status: recorded
tags: [milestone, x11, input, validation]
---
# M5: the XTEST adapter accepted, and the one obligation left open

This records M5 under [t093](../../../todo.md) and its
[plan](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md).

**M5's acceptance gate is met.** All eight obligation groups pass together
on one source: `cargo xtask check m5-acceptance` reads `Pass; 8/8 cases
passed`, every group starting real private Session instances over real
sockets in both byte orders, accounting for every service thread and every
registered ordered worker it started, and printing the record the gate
parses. The eight run together in about two seconds and were run repeatedly
rather than once.

**The independent conformance profile passes thirty-six of forty subcases**
in both byte orders. The four that remain are one obligation rather than
four, and it is stated below rather than left to be inferred from a count.

## What the extension is

XTEST 2.1's four requests, answered against the private Session's own input
authority rather than beside it.

Discovery is one decision. A client that may inject is told XTEST exists,
on an opcode, with both error and event bases zero because the extension
defines neither; a client that may not is not told it exists at all, and
meets `BadAccess` on every request rather than `BadRequest`. Those are
different answers and the difference matters: `BadRequest` would tell a
client that guessed the opcode that the server has no such extension, which
would be false. `ListExtensions` and `QueryExtension` answer from the same
decision, so a client that enumerates and then asks is never told two
things.

`GetVersion` is a constant. The reference never reads the version asked for,
so there is no negotiation to perform and no per-client version state to
keep; the fields are decoded anyway, so a record can say what was asked.

`FakeInput` carries key, button and motion. Its refusals are the reference's,
in the reference's order and naming the reference's values: the type before
the record count and both before detail and root, the unmasked byte reported
where the send-event bit was masked for dispatch, a motion's root consulted
for motion alone, and coordinates clipped rather than refused. Its delay is
taken from the raw bytes before the request is decoded, which is that order
too, so a malformed request carrying one waits and only then answers its
error.

`GrabControl` takes a strict boolean, and its imperviousness is wired: a
client that asked for it is not paused by another client's server grab,
which is the whole reason the request exists, since a harness must be able
to drive a server the client under test has grabbed.

`CompareCursor` answers against the window cursor attribute, which this
server now carries and keeps. None is a real answer rather than missing
information, CurrentCursor is read from where the pointer actually is, and
the window is validated before the cursor because that is the order the
request reads.

## What M5 found in the executor rather than in the adapter

The adapter was the smaller half. Five defects turned up underneath it, each
found by a subcase that could not pass, and each repaired:

**A delay stalled the whole instance.** The delay was taken after the
request had been numbered and given a transaction ticket, and observations
publish in ticket order, so one client sleeping for a second stopped every
other connection on the instance from publishing anything for that second.
The evidence was a healthy third client whose request was read at once,
which acquired the runtime at once, and which then sat for five hundred and
fifty milliseconds inside its own observation waiting for a ticket older
than its own to finish sleeping. The delay now precedes the numbering.

**An instance could not route by its own focus.** The applied routing view
began unpublished and became published only when a focus change was applied,
so a fresh instance refused every pointer route until some client happened
to move the focus, while `GetInputFocus` already reported focus applied on
the root. Preparation now publishes that initial focus when the runtime's
focus is exactly what a fresh publication describes, and leaves a retained
one for the change that next applies it.

**An instance had no pointer.** A key needs the pointer's position, and the
executor takes it only from an observation, which a native source makes when
the pointer first crosses a surface. An instance where that had not happened
had none, so every key was refused; it had been recorded as a retained
diagnostic rather than read as what it was. An instance now starts with an
observation over the bare root at the centre of the screen, made only when
nothing else has observed the pointer, and the key path learned to read one
over the root.

**Motion over the bare root went nowhere.** It names no registered surface,
and the transient path looked its target up and refused. Its recipient is
now found instead, the way the applied view will find it again, and when
nobody selects it the pointer still moves and the query state records it --
a decision of no event rather than a refusal.

**Focus events were delivered to clients that never selected them.** Two per
`SetInputFocus`, on both the routed and unrouted paths. Found from XTEST
because the two-injector subcase is the only one in the profile that asserts
a client's event buffer is empty rather than free of a particular kind.

## The obligation left open

**A departing source's held key is never released to its recipient**, filed
as t136 with its own
[note](../investigations/6xaim3pn-a-departing-sources-held-key-is-never-released-to-its-recipient.md).
The two wire subcases that remain red are both this: a client presses a key,
the recipient receives it, the client disconnects still holding it, and the
recipient's keyboard stays down for ever; and its other face, where a
surviving second source's own release answers `SurvivorRemains` for a holder
that has gone.

It is not a departure-notice problem and was measured not to be: the
connection exits the moment its peer closes, its admission is revoked on
that same thread, and `revoke_grant` retires the source and reports one owed
release. The debt is then bound to an underscore and dropped, and nothing in
the X authority has ever consumed a `RetiredDebt` or called `next_debt`
outside a test. The deciding half is complete, the delivering half is
complete, and nothing joins them.

**`cancellation_half_close` passing does not cover it**, and the two are
easy to confuse. That group proves work still *pending* when a client
departs is cancelled and never happens. t136 is work already *completed*,
whose effect outlives the client that caused it. Eight of eight is not
forty of forty, and this is the difference.

One narrower row is open beside it: **t137**, an impervious client's own
`GrabServer` is dropped rather than deferred, which the imperviousness work
opened and which the `grab_control` group currently pins the consequence of.

## What this does not claim

No real application has driven XTEST here, which is why its matrix row is
`wire` and not higher. The profile's XTS5 rows remain BLOCKED and are
reported as such rather than counted. The conformance profile is still run
by hand rather than by a gate; registering it is separate work, in progress.
M6 is untouched.

## Connections

- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md)
- [A departing source's held key is never released to its recipient](../investigations/6xaim3pn-a-departing-sources-held-key-is-never-released-to-its-recipient.md)
- [An impervious client's own GrabServer is dropped rather than deferred](../investigations/3pq7vn2e-an-impervious-clients-own-grabserver-is-dropped-rather-than-deferred.md)
- [M3 integrated acceptance](nywg1vat-m3-integrated-acceptance-checkpoint-and-remaining-c-controls.md) --
  whose group B this milestone refined, and why.
