---
id: qqdj8dfu
date: 2026-09-28
kind: investigation
status: implemented
tags: [investigation, policy, lifecycle]
---
# Initial policy activation consumes negotiation before Session records it

## Question

Why can initial WM startup finish configuration and a layout commit while
Session still reports that pointer-focus capability is unavailable?

## Evidence

The regression was reproduced on `a21104d59` while moving the generic protected
control-owner fixture from the IPC demo to the independent C SDK over 9P. The
peer echoes exact profile identities, configures one action and proposes an
empty-output layout. It requires the pointer-focus capability. The parent
withholds layout commit to check that control completion cannot precede it.

After the first real layout commit, the new pointer-focus assertion failed.
The same assertion passes after the repair, including after both controlled
replacements. This is a Session state bug, not a physical pointer observation.

## Finding and resolution

`activate_public_launch` consumes the worker's `Negotiated` notification only
after successful profile admission. It then constructs `StartedPublicPolicyLaunch`.
`from_started_public_config` incorrectly initialized the running owner's
`negotiated` flag to false. That notification is not delivered twice, so the
initial owner never restored the flag. `pointer_focus_enabled` requires both
negotiation and configuration, even if the selected capability is present.

The constructor now preserves the completed negotiation. Configuration,
selected-capability publication and cycle readiness remain separate. Restart
paths still clear negotiation and await a fresh admission. The ordinary
transport-selection test asserts both that the initial owner is negotiated and
that pointer focus remains disabled while configuration is pending.

The old control fixture also omitted the production loop's input-idle call to
`settle_desktop_reload`, preventing staged action configuration from committing.
The new fixture performs that settlement. Existing commit-order assertions and
deadlines are retained. The WM connection is now 9P; administrative commands
still use control-v1, whose migration belongs to t254.

## Validation and remaining work

Evidence under `~/.local/state/sophia/development-evidence/ipc-retirement/`:

- `t269-startup-negotiated-before.log`: fails the initial pointer-focus assertion
  after layout commit with the old initializer.
- `t269-startup-negotiated-after.log`: passes the same assertion and the complete
  control-owner test: action commit, two replacement epochs, stale-action
  refusal, reload settlement and logout.
- `t269-control-files-focused.log`: retained C fixture compile error; corrected
  to read Action as a Cycle cause.
- `t269-control-files-focused-run2.log` and
  `t269-control-files-startup-diagnostic.log`: retained configuration-settlement
  failures; `t269-control-files-focused-run3.log` passes after fixture repair.
- `t269-control-files-full.log`: the complete offline `cargo xtask check` exits
  zero, including SDK snapshots, clippy, formatting, layout and tool verifiers.
  It reports 496 result groups, 6,716 passes, zero failures and 63 ignored.
  These totals include fixture-child reports rather than unique test identities.
  The ignored independent C control test is covered by the separate focused run.

The tests run in a device-hidden, offline sandbox. They prove owner state and
independent SDK interoperability, not native presentation or physical focus.
No installed release or running session was changed. This is preparation for
t269; t250/t252 qualification and rollback requirements remain in force.

## Connections

- [WM file contract](../../sophia-wm-files.md) separates file negotiation,
  profile admission and configuration; this repair preserves that ordering.
- [Pointer focus](nsu4a0n2-optional-pointer-focus-follows-presented-targets-through-committed-policy.md)
  retains its presentation and physical-acceptance requirements.
- [IPC retirement inventory](1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
  tracks the remaining compatibility removal and qualification work.
