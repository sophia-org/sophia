---
id: 87juczar
date: 2026-09-26
kind: investigation
status: investigating
tags: [investigation]
---
# WM file reactor waits behind the driver command channel

## Question

Why does the same protected normal Hagia show a periodic extra delay on the
file transport in the new Session drag smoke? Which owner may remove it
without changing admission, phase permissions or semantic settlement?

## Evidence

Hagia's signed measurement tooling `48fed33e4c1847f49637feba933d73dd8d721006`
overlays exact Sophia `28eed9742776f2ae2bb84336528790924220adc5`, using the
unchanged frozen normal `7455c3e` executable. The sixteen debug fixture runs
at `hagia-wm-measurements/.artifacts/measure/pair-third` offer and admit 256
updates: 235 settle, 21 coalesce, and none reject, time out, disconnect or stay
unresolved. They cover move/resize at 60/120 Hz, idle/two CPU workers, on both
wires. Geometry uses the actual reducer's outer allocation and Engine's
chrome conversion for content layers. Supplied frontend ACKs and CPU facts
are not application/native completion.

Every pair refuses at least one declared latency budget; the runner exits 1
and retains the complete report. At 60 Hz the current-IPC p95 range is
6.153–10.338 ms and files 15.279–17.020 ms. The 120 Hz pairs have different
survivors and cannot establish comparative survivor latency. These small
debug samples diagnose the harness and scheduling path; they are not the
release campaign or daily-driver acceptance. No threshold was relaxed.

Hagia's `tzz6apym` investigation and `tools/sophia_pairing/README.md` retain
the workload, exact hashes, first fixture failures, accounting rules and
remaining evidence. The ordinary overlay separately passes all 25 retained
owner/legacy cases and strict Session/runtime, layout and format checks.

## Finding and resolution

Source at this exact base confirms a scheduling asymmetry:

- `policy_transport_worker/driver.rs` waits on `commands.recv_timeout(10 ms)`.
  Only a timeout calls `try_receive(DirtyOnly)` while idle.
- `ninep/runtime_adapter.rs` delegates that call to a reactor turn.
- `ninep.rs::send_encoded_before` appends an event and rings the wake pipe,
  but a successful append does not turn the reactor. Capacity pressure does.
- A Cycle immediately enters `receive_within`. ProjectionOutcome and other
  non-Cycle commands return to the command-channel wait instead.
- Current IPC writes and flushes its outcome frame inside send.

Consequently appended file events and new client ACK/clunk/flush/read
requests can wait until the next Cycle or idle timeout. The wake pipe cannot
make progress while nobody polls it. Local revocation and semantic phase
authority remain with their existing owners; this is a servicing delay.
The observed 60 Hz beat is consistent with it, but its contribution to the
measured difference needs a controlled change and release rerun.

The bounded correction uses an accepted-command bell and an idle reactor wait
on both socket and wake readiness, with staging deadlines retained. Driver
code still mints DirtyOnly permits and interprets events. An idle turn returns
to the command queue after readiness; active projection receive retains its
existing loop. Current IPC keeps its existing waiting path. A shorter fixed
timer, busy polling, or one flush immediately after send would not establish
the required idle ACK/clunk progress and is not the proposed correction.

## Validation and remaining work

Signed correction `9f03d3e8424184c1ceb4b8c4b51bd1e3a57de6b4`, joined as
`82085333571141eb8662bcf3468877442c72d7dd`, passes generic controls for
outcome delivery without another command,
idle ACK/clunk/flush service, admitted-only command wakes, enqueue races,
ordered commands, quiet no-spin behavior, Stop and unchanged DirtyOnly
refusal. All seven new controls pass within the restored worker suite:
59 passed, three explicitly ignored. Strict native Session all-target Clippy,
fresh-worktree layout, workspace/direct formatting and whitespace pass.
The active receive source is unchanged; current IPC retains its old branch.
A compiled selector negative enters the old fallback and fails with the
external test's `legacy_fallback_refused` marker. Exact restoration passes.
This proves the selected servicing path, not a numerical latency bound.

The unchanged Hagia workload then runs at exact `820853335` in
`hagia-wm-measurements/.artifacts/measure/pair-idle-fix`: all sixteen cases
complete, with 256 admitted, 229 settled, 27 coalesced and no terminal failures
or unresolved work. The campaign still exits 1: six of eight pairs refuse
budgets. Three 60 Hz file runs have medians of 1.74–1.85 ms, compared with
roughly 9 ms before the correction. Ordinal 02 is an **idle**, not CPU-loaded,
move run with two survivors at 647.7 and 722.6 ms. Its manifest binds a load
record with zero workers. The 120 Hz interval and survivor failures remain.
No threshold or checkpoint behavior changed.

The outlier coincides with long gaps between Hagia's checkpoint-save messages.
Its synchronous file and directory fsyncs share the evidence/build filesystem.
Durable-storage latency is a hypothesis, not an attributed cause: retain the
failure and distinguish it with bounded I/O-pressure/checkpoint timing or a
declared order control. Do not remove fsync, discard samples or redefine the
gate after the result. The full release campaign has not run.

The separate ordinary overlay at `.artifacts/measure/retained-idle-fix` passes
all 25 named owner/legacy cases, strict native Session/runtime checks, fresh
layout and workspace/direct overlay formatting. The source fix bundle is
`~/.local/state/sophia/development-evidence/t249-wm-idle-9f03d3e8`; the original
measurement bundle is
`~/.local/state/hagia/development-evidence/h006-drag-measurement-48fed33`.
t249 remains open and installed current IPC stays the default. Output and
shell remain current IPC; this grants no physical acceptance.

## Connections

- [WM file migration plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md)
  owns t249 and its latency budgets.
- [Typed driver ownership](uf2wya88-typed-wm-driver-preserves-current-ipc-phase-and-shutdown-ownership.md)
  preserves the phase and cancellation boundary being serviced.
- [WM files](../../sophia-wm-files.md) separates byte custody, acknowledgements
  and semantic outcomes; reactor progress must not collapse them.
