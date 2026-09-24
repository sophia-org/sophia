---
id: xd4zhgxm
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, routing, events, probe]
---
# A namespace broadcast skipped a client between its setup reply and its state

## Question

The core conformance probe's `set_pointer_mapping` case timed out twice,
once on a quiet machine, waiting for the MappingNotify its peer client is
owed (`peer.event(34)`); every rerun passed. The x11bench pane reported
both. What could a freshly connected client miss?

## Evidence

`broadcast_protocol_event` (`routing/registry/window_parents.rs`), the
one path a MappingNotify takes to every client of a namespace, chose its
recipients by the connection's *applied state*
(`senders.connection_state.get()`, which names the namespace). That state
is attached in `connection/dispatch.rs` after the setup reply has been
written (line 487 writes it; line 724 attaches), after the route
registration (line 615), the private lifecycle attachment and the XTEST
injector's issuance. A client that has read its setup reply is connected
as far as it and the protocol are concerned, and for that stretch it was
invisible to the broadcast. The probe's peer connects and does nothing;
the main client sends three SetPointerMapping requests at once; whenever
the third was served before the peer's thread reached the attachment,
the peer was skipped and the case waited out its budget. The wire test of
t166-d has the same shape and passed by timing.

## Finding and resolution

The registry now learns a connection's namespace when the connection is
registered (`register_client_in_namespace`, called from the connection
path with the namespace its setup admitted), and the broadcast accepts a
recipient by that namespace or by the attached state. What remains before
the registration is the setup reply's own write and the route
registration a few lines on, with no policy call between them.

Proof, `x11_socket/tests/stalled_recipients.rs`
(`a_broadcast_reaches_a_client_registered_before_its_state_is_attached`):
a client registered into the namespace with no state attached receives
the broadcast; one registered into another namespace does not. Before the
change the filter required the attached state, so the first assertion
could not hold. The core profile is the field check.

## Connections

- [The input map requests were BadRequest](cqg821ne-the-input-map-requests-were-badrequest-and-served-as-far-as-the-authority-can-report-them.md) --
  t166-d, where the broadcast was introduced.
- [A stalled protocol recipient can escape X11 client containment](psf52z1x-a-stalled-protocol-recipient-can-escape-x11-client-containment.md) --
  t090, the contained route the broadcast delivers through.
