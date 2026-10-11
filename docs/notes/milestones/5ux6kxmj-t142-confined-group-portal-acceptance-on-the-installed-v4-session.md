---
id: 5ux6kxmj
date: 2026-10-11
kind: milestone
status: recorded
tags: [milestone, session, portal]
---
# t142 confined-group portal acceptance on the installed v4 session

## Result

t142's installed-session acceptance passed on 2026-10-11. niltempus confirmed
confined Firefox to shared kitty CLIPBOARD paste, PRIMARY selection paste,
refusal of reverse CLIPBOARD transfer, and continued shared-terminal use after
closing all confined browser windows. Shared-to-shared paste still worked.
Read-only diagnostics confirm the browser launch exited with code 0 and signal
0; no Firefox process remained. No automated input or process termination was
used. These are attended observations, not captured pixel or timing proofs.

The session was
`00000001791724571605-8967bb2f-f8e7-4535-9daa-3b4bd82c9e3d`, running signed
Sophia `9bd3a4494b588fa56f2169a604cab2e136dc8405` in release
`niltempus-af190daf0be3bfb9d76c`. The diagnostic manifest binds the executable
SHA-256 `4dc5fa3d28f3aae12de76960673e42c6671db774de4fb9c5064c0e7a3badb037`.
Integration commit is `2085f8afc2f2c123da7d23025c879eaa38112e77`.
The v4 acceptance configuration is identical to v3; only its release changes.

## Evidence and scope

The [investigation](../investigations/rd9nia0s-several-x-listeners-and-confined-application-launch-validation.md)
binds the implementation, real Bubblewrap path exclusion, source and protocol
controls, M3/M5 results, and final isolated full gate. The final gate has 7,625
passing results, zero failures and 105 ignored across 549 libtest summaries.
It executes both
`listener_groups_reclaim_only_after_every_listener_is_idle` and the ten-round
`departures_are_reclaimed_during_the_run_and_the_instance_keeps_admitting`.
The attended departure proves shared-session survival; it does not claim full
idle reclaim while shared clients remain connected. That obligation is covered
by the deterministic tests. The real confinement probe proves trusted-path
exclusion; it was not replaced with an authorization-denial check.

The attended package is
`~/.local/state/sophia/development-evidence/t142-v4-attended-01`, with
`report.json` recording four PASS rows and the supplied/human observation limits.
Its verified, self-excluding `SHA256SUMS` has SHA-256
`4e249f14cc7ebb0873f21b9e16d49530840ae01c24b1472359af2f37218d4fe1`.
`t142-authority-integration-01` separately binds the signed source, complete
tested crate/Cargo delta, release manifest and binary hashes; its manifest is
`ff0be6a1bf1f5f5043a59bfb875f433d23b7eae8e02c65f0298e988d49171bff`.

This completes the [t142 exit](../plans/ooy00zjd-socket-directory-and-frontend-multiplexer-architecture.md#phase-3--live-acceptance),
not t249, a production 9P application frontend, or every portal kind's executor.
The [unified authority decision](../decisions/bdxxcof6-unify-portal-transfer-authority-across-protocol-adapters.md)
retains those boundaries.

## Residual observation

niltempus reported a slow initial browser launch. The launch record shows six
milliseconds from the recorded shortcut launch request to spawning Bubblewrap;
this is not input-to-window latency. Firefox reported nested namespace and
D-Bus/machine-ID warnings. Their causal relation to the delay is unproved.
No startup performance acceptance is claimed, and the sandbox's denied network,
private home and software-rendering fixture remain unchanged. This observation
does not invalidate the completed isolation, transfer or departure checks.
