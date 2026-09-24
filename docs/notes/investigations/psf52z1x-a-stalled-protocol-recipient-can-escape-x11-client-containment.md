---
id: psf52z1x
date: 2026-09-12
kind: investigation
status: resolved
tags: [investigation, x11, containment, conformance]
---
# A stalled protocol recipient can escape X11 client containment

## Source finding

At 3f4b0462, two older peer-delivery paths in
`crates/sophia-x-authority/src/x11_socket/connection/dispatch.rs` convert every
`route_protocol` error into an unclassified `X11SetupSocketError::new`:
peer Present NotifyMSC delivery and DestroyNotify delivery during client release.
A recipient queue failure can therefore escape the client worker through the
same service-reaping boundary implicated by t089. The sender or departing owner
is not necessarily the failed recipient.

This is a source finding, not a reproduced queue-pressure incident. The ordinary
destroy and peer-close cases pass in the 94-execution independent gate; they do
not fill a recipient queue. No live display was contacted to investigate this.

## Finding and resolution (2026-09-24, t090)

Since t165 a connection's output is never waited for: every write is a
non-blocking send into the kernel or the connection's spill, and the spill's
two bounds (16 MiB owed, or six seconds owed with nothing read nor asked)
end a client that will not take its output. What remained of this gap was
the bounded hand-off between a sender's thread and the recipient's writer:
`route_protocol` answers `ClientQueueFull` when that queue is full, and
every peer delivery but the XFixes watcher path turned that answer into an
unclassified error on the sender's own thread -- a lifecycle notice
(DestroyNotify among them) failed the client whose request caused it, the
departed owner's destroys failed its own teardown, a peer Present NotifyMSC
failed the client that asked for the clock, and the registry's Present
completion route stopped at the first full subscriber, so the subscribers
behind it were never told and the session logged the loss.

The registry now has one act for it, `route_protocol_contained`
(`routing/registry/delivery.rs`): a recipient whose queue is full is ended
exactly, through `input_recovery.disconnect_exact` with the identity the
route captured (never whoever holds the number next), so its own thread runs
its own teardown and it reads EOF like any departed peer; a recipient that
has already gone is skipped; poisoned shared state and every other refusal
remain the caller's error. The XFixes watcher path ends through the same act
and then removes its row. Every peer delivery of the socket layer goes
through it -- the lifecycle, property and selection passes, the MappingNotify
broadcast, the peer NotifyMSC deliveries, the departed client's destroys, the
retained-range destroys and the save-set reparents -- and so do the Present
completion, idle and MSC routes, which now tell every subscriber behind an
ended one. The private control path keeps its receipts and settles them
itself, as before.

Proof, `x11_socket/tests/stalled_recipients.rs`: a one-slot protocol queue
and a recipient that never reads. A Present completion fanned out to a
stalled subscriber that sorts first and a healthy one behind it returns
routed, the healthy one is told, the stalled one reads EOF holding only what
it had, its connection is open no longer while the healthy one's is, and a
newcomer registers; the same for a DestroyNotify fanned out by the lifecycle
pass, where the sender, which did not select, is handed no copy. Each is its
own negative control: without containment the routed call answers the queue
error and the assertion on it fails. `tests/x11_wire/client_lifetime.rs`: a
watcher that departed before the window's owner does not fail the owner's
teardown, and the window is gone for a newcomer. The core conformance probe
gains `destroy_notify_stalled`, the lifecycle-path twin of
`xfixes_selection_stalled`: a silent watcher is ended past the allowance
while the owner keeps destroying and the laggard and the healthy watcher
receive every notice.

## Required repair and proof (as filed)

Task t090 owns this separate routing-containment gap. Reproduce it using bounded
private fixtures, then require the affected recipient to be disconnected on
queue exhaustion while healthy senders, recipients and new admissions continue.
An already departed recipient must not make its sender or the service fail.
Do not silently drop mandatory events for a client that remains connected.
Keep poisoned shared state and other authority failures fatal.

Cover both named delivery paths and retain a negative control that makes the
fixture fail when recipient containment is removed. Check that teardown still
retires subscriptions and resources even when a recipient has gone. The current
94-case baseline is insufficient to close this task.

## Connections

- [todo.md](../../../todo.md): t090 is in the highest-priority protocol tranche.
- [Setup-disconnect incident](kwhei4x4-preflight-setup-disconnect-precedes-an-authority-exit.md):
  t089 covers setup/request failure classification and installed acceptance;
  it does not certify every asynchronous recipient path.
- [Independent conformance evidence](wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md):
  ordinary destroy delivery passes; queue-pressure coverage remains separate.
- [t063](../plans/queue-11-parallel-production-readiness.md#t063): the new XFixes
  path must disconnect a stalled watcher rather than silently dropping its
  events. That work stays with the XFixes repair, not this older-path task.
