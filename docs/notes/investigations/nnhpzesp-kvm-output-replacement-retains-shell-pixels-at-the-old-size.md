---
id: nnhpzesp
date: 2026-10-14
kind: investigation
status: investigating
tags: [investigation, topology, rendering, recovery]
---
# KVM output replacement retains shell pixels at the old size

## Current resumption

On October 14, niltempus switched the KVM to another computer and the installed
session exited. The first retained error is `shell content targets a stale
output size`; cleanup also reported retained surface presentation ownership.
The observer work was uninstalled and is not the running candidate. Repair this
recovery regression before continuing t319 capture work.

Signed fix `1e2c9e50a` withdraws shell frames whose output disappeared or changed
pixel size, both during native resume and during a quiescent topology rebind.
It refuses to discard an unrevoked presentation claim. The deterministic gate
passed and the matched recovery release is installed. niltempus confirmed that
the new live session survived switching the KVM away and back. The recording
shows two outputs becoming one, then two again; both retired native owners
settled without in-flight work or pending cleanup. Normal logout and its final
presentation/resource cleanup remain to be checked before closing this repair.

## Evidence

Installed release: `niltempus-e8818395183f05263ccc`, Sophia
`d5938f2ada2741a9ffc51bf322e3901ecc6f0152`. Session:
`00000001791983659719-c1d52663-c983-40ea-8edf-06758946730d`.
Its binary SHA-256 was
`5c843c0f4081f77f38105a58d12aca060f9c9511b58488f604d3ab18752f83ad`.
The installed `sophia session keep` command preserved the logs under
`~/.local/state/sophia/session-investigations/00000001791983659719-c1d52663-c983-40ea-8edf-06758946730d-0db91d41-225a-4258-8459-378299373178`.

niltempus clarified that crunch has two monitors: one directly connected to the
GPU and one shared through the KVM. Switching the KVM should leave the direct
monitor usable; it is not an all-outputs-absent case. The runtime's one-output
fallback reached readiness before the crash.

UTC sequence from `events.0.log` on October 14:

- 13:38:14.422: input epoch advances; USB input devices begin disappearing.
- 13:38:14.891: native owner 1 is settled with no in-flight or cleanup work.
- 13:38:14.916: native owner 2 resolves one output, with an unavailable-output adjustment.
- 13:38:15.820: the remaining output is ready; its work area is 1920 × 1056.
- 13:38:15.823: shell component connections retire and a new bar connection is admitted.
- 13:38:15.832: the owner loop detects a fatal error.
- 13:38:16.041: native cleanup reports drained, but presentation shutdown fails.
- 13:38:22.497: session outcome records exit status 1 and handoff to the display manager.

## Finding and resolution

The CPU compositor correctly rejects shell images sized for another output.
Native resume installs a new output set without reconciling retained shell
images. The separate topology-rebind path prunes by output number only. A
surviving output number therefore can retain images with obsolete pixel bounds.
The next ordinary repaint reaches the compositor's stale-size error.

Reconcile the images at the runtime's topology boundary. Do not stretch stale
pixels, ignore all compositor errors, or release a source still held by a native
consumer. Keep unchanged neighboring output content. Existing input projection
replacement must prevent the old targets from acquiring fresh authority.

## Acceptance

Exercise both resume and rebind through the production runtime and CPU
compositor, with a changed output size, an unchanged neighbor, and an
unrevoked-claim refusal. Prove the regression test fails when the reconciliation
is removed. Run the repository gate, then prepare a matched desktop release and
confirm KVM away/back recovery with the owner. Check the subsequent cleanup and
resource disposition independently; the original cleanup error is not yet proved fixed.

## Deterministic validation

All 383 `sophia-backend-live` library tests passed with all features enabled.
The initial three controls exercise changed-size resume, changed-size rebind, and
refusal to discard an outstanding shell presentation claim. Both repaint cases
use the real CPU compositor, retain an unchanged neighboring output, and check
that old input targets do not regain authority.

Temporarily replacing the exact-size comparison with an always-compatible
positive-size test made all three fail. The two repaint tests failed with the
same `shell content targets a stale output size` error as the installed session;
the claim test failed its required refusal. The comparison was restored before
the 383-test run. Native device facts and completions are simulated here.
A fourth control then added the owner's exact topology shape: the preferred KVM
monitor disappears, leaving the smaller direct monitor as logical output 1.
All four targeted tests pass. `umask 022; cargo-slot sophia cargo xtask check`
passed on the combined recovery/observer working tree: workspace tests, strict
Clippy, SDK checks, render-node pixel checks and retained evidence verifiers.
The fourth topology test was added after that gate's workspace-test build and
passed in the separate four-test run; Clippy checked the final source.
Logs are retained in
`~/.local/state/sophia/development-evidence/t327-kvm-size-20261014/`.
This is not physical KVM acceptance.

## Recovery release

Release `niltempus-9fcf63fe095175ee564d` contains Sophia `1e2c9e50a`, Hagia
`2ba92d984d0317ced163c1137d9641fddfce226d`, and integration
`0dbec396b86704867e0bfba20d1ce47ed6bee875`. Only Sophia's flake lock entry
changed. Hagia, narthex, Lom, Bemenu and kleis executables compare byte for byte
with the installed release. The Nix build and complete checksum verification
passed. Observer commit `146344c48` is excluded from this release.

Built store path:
`/nix/store/v2fpkdiv3ml6v5n22hy8vz1rjyd7mdk0-niltempus-desktop-niltempus-9fcf63fe095175ee564d`.
The first install attempt stopped because sudo requires a password and a terminal.
The owner subsequently installed the release; `current` now resolves to
`niltempus-9fcf63fe095175ee564d`. The installer created a GC root for the new
store path.
The short `~/t327-install` launcher invokes the normal installer with that
exact store path and passes `bash -n`. Build, checksum and failed-install logs
are retained beside the deterministic evidence.

## Attended recovery

niltempus confirmed that the session survived switching the KVM away and back.
The running session is
`00000001791986556232-e379ffa5-3f8f-4013-b517-b580c55b318d`, with Sophia
`1e2c9e50a0eddd5b26286a8530bfa6a288e62e47` and binary SHA-256
`74c84c80398c565d34322115b5d35b4266677db830738c406defde5e0b80d07d`.
Its retained `events.0.log` records native owner 1 settled at sequence 11934,
one output resolved at 11937, owner 2 settled at 12197, and two outputs committed
under owner 3 at 12377. Both retirements report `in_flight=false` and
`cleanup_pending=false`. Subsequent events present shell content on both outputs.
No stale-output-size or fatal error appeared in the inspected log.

At sequence 91440, recording remained running, with zero storage errors,
discarded records or rotated bytes; 50109 records were suppressed. This live
check establishes recovery and native-owner retirement, not final presentation
shutdown. The session has not yet exited, so that separate cleanup check remains.

## Connections

- [Earlier KVM recovery](kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md)
  records the earlier physical gates; this is a new observed failure.
- [Observer delivery plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t319)
  owns the work interrupted by this live incident.
