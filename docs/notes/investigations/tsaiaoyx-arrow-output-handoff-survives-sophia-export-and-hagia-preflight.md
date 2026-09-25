---
id: tsaiaoyx
date: 2026-09-24
kind: investigation
status: closed
tags: [investigation, policy, validation]
---
# Arrow output handoff survives Sophia export and Hagia preflight

## Question

Does Hagia's configurable arrow handoff survive Sophia's desktop envelope,
policy export and launch preflight without a new Sophia schema?

## Evidence

Hagia implementation: signed commit
`ad3a738d7aee2af4ab31f7e7139d1ceecdc7dcbe`, verified signature G and clean tree.
The provided binary's verified SHA256 is
`991cc705e0c5ac540df737ae52cb7c43a1ffcb350e11c76acb836c92abec6c0c`.
Sophia acceptance source: clean signed
`c162ea1cc9f58d977a56bf91117aede6388f3f0e` on `session-launch/t027-rust`.
The existing Bash adapter was also extracted from master `a4f9c231` and tested;
t080 does not require the unmerged Rust-preflight refactor.

Durable evidence is under
`~/.local/state/sophia/development-evidence/t080-paired-ad3a738d/`.
`paired/paired-report.json` records all 20 checks with expected decisions and
exit codes. Fixtures, exports, individual logs and binary hashes are retained.
`hagia/` retains the peer's model, foundation and build logs and investigation
`2a8wjk9n-sophia-t080-configurable-arrow-output-handoff.md`.

## Finding and resolution

No Sophia schema change is needed: ordered WM-owned policy records already
preserve the boolean. Sophia validates the envelope; Hagia owns vocabulary,
value types and duplicate identities. True, false and omission pass export
validation and launch preflight. A string value and a duplicate setting pass
Sophia's envelope but are correctly rejected by Hagia and both launcher paths.

Hagia's guard covers column-edge and empty-output directional handoff. Opt-out
retains local navigation and explicit output switching. Checkpoint version 19
persists the preference; supported older checkpoints restore true. Reloads
retain focus and workspaces. These behavior checks are owned by Hagia's
navigation and model suites, rather than inferred from configuration acceptance.

## Validation and remaining work

Direct paired acceptance passed all five fixtures through each of four paths:
export-to-Hagia, Rust preflight, existing master Bash adapter, and candidate
Bash adapter. Tests hid device nodes and live display/runtime endpoints and
used private configuration directories. Hagia reports navigation 13, model 180,
and foundation 46 passing; its retained logs distinguish that peer validation
from the paired checks run here.

The full native-family gate passed all eight phases: isolation, independent WM,
independent shell, protocol/runtime, Engine owners, live output owner, output
client, and control service. `native-family/report.json` and per-phase logs
retain the result and clean source identities, including Narthex
`50b9014d96f675f515b5e092c071427fb8e34423`. The independent WM corpus completed
all 11 behavior scenarios. The gate ran in the worktree with its own Cargo
target directory, an immutable Hagia clone and hidden devices.

These checks close t080's deterministic acceptance. No live installation,
reload or physical acceptance is claimed. Lom content-shell acceptance remains
t099; the optional Lom client was not supplied to this gate.

## Connections

The [t080 plan](../plans/queue-11-parallel-production-readiness.md#t080) owns
acceptance criteria; [configuration](../../configuration.md) documents the
setting and WM validation boundary. The separate
[t027 migration](../plans/queue-11-parallel-production-readiness.md#t027)
remains open; testing its candidate does not close t027.
