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

The candidate fix withdraws shell frames whose output disappeared or changed
pixel size, both during native resume and during a quiescent topology rebind.
It refuses to discard an unrevoked presentation claim. Deterministic tests and
an attended KVM switch on a matched release remain required; no fix is installed.

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

## Connections

- [Earlier KVM recovery](kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md)
  records the earlier physical gates; this is a new observed failure.
- [Observer delivery plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t319)
  owns the work interrupted by this live incident.
