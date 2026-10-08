---
id: b0yjm547
date: 2026-10-07
kind: plan
tags: [plan, development, rendering]
---
# Separate GPU development from the live desktop

The operator requests Codex and Claude work concurrently on separate GPUs and
close tasks once their acceptance criteria hold. The normal desktop remains
usable on discrete DP-1. Development automation is driven from the host; a
second physical keyboard or mouse is not required for automated controls.

## Ownership

- Codex: iGPU runner/admission, t289 and this setup.
- Claude: dGPU offscreen correctness, t309 startup diagnostics, and the
  existing t306/t307 work. A device runner does not waive source review.
- Each agent uses its existing checkout and private build target. Concurrent
  CPU work is allowed with bounded parallelism; comparative measurements still
  need an agreed quiet window across both agents and the live desktop.
- Accepted work with a green required gate is merged and pushed under the
  standing rule. A task requiring physical acceptance stays open until that
  acceptance, even when a code slice has landed.

Host identities observed on 2026-10-07:

| Use | PCI identity | Current render node | Connector |
| --- | --- | --- | --- |
| Daily desktop / Claude offscreen | 0000:03:00.0 | renderD128 | DP-1 |
| Codex development | 0000:16:00.0 | renderD129 | HDMI-A-2 |

Node numbers are observations, not selection policy. The runner must resolve
and check the physical identity each time. The normal desktop currently opens
both cards even though its profile disables HDMI-A-2.

## t311

Provide a bounded runner for an already-built, hashed renderer test executable.
Select exactly one PCI GPU and expose only its render node in a private mount,
PID, IPC and network namespace. Hide primary DRM nodes, physical input, VTs,
live display sockets, session control sockets and the user's home. Start with
clean environment and standard input; inherit no ambient device descriptors.
Give the test a private temporary home and runtime directory. Serialize runs
on each GPU while allowing different GPUs concurrently.

Record the test and runner hashes, command, physical device and node identity,
namespace-visible devices, exit status and timeout/cleanup outcome in a fresh
output directory. A successful process exit does not by itself prove hardware
rendering or pixel correctness: the selected test must provide those controls.
This is for correctness, not a quiet-host performance result.

Exit: deterministic controls reject ambiguous/wrong devices and prove the
command construction has no extra devices or ambient display state. An actual
sandbox control confirms denied access to the other GPU, card nodes, input,
VTs and host session sockets. A bounded renderer correctness invocation on
each GPU passes while the daily desktop remains alive. No KMS, seat change,
lock, install or hotplug is part of this exit.

### Qualification, 2026-10-07

Runner source `68c75b1064269360674a0c4b746d77bac97529ff` passed the
isolated full `cargo xtask check` gate with devices hidden. Its 17 Python
controls are part of that gate. Claude's read-only source review found no
blocking admission or confinement issue.

The actual sandbox control denied opens of the other render node, both primary
cards, physical input, physical VTs and session sockets. `/dev/tty` had no
controlling terminal, and the live abstract X11 endpoint was unreachable.
The first preflight attempt stopped before launch because bubblewrap required
explicit `--unshare-user` alongside `--unshare-all`; its failed receipt is kept.

Both agents then used the same frozen `snapshot_reuse` executable, SHA-256
`20b714f10418a1316d181f9938fa9ba0621e4ff6cab78eb5b527e04f43b44352`,
for one invocation per GPU with a 60-second limit:

| Agent | PCI GPU | Snapshot tests | Exit |
| --- | --- | --- | --- |
| Codex | 0000:16:00.0 | 6 passed, none skipped | 0 |
| Claude | 0000:03:00.0 | 6 passed, none skipped | 0 |

The tests assert immutable pixels after producer rewrites, bounded allocation
and import reuse, descriptor lifetime through donor destruction, and eviction
and cleanup. Each namespace exposed only its selected render node and inherited
no descriptor above standard streams. No test process remained afterwards.
The live desktop's PID and start time, its helper processes, and connector
states were unchanged: DP-1 enabled, HDMI-A-2 disabled.

Evidence is retained under `development-evidence/igpu-development-01`:
`runner-gate.log`, `runner-boundary-01`, `SNAPSHOT-QUALIFICATION.json`,
`igpu-snapshot-01`, `dgpu-snapshot-01`, `desktop-before.json`,
`desktop-after.json`, and `RESULT.txt`. `RUNNER-REVIEW-LIMITS.json` records
bubblewrap identity and receipt limitations. One optional mutation harness
stopped on a selection exception; it is retained without a mutation-score claim.

These are bounded offscreen correctness results while the desktop remained
alive. They establish neither performance nor exclusive GPU ownership. The
desktop still opens both render nodes; the runner lock coordinates cooperating
test runners only. Libraries are read-only but not frozen, and the reviewed
test can write its artifacts, including the admission file. This qualification
does not clear t306/t307 or establish the independent visible Session in t312.

## t312

An independent visible native development Session needs more than output
profile exclusion. Current scanout selection enumerates all primary nodes;
construction compares owned heads with global sysfs connectors. The normal
launcher changes VT modes and native --no-input is refused. Render inventory
already has udev seat filtering, but scanout and connector projection do not
share that admission boundary.

Define one admitted device set for primary/render nodes, connectors, initial
ownership, hotplug and replacement. Scope completeness checks to that set,
without ignoring a failed admitted device. Prove the daily session cannot
open the development primary card and the development session cannot open
the daily card, input or VT. Specify independent seat lifecycle, automated
input and cleanup before implementing a launcher. Do not bypass seat admission
or the existing recovery guard merely to make a second process start.

Exit: two visible sessions run concurrently; development start, bounded exit,
crash recovery and admitted-head changes leave the daily desktop presenting
and taking input. Device/seat configuration and any one-time daily relogin are
prepared for review before application. This remains open after t311 lands.

### First source slice: seat-scoped DRM discovery

Primary selection and render inventory now share a bounded udev seat-card
inventory. Missing `ID_SEAT` means seat0 only on an initialized record. The
seat comes from the opened libseat controller. An output profile or an
`--input-seat` argument does not grant ownership of another seat's cards.
The inventory is collected before opening nodes. A seated primary open must
preserve the admitted node's device number, inode, filesystem, physical path
and seat assignment before any KMS query uses that descriptor. An admitted
open failure refuses the selection instead of falling back to another card.

The seated scanout constructor reads connector facts only for those admitted
cards, including its completeness check. Connector IDs are matched together
with their card because their numeric values may overlap between GPUs. Its
image-import render devices come from the same card inventory. Startup,
topology replacement and seat reacquisition use this constructor. Standalone
card probes retain their explicit, host-wide discovery route.

CPU controls cover foreign and uninitialized card exclusion, failed admitted
discovery, capacity and identity aliases, returned-descriptor identity,
foreign-connector exclusion before reading its facts, connector-ID collisions
between cards, and refusal to fall back after an admitted open fails. Evidence
and review for this slice live in `development-evidence/t312-seat-admission-01`.
This is source preparation; physical seat assignment and concurrent visible
sessions have not been qualified.

Both GPUs are still assigned to seat0 on this host. This change alone therefore
does not release the iGPU from the daily desktop. Remaining work is to scope
topology notifications and recovery to admitted devices, qualify an independent
seat without taking the daily VT or input, and prepare a private development
launcher with bounded crash cleanup. Any host configuration and daily relogin
must follow the prepared review, while the daily desktop remains protected.

## Evidence

- `igpu-development-01/FINDINGS-01.txt` and `READ-ONLY-01.json`: source survey,
  current desktop descriptors and seat identity. No device changes.
- [Login refusal and profile repair](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#subsequent-hardware-change-and-login-failure).
- [Validation contract](../../validation.md): device tests remain separate
  from CPU gates and do not establish native presentation timing.

Task status lives in [todo.md](../../../todo.md).
