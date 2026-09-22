---
id: 945mtp8i
date: 2026-09-22
kind: investigation
status: investigating
tags: [investigation, x11, xtest, containment]
---
# A real XTEST client that closes right after a zero-delay FakeInput ends the frontend service

## Question

Under `--admit-xtest`, one `xdotool mousedown 1` against the bare root of the
production frontend ended the whole service. Which client behaviour does it,
which serving loops are exposed, and why does the XTEST conformance profile --
which covers injector disconnects -- stay green?

## Evidence

Found under t124 on `839d011a`, trying to drive a real xterm with an XTEST
drag headless. Fixture: `x11_conformance_host /tmp/.X11-unix/X88 --admit-xtest`
on a private display, operator display `:77` untouched. Three reproductions,
then three isolating runs; scripts and host logs retained under
`.artifacts/t124-xtest-service-exit/`.

| run | client traffic | host |
| --- | --- | --- |
| 1 | xterm + `xdotool type --window` + XTEST drag | exited |
| 2 | xterm (text via `-e`) + XTEST drag only | exited |
| A | xterm alone, then `SIGTERM` | **alive** |
| B | six `xdotool getmouselocation` connect/disconnect cycles | **alive** |
| C | one `xdotool mousemove; mousedown 1; mouseup 1` on the root | **exited** |

Every exit is the same line, and it is the whole of the host's output:

```
Error: X11SetupSocketError { message: "X11 dispatch ended before its effects
were published", client_disconnect: false, client_failure: false,
service_shutdown: false }
```

Ordinary disconnects (A, B) and `selection_probe`'s own connect/query/close,
which ran in every run, do not do it. XTEST `FakeInput` followed by the
client's immediate exit does. That is exactly how every `xdotool` invocation
behaves: zero delay, no round trip, close.

## Finding and resolution

**Established.** The worker's dispatch loop resets `dispatch_started` and
`dispatch_complete` per request (`x11_socket/connection/dispatch.rs:1069-1070`)
and sets `dispatch_complete = true` only at the end of the body (`:2631`). A
departure noticed while the connection is *waiting* ends cleanly
(`ConnectionWake::Departed => return Ok(())`, `:1008`); a departure noticed
*after dispatch started* reaches `:2935-2937`, which builds the error above with
`X11SetupSocketError::new` -- so all three classification flags are false. The
comment there states the intent: "Partial dispatch is fatal, never an empty
success that would certify missing authority effects."

That intent is right for what it protects and wrong for where it lands. The
reaper, `poll_client_workers` (`x11_socket/frontend/service.rs:509-523`),
contains a worker error only when `client_failure || client_disconnect ||
service_shutdown`; anything else it returns as `Err`. Every serving loop found
propagates that with `?`: the fixture (`examples/x11_conformance_host.rs`), the
**production private input service** (`connection/private_service.rs:211,243`)
and the once-only routed server (`connection/server.rs:249`). So a real
client's mid-request close is not classified as the client's fault and ends
the service that admitted it. The private input service admits XTEST by
design (M5), so it is exposed by code reading; it has not been driven to exit
here. The live session's main display installs the same injection policy under
`--admit-xtest` (`live_session.rs:714-724`); **whether its serving loop
propagates the same way is not confirmed** -- several searches did not locate
that loop, and this note does not guess.

**Why the gate is green.** The XTEST profile's disconnect cases
(`tools/probes/x11_conformance/xtest_cases.py:381-440`) close in a different
phase: `half_close` and `full_delay_disconnect` send a `FakeInput` with a
delay (500 ms, `0xffffffff`) and close during the *wait*, which is the clean
`Departed` arm; `disconnect_release` and the others round-trip `GetInputFocus`
(opcode 43) after the `FakeInput`, so the dispatch has completed before the
socket closes. No case sends a zero-delay `FakeInput` and closes at once.

A zero-delay `FakeInput` is the ordinary case: `delay=0` is xdotool's default
and the reference server's fast path. The gate exercises the two edges and not
the middle.

**What a repair must keep.** The rule that a partial dispatch is never
certified as an empty success stands -- it is what keeps the gate honest. The
gap is classification, not policy: a departure observed mid-dispatch is a
`client_disconnect` and must be reported as one, so the reaper contains it and
the service survives; the observation must still publish that the request did
*not* complete, so no downstream reader mistakes it for success. The proof is a
new mandatory XTEST wire case -- zero-delay `FakeInput`, immediate close, then a
fresh client must still be admitted and answered -- which goes red on the
current tree and green on the repair.

## Validation and remaining work

Open as t154 in [todo.md](../../../todo.md). Not yet repaired. This blocks
[t124](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md):
its remaining question needs exactly the client behaviour that kills the host.
Also worth noting, and not this task's: `xdotool type --window` uses
`SendEvent` (opcode 25), which the frontend decodes (`wire.rs:1454`) and
refused here with `BadValue 0x2`; without `--window` it uses XTEST.

## Connections

- [Primary selection does not reach another client from xterm](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md) --
  owns t124, blocked on this.
- [Preflight setup disconnect precedes an authority exit](kwhei4x4-preflight-setup-disconnect-precedes-an-authority-exit.md) --
  t089, the setup-phase half of the same containment class.
- [A stalled protocol recipient can escape X11 client containment](psf52z1x-a-stalled-protocol-recipient-can-escape-x11-client-containment.md) --
  t090, the recipient half. Neither names a mid-request close.
- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  M5, which made the private service admit XTEST.
