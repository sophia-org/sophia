---
id: cfmn7st7
date: 2026-09-20
kind: milestone
status: recorded
tags: [milestone, input, session, validation]
---
# t094 accepted: two keyboards on one seat, from an ordinary session

The operator logged into the installed release with two USB keyboards on
seat0, typed on the Keychron, unplugged it, typed on the Smurve80, plugged
the Keychron back in, typed on it, and logged out. The session's own record
directory is the evidence, read by one command:

```text
$ tools/verify_keyboard_independence_session.sh
keyboard independence accepted from session 00000001789946195390-0caec8f7-a158-495d-93f6-fa5cfb8cc2d3
  profile=hagia release_commit=061c9e6037ed3157b75fe28f348b50ed49dcf4cd binary_sha256=b9f737d3306e82ca3b16b2771cc5e1dab847493e1aa97db88c87825f819c8341
  keyboards typed on: 265 269 270 276 (hardware present before the unplug: 256 257 258 259 260 263 264 265 267 268 269 270 272 273 274)
  unplugged: device 270 released=0 (the kernel releases a USB keyboard's keys itself; the session releases what it did not)
  kept routing: device 265 typed between the unplug and the return
  returned: device 276, a new identity, typed on after its announcement
  records discarded by the recorder: 0
```

The session is `~/.local/state/sophia/sessions/00000001789946195390-0caec8f7-…`,
twenty-five seconds long, on the installed release built from `061c9e60`
(the manifest's `release_commit` and `sophia_binary_sha256` above), exited
`status=exited exit_status=0` with the lifecycle log closing
`status=returned phase=handoff installed=true exit_status=0 emergency=false`
and `storage_errors=0`, `discarded=0`.

**What the record shows.** The seat announced fifteen hardware keyboard
devices at open (each USB keyboard is several kernel devices; the Keychron
K8 Pro is five, 270 through 274). Keys were observed from 270, the Keychron's
main interface, before all five of its devices were removed together. Keys
were then observed from 265 (the Smurve80) while the Keychron was out, and
the seat kept routing them. The Keychron returned as 275 through 279,
identities never announced before, and keys were observed from 276. The seat
ran on no class-identity fallbacks. Every device record carries only an
opaque identity and capability flags; no name or path appears anywhere.

**What the kernel does, and what the session does.** `released=0` on the
unplug is the ordinary reading on Linux: the input core releases every key a
USB device still holds when the device is unregistered, before libinput
reports the removal, so the session's own release-on-departure finds nothing
left to release. That path exists for what the kernel does not release (a
seat reopened around a VT switch, an evdev stream that lost its releases)
and is pinned headlessly in
`crates/sophia-session/src/live_session/tests/device_identity_tests.rs`; the
attended gate's original demand for `released=1` was asking for a record
this hardware cannot produce, and was corrected in `fad8cb4c`.

**What this accepts.** The row's claim: distinct physical-device identities
through backend and Session ingress, independent holds and recovery. The
backend mints an identity per kernel device and announces arrivals and
departures in band (`e3b50379`); the emergency chord and shift coverage are
per device (`0796490b`); a departed device's repeat is cancelled
(`e78b5893`); the session releases a departed device's held keys on its own
turn (`5b31d0cf`); the owner loop reports every device fact and tracks the
releases (`e23b67e8`); the daily recorder keeps those records
(`c56eddf8`); and this session shows the whole path on two real keyboards.

**What this does not claim.** The split emergency chord not arming the
input guard is pinned by `crates/sophia-session/tests/emergency_input.rs`
and was not exercised physically; the optional attended gate exists for it.
No live-seat synthetic-input mode exists; the authority's physical-source
ledger stays fixture-proved and is wired when that mode is built (t144).
Two keyboards on one seat still share one core modifier state by design.
A uinput keyboard is admitted and marked `virtual=true`; the verifier
requires hardware on both sides.

**Method.** Three ordinary sessions were needed, each a few seconds of
typing: the first showed the daily recorder reducing the device records to
their booleans (fixed in `c56eddf8`), the second showed the verifier failing
a correct session under `pipefail` and choosing the wrong returned interface
(fixed in `fad8cb4c`). None needed a console, a guard phase or a proof
phrase. The row closes with this note; the attended gate,
`tools/keyboard_independence_physical_gate.sh`, remains available and
optional.
