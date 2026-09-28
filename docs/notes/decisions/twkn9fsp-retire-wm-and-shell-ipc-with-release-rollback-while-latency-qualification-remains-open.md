---
id: twkn9fsp
date: 2026-09-28
kind: adr
status: accepted
tags: [adr, protocol, compatibility]
---
# Retire WM and shell IPC with release rollback while latency qualification remains open

## Context

At Sophia `d03690fb5`, t269 requires t250/t252 qualification before removing
WM and shell IPC. The recorded t249 campaign refused all 40 pairs. Its
survivor populations differ and both wires miss some latency budgets; it does
not establish transport causality or qualify a default switch. That verdict
stands. The later product direction is 9P-only, and the current client
implementations have removed their IPC paths.

The existing plan keeps two different decisions coupled: accepting measured
daily-driver behavior and removing compatibility source. Continuing to prepare
tests cannot resolve that policy choice. This proposal makes the choice explicit
without treating ordinary-use reports as latency measurements.

## Decision

Permit t269 to remove core WM and shell IPC once
its independent 9P coverage, publication and rollback requirements below pass,
while leaving t249/t250 and any outstanding t252 qualification open.

Required before completing t269:

1. Preserve generic behavioral coverage for admission, configuration, profile
   handoff, content/descriptor lifecycle, input, revocation, supervision and
   recovery. Record each retired socket test's retained file/owner counterpart;
   byte-specific socket tests retire with their wire.
2. Make WM and shell defaults explicitly 9P, reject retired selectors and socket
   variables, remove their old workers/codecs/generated bindings and obsolete
   dependencies, and update public contracts and SDK compatibility records.
   Shared code still used by another role remains under its actual owner.
3. Pass the complete isolated repository gate and independent SDK export tests.
   Preserve product tests in their own repositories and named-stack tests in
   niltempus. No physical claims follow from a device-hidden gate.
4. Publish exact source/SDK identities and a rollback recipe to a previously
   verified, unchanged whole release. The recipe must identify its compatible
   component set, manifest identity and verifier. Rollback is a deliberate
   next-login selection; it cannot silently substitute an IPC component in a
   9P-only running session. Prove selection and tamper refusal with private
   installer tests. Do not claim an installed rollback target was verified
   merely because the installer supports one.

t270 may then publish SDKs without the retired compatibility APIs and re-vendor
them. The source retirement decision does not qualify performance, close a
physical acceptance row, authorize a new installation, or change a running
desktop. Numeric latency budgets remain unchanged. A later campaign requires
a declared method and its own evidence.

The t269 dependency description now distinguishes completed implementation and
coverage from still-open daily-driver qualification. Defaults change with the
tested implementation, not with this documentation amendment.

## Alternatives

- **Keep the current dependency:** retain core IPC until a new declared campaign
  and the remaining attended/physical checks qualify the migration. This was
  the previous rule; the operator selected separate source retirement instead.
- **Treat the earlier live session as qualification:** rejected. Operator
  observations do not supply the missing paired measurements or change the
  recorded failure.
- **Retire compatibility with no rollback recipe:** rejected. A sealed,
  compatible release remains the recovery unit even after source removal.

## Consequences

This unblocks the requested WM/shell source purge without misreporting the
latency verdict. It also makes current builds unable to launch legacy WM/shell
clients; compatibility would require the documented older release as a whole.
The documentation must label the default as experimental while qualification
remains open.

Output and administrative IPC are separate roles. This proposal does not bypass
t253's requirement to identify a real output-role consumer, authorize a new
consumer, or close t272. It also does not implement the broker/portal designs.

## Acceptance and connections

Accepted by niltempus on 2026-09-28 after the proposed decision was presented
in full. The operator approved source retirement with verified whole-release
rollback while latency qualification remains open. This changes the retirement
prerequisite; it does not change a numeric threshold or itself alter runtime
defaults. Installation and running-session changes remain separate actions.

- [Daily-driver migration plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  owns the t249 refusal, t250/t252 qualification and t255 retirement policy.
- [WM acceptance plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md#t249)
  owns the unchanged numeric budgets.
- [IPC inventory](../investigations/1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
  owns the removal list and retained coverage.
- [WM](../../sophia-wm-files.md) and [shell](../../sophia-shell-files.md)
  contracts must reflect the actual selectors and compatibility window when
  implementation lands.

Recovery mechanism: niltempus `installer/install.go::rollback` selects the
verified previous release for the next login, leaving running sessions unchanged.
The [retirement investigation](../investigations/1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md#session-wm-ipc-worker-retirement-t269-2026-09-28)
records the installed release and activation identities verified read-only,
private-mount selection and tamper tests, and the external recovery recipe.
Ordinary rollback preserves personal component choices; the recipe separately
selects the sealed components when the complete older set is needed. No graphical
recovery run or host selection was performed.
