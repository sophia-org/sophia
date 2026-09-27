---
id: bzitjwp8
date: 2026-09-27
kind: milestone
status: recorded
tags: [milestone, 9p, sdk]
---
# C WM SDK codec and session checkpoint

## Implemented slice

The C SDK has a WM file codec and bounded session alongside its shell modules.
It remains an independent repository. This is progress toward
[t263](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md), not
completion of WM product migration or the complete SDK role surface.

The signed C SDK commits are `32f9c4a53e79602311fdeffa796a78e88c09940a`
(codec) and `841563d614ed8540472f0edfa7f4cddaafe3fdde` (session). The pinned
WM contracts come from signed Sophia `de776c68afdf9a133818f86917893c3362dc9fb7`.
Bemenu retains its separate `a0ab8c8` SDK pin.

## Evidence and decisions

The first C99 compile with `-Wall -Wextra -Werror -pedantic`, `WITH_IPC=0`,
nice 19 and two jobs passed without diagnostics. The seventeen session test
groups passed on their first run, along with the nine file/transport test
executables and all twenty specification digests. Execution used a private
build directory and bwrap without devices, network or display access.

Four mutants failed at their intended checks: remove the preceding Submitted
ACK barrier; classify a submit reply before that drain's events; remove EAGAIN
pacing; remove snapshot identity binding. Each mutation used a private copy;
the baseline archive was unchanged. A mutation runner initially expected a
case name missing from a shared assertion helper. That diagnostic failure was
kept; named-case diagnostics were added and all mutants were then verified.

Evidence is under `~/.local/state/sophia/development-evidence/` in
`c-wm-sdk-codec/` and `c-wm-sdk-session/`. The session test links the real generic
9P pipeline, C codecs and session, against a scripted peer using the C encoder.
Literal codec vectors and twenty-two golden row layouts are separate checks.
These results do not establish operation against the production WM export,
independent peer encoding, policy acceptance or native presentation.

## Remaining work

The production-export harness is being added in Sophia with a generic C peer.
Its initial scenario covers profile preparation/activation, configuration,
Dirty, a snapshot larger than msize, projection, session operation and a supplied
presentation receipt. Admission and policy outcomes remain fixture decisions.
At this checkpoint the harness is code-only, with no run result.

Hagia's vendored SDK binding and policy regression gate remain open. Output and
admin file contracts and SDK modules are separate gaps. The SDK's
`compatibility.json` therefore still sets `wm_files=false`. The linked t263
plan and [role migration plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
own the remaining exits; the running desktop was not changed by this work.
