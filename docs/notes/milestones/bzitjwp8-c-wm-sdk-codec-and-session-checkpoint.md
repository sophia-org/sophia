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
Those SDK-only results do not establish operation against the production WM
export, independent peer encoding, policy acceptance or native presentation.

## Production-export checkpoint

Sophia `c4e17899eed0aecff19346b1a5e4521be9955f7a` pins the signed SDK session
commit and adds a generic C peer against the production WM file export, reactor,
profile reducer and transport driver. Its first strict compile and run passed:
profile preparation/activation, configuration, Dirty, a 64-surface snapshot
larger than the 4096-byte msize, projection, session operation and a supplied
presentation receipt. The peer links only the SDK's generic 9P, WM codec and
WM session modules. Its encoding is independent of the server's Rust codecs.

The related `policy_transport_worker::ninep` tests passed: 51 passed, three
ignored (two opt-in Nim peer cases and one supervised child entry). Focused
session/xtask clippy with all targets and features, `check c-desktop-sdk`, layout
and formatting passed. The snapshot regression fixture initially failed because
its temporary contract tree omitted the four newly required WM contracts. The
fixture now copies and checks drift for all four; its regression and repeated
focused lint/layout/format checks passed. No SDK or server behavior changed
during this gate. The failed fixture log is retained.

Evidence is in `~/.local/state/sophia/development-evidence/c-wm-sdk-production/`.
The gate used offline/locked Cargo, a private target, nice 19 and two jobs inside
bwrap with a read-only source and no network, hardware devices or display
access. Admission and policy outcomes remain fixture decisions: this is not
supervisor authentication, Engine policy acceptance or native presentation
evidence. It does not close the descriptor-shell independent-peer gaps.

## Remaining work

Hagia's vendored SDK binding and policy regression gate remain open. Output and
admin file contracts and SDK modules are separate gaps. The SDK's
`compatibility.json` therefore still sets `wm_files=false`. The linked t263
plan and [role migration plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
own the remaining exits; the running desktop was not changed by this work.
