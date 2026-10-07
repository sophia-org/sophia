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

## Evidence

- `igpu-development-01/FINDINGS-01.txt` and `READ-ONLY-01.json`: source survey,
  current desktop descriptors and seat identity. No device changes.
- [Login refusal and profile repair](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#subsequent-hardware-change-and-login-failure).
- [Validation contract](../../validation.md): device tests remain separate
  from CPU gates and do not establish native presentation timing.

Task status lives in [todo.md](../../../todo.md).
