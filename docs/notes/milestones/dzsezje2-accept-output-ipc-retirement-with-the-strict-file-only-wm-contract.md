---
id: dzsezje2
date: 2026-10-01
kind: milestone
status: recorded
tags: [milestone]
---
# Accept output IPC retirement with the strict file-only WM contract

## Result

T272 meets its output IPC retirement exit. Signed Sophia
`21bdf9f60ec1aca53e13f8d6f828be8ddebcb21d` passed the full repository gate,
fresh native preparation, performance qualification and all four attended
native stages with SDK 0.4 and the matching external WM. Signed merge
`4c1012b890ef069ce8d4623a67c41d07f354a7b6` promotes that exact tree to master.
Contract C remains reachable; master never held the incompatible intermediate
SDK/API pair. This is source acceptance, not desktop installation.

The [implementation record](../investigations/j8jkd97a-retire-the-output-socket-after-native-file-role-acceptance.md)
maps retired socket coverage to retained file and owner tests. Native profile
authority needs no output listener; only an explicitly configured protected
output process gets the 9P role. The WM receives no output grant. Passive
revision-1 domain types remain, while output IPC framing and its endpoint are
gone. The strict WM API names `output_transport=9p2000.L`.

## Candidate and evidence

| Input | Identity |
| --- | --- |
| Integration | `20f98131e65ac5ba45b00aa325e53f5f80bcfe8f` |
| C SDK 0.4.0 | `497e7e01531415078a4a3da2455ebe82ec18fd0e` |
| C SDK manifest | `ab45a6460bef0606210c9dd3da168dafdd9f3f902a639a3cdfaf6bc3945bf5af` |
| Rust SDK documentation import | `b38b809e1a09a62911dd940cfcc5067a7ee5b06d` |
| External WM | `0de7ef229146e6abedff98d726f0afb0815bb218` |
| Inputs manifest | `5533fa478ab7163ce6d7fd54fb1fd56f00934f008ff3f8b6f9d8830971ee6c1a` |
| Preparation | `e2148fe443e14a29397c63d809bba80b21460d3c3bc9779592af7b82540b757e` |
| Run manifest | `0c223cf726aa623596873904a8009b7f7644b1815dbc1f43912d10e4b2156922` |

Evidence is under `~/.local/state/sophia/development-evidence/ipc-retirement/`:

- `t272-retirement-01/34-shared-peer-full-check.log`: workspace and SDK tests,
  strict Clippy, layout, wire and archive verifiers pass. Earlier failures and
  their fixes remain recorded. Native preparation on the signed source passes
  all 13 peer cases and the Session-supervised fixture in
  `t272-native-21bdf9f60-01`.
- `t272-perf-21bdf9f60-01`: small/maximum connection p99 is 30.743/30.885 ms;
  proposal p99 is 7.433/7.525 ms; idle CPU is 0.203–0.209% of one core.
  Independent recomputation matches every summary; all declared bounds pass.
- `t272-native-run-20f9813-01/run`: validate, transport rejection,
  commit-and-restore and peer-death rollback pass. Every stage reaches the
  external WM's file bootstrap, exits zero and proves clean console recovery.
- `t272-final-audit-20f9813-01`: independent offline verification reproduces
  the four-stage verdict and binds all stage-log digests to the manifest.

In the peer-death stage, supervisor SIGTERM exit and Disconnected follow the
all-cards-applied termination request. Sixteen rollback wait turns precede
reverse programming. KMS and labelled owner readback restore the original
120 Hz mode; local RolledBack settlement and the typed peer-loss pass follow.
There are no MissingCycle errors, quiescence timeouts or frame-slot deferrals.

## Limits and rollback

Physical acceptance covers one machine, one card, two heads and a refresh-only
change on one head. It proves readback and owner state, not pixels, resolution,
position, enablement, transform, mirror or multi-card changes. There is one
peer-loss boundary. The retired startup-only proof has no replacement physical
startup-transaction rollback claim. Replacement-bootstrap ordering is covered
by its deterministic regression and failing mutant, not by this attended run.

Rollback restores the whole previous pair: pre-C Sophia with the previous WM
and SDK. Both mismatched API pairs refuse bootstrap. Historical frame-fed
archives remain valid for their original source pins. No revision-2 Confirm,
persistence, docking or product output launcher is admitted by this result.

The [migration plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
owns subsequent scope; the [t253 milestone](ofard23a-accept-the-revision-1-output-file-role-through-native-rollback.md)
retains the distinct pre-retirement acceptance.
