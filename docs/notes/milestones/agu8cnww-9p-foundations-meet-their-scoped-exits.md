---
id: agu8cnww
date: 2026-09-26
kind: milestone
status: recorded
tags: [milestone]
---
# 9P foundations meet their scoped exits

## Decision and evidence

The September 26 reconciliation satisfies the t247 shared-core and t248
typed-adapter exits in the [Hagia-first plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md).
It does not accept t249, a mounted filesystem, a daily default or another role.

The shared `.L` core, directory enumeration and read-only client at signed
`81e28524214f19465e7c4fc05ec9440e02b5e0ea` and
`80416141665dbcc8ae2fa5e8aa304a83310d5367` retain version/framing/msize,
tag/fid/queue, walk/open, authorization, cancellation and teardown controls.
The pinned third-party Go client covers ordinary file operations. It has no
Tflush API; the independent raw prober supplies flush evidence instead.

Signed follow-up `2f9c222096d8ec9dd9d117a020a9a36129601901`, integrated as
`7ce5d61eb`, fills the independent-prober gaps: strings extending past frames,
invalid access/flags, an occupied newfid, fid and waiting-read exhaustion,
and disconnect with held fids and a pending read. A later ordered getattr
reply proves the read was processed before disconnect. The static test
export's release ledger proves four fids and two open handles were released;
closing the client's socket alone is not treated as cleanup evidence.
The ledger is test-export data, not a new production role interface.

The committed-tree oracle passes all 50 checks. Ten compiled mutations each
fail their required named controls, including isolated limit/release mutations.
Core and xtask tests, strict Clippy, Go vet, layout and formatting pass.
Evidence is retained under `sophia-t247/.artifacts/9p-conformance/` at
`2f9c2220-1790434961` and `2f9c2220-self-test-1790434962`; the source bundle
SHA256 is `b504c854a09ac52387a6ba54808b78d0bfcdc431099090f23e99289fed9cf73b`.

The [typed-adapter reconciliation](../investigations/uf2wya88-typed-wm-driver-preserves-current-ipc-phase-and-shutdown-ownership.md#september-26-extraction-exit-reconciliation)
maps its signed source, nine focused controls, 620-test native Session run,
strict checks and corrected fresh layout check to t248. Later permit and
profile controls retain current-IPC semantics without changing output IPC.

## Remaining boundary

These are direct-socket, affected-owner development checks. The WM role still
needs its remaining lifecycle and reproducible measurement evidence, followed
by the separately scoped daily configuration. Shell contract work includes
Lom, Bemenu, Provlita and Narthex but gains no acceptance from these closures.
The [desktop migration plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
continues to own that sequence; task state remains in the queue.
