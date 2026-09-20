---
id: 3pq7vn2e
date: 2026-09-20
kind: investigation
status: resolved
tags: [investigation, x11]
---
# An impervious client's own GrabServer is dropped rather than deferred

## Question

XTEST's GrabControl marks a client impervious to another client's server
grab, so that a harness can drive a server the client under test has
grabbed. What should happen when the impervious client asks for the server
grab itself, while somebody else holds it?

## Evidence

Found by the M5 `grab_control` group, which proves the consequence at the
wire: with a holder in place, an impervious client's GrabServer and its
following UngrabServer change nothing, the paused ordinary client stays
paused throughout both, and it is answered only when the real holder
releases.

The dispatcher discards the result: `dispatch/core/grabs.rs` answers
GrabServer with `let _ = runtime.input_authority_mut().grab_server(...)`,
and `grab_server` returns `AlreadyGrabbed` when another client owns it. So
the request is a silent no-op.

## Finding

**Answering nothing is right; dropping the request is not.** GrabServer
defines no error in the core protocol, so there is nothing to send and a
client that asked cannot be told it failed. What the reference does instead
is defer: the request is reset and the client stopped until the grab is
free, and it then takes it.

This instance already defers correctly, and did so for every client until
this week. The connection loop's pause parks a client before dispatch while
another holds the grab, re-reads the owner on every wake, and lets the
request through when it is free -- which is deferral, reached by a different
route. Wiring XTEST's imperviousness exempted impervious clients from that
pause, which is what imperviousness means for their other requests, and
carried their GrabServer past it too. Past the pause there is nothing that
defers, only the discard.

So the hole is exactly as wide as the exemption, and no wider: an impervious
client's GrabServer, and only that, is now dropped where it used to wait.

## What a repair looks like

The exemption should not cover a GrabServer request itself. A harness is
entitled to have its other requests processed through someone else's grab;
it is not entitled to have a grab it asked for quietly discarded. The pause
loop already does the waiting, so this is a condition on the bypass rather
than new machinery.

The `grab_control` group's third subcase asserts the current consequence,
and its text -- that an impervious client never takes a grab another client
holds -- still holds under deferral, since waiting for the holder is not
taking. The assertion that a paused client is answered only on the holder's
release is the one that changes, because the impervious client would then be
waiting too.

## Resolution

Repaired 2026-09-20. The exemption is now a condition on the bypass rather
than new machinery, exactly as this note anticipated: the pause is skipped
for an impervious client's requests except its own GrabServer, which goes
through the pause and is therefore deferred the way every client's was before
M5.

The `grab_control` subcase that pinned the consequence is rewritten and now
pins the deferral: the impervious client's GrabServer is not answered while
another client holds the server, it takes the grab when the holder releases,
and an ordinary client is then paused by that grab until the impervious
client ungrabs -- which proves the grab was really taken rather than merely
not lost.

Two shape notes for anyone reading the control. The subcase no longer uses
`while_grabbed`: that helper syncs the owner after its UngrabServer, and a
deferred grab is taken in exactly that gap, so the owner's own round trip
would be paused behind the grab it had just released and the helper would
never return. And the ordinary client's probe is sent only after the
handover, because one sent before it could be answered on either side and
would pin the scheduler rather than the grab.

## Connections

- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  M5, whose imperviousness work opened this.
