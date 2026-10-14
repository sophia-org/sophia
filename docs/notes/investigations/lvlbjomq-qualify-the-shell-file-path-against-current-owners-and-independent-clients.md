---
id: lvlbjomq
date: 2026-10-14
kind: investigation
status: investigating
tags: [investigation, shell, 9p, qualification]
---
# Qualify the shell file path against current owners and independent clients

## Question

What remains to qualify t252 after source retirement and the later shell-owner
repairs? Reconcile the existing evidence before repeating a desktop campaign.

## Current resumption

Reviewed Sophia `96d945b76` on October 14. Generic C/Go export, descriptor,
content and Session component checks pass. The protected joined-launcher test
failed because its resource-wait loop stopped advancing the Session owner after
negotiation. The fixture now polls that owner on every pass; the same protected
test passes with its five-second deadline and identity checks unchanged.
Production code is unchanged. Repository-wide validation is recorded below.

t252 remains open for exact paired product evidence, native lifetime/input
acceptance, the declared B9 performance campaign and verified release rollback.
The old E1 independent descriptor gap is resolved by the existing C file peer;
the September plan's open wording is historical. Do not infer a new installed
desktop qualification from these headless checks. The t328 route-lease repair
proposal remains a separate correctness concern before final acceptance.

## Evidence

Commands ran through `cargo-slot sophia`, offline and locked, using the existing
slot 1. These are correctness checks, not quiet-host timing samples. No native
scanout, input-device access or installed-session change was requested.

| Scope | Current result | What it does not prove |
| --- | --- | --- |
| `sophia-runtime`: `shell_files_c`, `shell_files_c_roles`, `shell_files_oracle`, `shell_native_sdk` | 7 test cases pass, including independent C vectors, production export exchange, launcher/dock sessions and the Go verdict controls. | Product UI, native pixels or supervisor authentication in the supplied-admission oracle. |
| `sophia-session`: `shell_component_connections_files`, default features | 9 pass. | Native-feature joins omitted by this build. |
| Same connection target with `native-session` | 13 pass, including three independently negotiated roles, borrowed native content and exact indicator publication. | Physical presentation; these controls use supplied completion facts. |
| `shell_component_processes`, default/native features | 1/7 ordinary tests pass respectively. The child entry and protected tests remain ignored in the ordinary invocation. | Ignored tests are not counted as passes. |
| Protected two-process custody | Explicit device-hidden invocation passes: independent stop/replacement, retained bytes and cleanup. | Native pixels or three-product coexistence. |
| Protected joined-launcher identity | Fails at the resource-wait deadline before the fixture repair; passes afterward with two generations, stale grant refusal and cleanup. | Installed launcher UI or physical focus. |
| `sophia-conformance`: `shell_content_files`, `shell_descriptor_modes` | 4 content and 5 descriptor tests pass, including malformed/mutated peers, persistent tabs/reference, reservation commit/withdrawal and launcher replay controls. | Product release identity or installed rollback. |

The protected tests ran the already-built native-feature test executable under
`bwrap`: read-only host filesystem, private `/dev`, `/proc`, `/tmp` and
`/run/user`, private PID/network namespaces, no DISPLAY or WAYLAND_DISPLAY, and
a private runtime directory. Only the named parent test was selected; its child
entry ran through production protected launch. No test binary was copied into
retained evidence. Cargo outputs and the red/green test results are retained in
the session transcript. The initial canonical gate log is
`~/.local/state/sophia/development-evidence/t252-owner-review-20261014/check.log`.
That invocation reached automatic render-node probes: buffer-age tests passed
on `/dev/dri/renderD128` (two passed, one ignored). It was stopped after the
first-frame wrapper selected the same node and began preparing its tests.
This was not the intended device-hidden scope and is not promoted as hardware
acceptance. The first device-hidden attempt, `check-device-hidden.log`, failed
three Session unit tests because its runtime directory was `/tmp`, which does
not meet private-directory admission. The corrected run uses a mode-0700
runtime directory, private `/dev`, `/tmp`, `/run/user` and PID/network
namespaces. Its log is `check-device-hidden-private-runtime.log` in the same
directory. Builds and tests use eight workers.

## Finding and resolution

`ShellComponentSession::poll` advances connected wires through
`turn_connected`; `ShellComponentTransport::service_owner_turn` resets the
per-pass service guard. The fixture previously borrowed resource service and
called `poll_io` repeatedly after reaching Connected, without starting another
owner pass. A newly arrived upload could therefore remain unserviced until the
test timed out. Polling Session in that wait models the actual owner loop.
The test also rejects unexpected child exits and negotiation failures while
waiting. The fix does not enlarge a timeout or relax a grant check.

The independent descriptor host evidence is already recorded under
[t271](1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md#independent-descriptor-hosts-over-files-t271-2026-09-28).
The current rerun covers its generic E1 cases. Narthex product compatibility and
whole-release rollback still need exact release identities; codec independence
does not supply those identities.

## B9 method preparation

Read-only product reconciliation found the external desktop's current
`flake.lock` pins Sophia `a29663a06`, Lom `dc65f561`, Bemenu `6e2efd96` and
Narthex `56d513fa`; it contains no Provlita input. Local Provlita HEAD is
`da4a15ad`. These are source selections, not verified installed artifacts.
The older Bemenu SDK-adoption note records an actual protected executable join
at Bemenu `a354251a` and Sophia `c23a38453`, including close/reopen and resource
retirement. Thus the old blanket "Bemenu r7 remains" wording is also stale;
the remaining question is coverage on the exact selected candidate, especially
the three-component target. No external checkout or pin was changed here.

Keep every numeric budget in [the shell file contract](../../sophia-shell-files.md#budgets).
Use an external comparison harness with frozen sources and independently built
clients. A historical IPC binary is not automatically comparable to current
owners: first document semantic equivalence or restore only the retired test
transport in an isolated comparison tree, as t249 did. Never restore it to
production or call unmatched owner costs transport overhead.

Declare these workload families before collecting acceptance samples:

- Bar-sized and 4 MiB resource upload through acceptance, with exact payloads.
- Candidate submission through actual Presented at 60 and 120 Hz, plus
  frame-demand/permit timing. Receipt time is not presentation time.
- Three independently admitted generic roles, including a stalled reader and
  replacement; report per-role progress, retained bytes and final reclamation.
- CPU per uploaded MiB and allocations per frame, with complete event totals.

Use alternating paired transport order under idle and declared load. Report
all offered, accepted, completed, refused, coalesced and timed-out operations;
never select matching survivors. Pin sample counts, warmup, hardware/output
identity, clock boundaries, instrumentation and failure rules before running.
Do not silently import t249's outcome-accounting amendment into shell traffic:
identify which operations may coalesce under the shell contract. An incompatible
comparator or proposed budget/outcome-rule change needs review before the
campaign, not reinterpretation after a failure. This is method preparation,
not a frozen runnable campaign or a claim that B9 passed.

## Validation and remaining work

The corrected device-hidden `cargo xtask check` passed (exit 0), including
workspace tests, strict Clippy and the available script checks. Hardware-dependent
proofs were skipped, not passed. The source was based on `d160fd03e` with this
fixture repair and the separate t254 codec foundation; no installed artifact
was selected. `git diff --check` also passed.

Next prepare the exact external paired-client matrix and runnable B9 method,
resolve the route-lease concern and schedule the required native/attended gate.
Do not mark t252 complete or advance its default/rollback acceptance on the
strength of the generic checks alone.

## Connections

- [t252 exit](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t252--join-and-accept-the-shell-path).
- [t328 delivery audit](jxcqbd04-audit-remaining-topology-and-delivery-obligations-before-architecture-acceptance.md).
- [Source-retirement decision](../decisions/twkn9fsp-retire-wm-and-shell-ipc-with-release-rollback-while-latency-qualification-remains-open.md)
  preserves performance and physical acceptance separately.
