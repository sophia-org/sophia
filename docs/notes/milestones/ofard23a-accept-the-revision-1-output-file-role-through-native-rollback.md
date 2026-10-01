---
id: ofard23a
date: 2026-10-01
kind: milestone
status: recorded
tags: [milestone]
---
# Accept the revision-1 output file role through native rollback

## Result

T253 meets its revision-1 output-file exit on signed Sophia
`ddd27bd6d9ac6d8e73394d9326705a7a62916f35`. The attended four-stage run passed
validation, transport rejection, commit A-to-B-to-A, and supervised peer death
after native apply with observed restoration. Independent offline verification
reproduced the complete-run verdict. This accepts the scoped role migration;
it does not install a desktop release or retire the old output IPC source.

The [approved sequence](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t253--migrate-the-separate-output-role)
now admits T272. Its final default is profile-owned output configuration with
no listener when no output process is configured. An explicitly configured,
protected output process receives the separate 9P role. The frozen C reference
consumer remains SDK evidence; product invocation is T254 and confirmation is
a later wire revision.

## Candidate and custody

| Input | Identity |
| --- | --- |
| Sophia | `ddd27bd6d9ac6d8e73394d9326705a7a62916f35` |
| Integration | `bec6db137d7edafef19e44781e00239335977a74` |
| C SDK | `7ccfece173b4b01e563a27b8fe5cc07d4369b55a` (0.3.0) |
| SDK manifest | `2259db2fc97b0c31dccfb3b93b8e64fea53c64de267e25c43be4ffff9a140907` |
| External WM | `b36af2951114f80efbb04a367e12165be9f3cd66` |
| Inputs manifest | `30b12d96b0879e1e2ba29f811fe66d0a534d51565cd385325ed0d90baf4f1b55` |
| Preparation | `57898ba3e91dcbfbdc8b9b1394fd03e615406a543cf86f1c2ad77108a5ef6e10` |
| Peer binary | `dd124a55d026fa076e2758b4fb16ad1e7d448a70611f25ae4f7e2086b276ebf7` |
| Run manifest | `3caee597b97ba77c4e6a87f26c5359abc3000310e82843c3147e53ff1f425b3d` |

Evidence below is retained under
`~/.local/state/sophia/development-evidence/ipc-retirement/`.

- `t253-native-run-bec6db1-01/run/`: four logs bound by `run.manifest`, all
  Session exits zero, foreground-console checks and console/keyboard recovery.
- `t253-native-bec6db1-audit-01/`: independent complete-run verifier invocation,
  recomputed identities and PASS. `t253-final-audit-bec6db1-01/AUDIT.md` records
  the second review of the same evidence.
- `t253-native-ddd27bd6d-01/`: exact signed preparation, thirteen public-SDK
  export tests and the protected Session fixture. These supplied-owner tests
  make no physical claim; the attended run supplies it.
- `t253-perf-ddd27bd6d-01/`: one release-profile measurement, both fixtures,
  every raw sample and the final passing verdict. The sample file digest is
  `c09d7aae7894b76b2b0df37ae83c6bcc89fb50c81227521b42c0a3f535dbae71`.
- `t253-rollback-drain-01/` and `t253-rollback-quiescence-01/`: backend and
  Session regressions, strict workspace clippy, and separate-target mutation
  controls for applying before quiescence and accepting readiness too late.

The native layout has one card and two enabled heads. Startup commits epoch 2:
DP-1 is 2560x1440 at 120 Hz (head 1, mode 260), DP-2 is 1920x1080 at 60 Hz
(head 2, mode 513). B changes only DP-1 to 60 Hz (mode 257), retaining geometry,
Normal transform, Fit mapping and Disabled VRR. Commit-restore publishes epochs
3 and 4 with fresh Qids. Peer death requests termination after all cards apply,
then observes signal 15, pause and the matching disconnect. Sixteen rollback
wait turns precede reverse apply, restored KMS and owner rows, local RolledBack
settlement at epoch 2, and the joined peer-loss pass. There is no fatal error,
MissingCycle, quiescence timeout or slot deferral in any stage.

## Performance and regression results

| Fixture/workload | p99 ms | Maximum ms | Limits p99/maximum ms |
| --- | ---: | ---: | ---: |
| Small/connect, 100 | 30.758 | 30.765 | 100/500 |
| Maximum/connect, 100 | 30.900 | 31.001 | 100/500 |
| Small/proposals, 1,000 | 7.445 | 7.511 | 50/250 |
| Maximum/proposals, 1,000 | 7.521 | 7.563 | 50/250 |

Twenty warm-ups per proposal fixture are excluded from those percentiles but
included in the exactly-once join: 2,040 settlements equal 2,040 deliveries.
All 204 connections disconnect; retries are zero; thread sets are restored and
cleanup is clean. Six ten-second idle intervals consume 0.208–0.215% of one
core combined, below 2%. The worker still wakes periodically; wakeups are
reported, and CPU is the declared gate. Niltempus's September 30 instruction
removed priority and build-job throttles: this preparation records 32 jobs and
nice 0. Thresholds and serial test ordering are unchanged.

The attended pass resolves the observed failures tracked in
[installed rollback timing](../investigations/l696uhdc-rollback-preparation-must-use-the-installed-mode-after-a-topology-commit.md),
[singleton framebuffer custody](../investigations/foc74sm7-topology-rebind-must-transfer-singleton-framebuffer-custody-before-ordinary-presentation.md),
and [rollback completion draining](../investigations/0bc9j2k2-rollback-must-drain-candidate-presentation-before-replacing-completion-trackers.md).
All earlier failed runs remain under their original identities.

The generic role regressions also retain epoch fencing, replay and bounded
custody, listener pause through a real Tversion probe, checked pidfd handoff,
startup settlement remaining local, and cancellation that preserves preparation
debt. The full workspace gate is not claimed green: retained logs 05–07 in
`t253-rollback-drain-01` record an isolation prerequisite failure followed by
the pre-existing shell descriptor fixture's `--serve` startup race. The focused
output suites and strict workspace clippy passed; that shell fixture needs a
separate correction.

## Limits and next boundary

This is one attended run on one machine, with one refresh-only change and one
peer-loss boundary. It does not qualify resolution, position, enablement,
transform, mirror or multi-card changes. KMS and owner readback are not pixel
proof. Drain waiting is visible through sixteen retries; the log cannot
distinguish a pending renderer export from submitted flips. Repeated
cancellation records are diagnostic noise, not repeated terminal outcomes.
Mixed child/Session output is trusted through the frozen peer and signed,
sealed external inputs; record text alone is not source authentication.

T272 must preserve this generic evidence path while removing IPC and qualifying
the assembled strict-API Sophia/SDK/WM replacement. Its source and release
identities will differ, so this run does not pre-accept that replacement.
Whole-release rollback and attended authorization remain required. The
[active task ledger](../../../todo.md) owns the remaining work.
