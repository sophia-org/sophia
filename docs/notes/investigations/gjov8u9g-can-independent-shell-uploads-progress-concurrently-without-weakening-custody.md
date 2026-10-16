---
id: gjov8u9g
date: 2026-10-15
kind: investigation
status: investigating
tags: [investigation, 9p, performance]
---
# Can independent shell uploads progress concurrently without weakening custody

## Question

Can independently admitted shell uploads progress concurrently, within today's
byte, tag and resource limits, and reduce owner passes or CPU/MiB without changing
custody, ordering or control responsiveness?

niltempus requested this investigation during t331. It is a candidate for the
optimization programme, not authorization to change upload scheduling or a new
t252 prerequisite. First measure the allocation and copying fixes in t331.

## Evidence

The `t331-owner-profile-06` diagnostic on comparator `e45744623` observes about
4.1 times as many owner passes for files on the panel workload and 2.6 times as
many for panel-plus-4-MiB uploads. This does not prove serialization accounts
for the gap. Native service also repeats work on every pass; t331 addresses
those allocations separately. See the [profile results](lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#t331-finer-profile-results).

At SDK `25b63ff`, the 9P pipeline admits multiple tagged requests, but the shell
file wire keeps one `Current` outbound unit. Its outbox is FIFO, one active slot
write waits for its reply, and partial success advances by the returned count.
Resource status admits the slot before walk and open; transaction reuse waits
for the covering `Submitted` acknowledgement. These are separate dependencies.
More tags do not make dependent operations safe to overlap.

Upstream source reads on October 16 provide comparison points, not Sophia
performance evidence:

- [Go p9 client at 3fd948847](https://github.com/hugelgupf/p9/blob/3fd948847ea46b413f8b98c8f226c6bb716807be/p9/client.go)
  dispatches replies by tag and pools response records. Its
  [transport](https://github.com/hugelgupf/p9/blob/3fd948847ea46b413f8b98c8f226c6bb716807be/p9/transport.go)
  sends separate header and payload vectors.
- [Linux client at 2f2f7eeef](https://github.com/torvalds/linux/blob/2f2f7eeef8b0a1a9b9ca998231419873ccae8d4e/net/9p/client.c)
  has a transport-dependent zero-copy path; its ordinary write loop still
  advances after each reply. Concurrency is not a universal same-file policy.
- [QEMU at 7e8d3c7fc](https://github.com/qemu/qemu/blob/7e8d3c7fc460fe7cd28a3e423d7466fa292bc269/hw/9pfs/9p.c)
  reuses PDU records and works with payload vectors. Its coroutine model is not
  a proposal for Sophia's owner loop.

Source hashes are retained in
`~/.local/state/sophia/development-evidence/t331-storage-20261016/upstream-sources.json`.

## Finding and resolution

There is an implementation choice to investigate, not yet a demonstrated
architectural fault. Compare independent resources with dependent writes to a
single resource. A proposal must state which FIFO barriers remain, which
requests may pass them, and how every ticket settles when replies arrive in a
different order. Do not assume larger windows or additional threads are needed.

### t332 — Investigate bounded upload concurrency

Use t331's measured candidate as the baseline. Trace request dependencies and
count owner turns, requests, syscalls and copied bytes for the existing panel
and bulk workloads. Separate transport waits from repeated owner work.

Describe a bounded experiment for independent admitted resources, preserving
same-resource ordering and transaction/ack barriers. Cover partial writes,
EAGAIN backoff, tag/flush reserves, revocation between requests, terminal status,
late replies, disconnect classification and fairness to events and controls.
Do not implement a production scheduler under this investigation.

The exit is a sourced recommendation: either retain serialization, or propose
an exact concurrency rule, custody model, unchanged limits, regression matrix
and matched performance experiment. Report remaining unknowns and no-change
results. Any implementation needs explicit promotion and its own reviewed scope.

## Validation and remaining work

No concurrency experiment or native benefit has been established. Keep the
candidate's status in `todo.md`; t331 and t252 retain their existing gates.

## Connections

- [t331 implementation plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md): baseline and preservation constraints.
- [Optimization programme](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md): related future work, without changing t289's scope.
- [Shell contract](../../sophia-shell-files.md): authoritative budgets, ordering and resource lifetime.
