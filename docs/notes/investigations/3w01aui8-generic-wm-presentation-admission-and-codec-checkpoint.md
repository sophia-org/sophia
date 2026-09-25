---
id: 3w01aui8
date: 2026-09-25
kind: investigation
status: resolved
tags: [investigation, rendering, protocol]
---
# Generic WM presentation admission and codec checkpoint

## Question

Can the WM propose bounded source instances without acquiring renderer or input
authority, while keeping old revision-3 clients interoperable?

## Evidence

Signed source checkpoint `89f5edf496e69b8665049244ecb8552d66eb2502` on
`rendering/foundation` implements the t243 protocol/admission slice over the
passive contract checkpoint `39f2f447`. It is an isolated buildable candidate,
not an integrated feature or a main-tree acceptance claim.

Checksummed logs are retained at
`~/.local/state/sophia/development-evidence/t243-89f5edf4/`: `protocol-tests.log`,
`policy-protocol.log`, `clippy.log` and `manifest.json`.

## Finding and resolution

Capability bits 18/19 admit visual records and reduced presentation actions.
Five bounded extension records follow the frozen ordinary projection prefix.
Messages 54/55 carry exact action identities and actual presentation receipts.
Malformed counts, reserved bytes, unsupported capabilities, duplicate identities,
invalid geometry and incomplete sets fail before the policy reducer commits.

Reducer admission requires current opaque sources and output generations,
affected-output coverage, increasing publication/target generations and no
recycling of retired target ids. A preview-only source remains authorized while
absent from ordinary placements. Source content updates preserve target identity.
Local revocation invalidates already-staged successors through the existing
commit serial without consuming transport credit. Session admission checks each
action against the registered pure-policy catalog; session operations are refused.

Neither reducer admission nor projection acknowledgement establishes actual
presentation. The session's presented-input owner must validate completion,
presentation epoch, capture and action delivery. The new transport receipt API
does not itself issue receipts or install input authority.

## Validation and remaining work

Workspace all-target check and strict protocol/engine/runtime all-target Clippy
passed, with niced builds and two jobs. Focused suites passed: Engine policy
29/29, presentation codecs 6/6, protocol wire 6/6 and runtime transfer 15/15.
The complete `tools/check_policy_protocol.sh` gate passed, including generated
schema agreement, independent C fixed-record checks, current C/Rust clients and
the immutable archived revision-3 client across reconnect/restart scenarios.

Hagia's independent Nim codec and paired admission remain t243/h002 work. Renderer
regions/replacement and source retirement remain t244; actual presented input and
session lifecycle remain t245. The complete Hagia overview remains t241/h002.
No live installation, reload, GPU device acquisition or physical acceptance was
performed. The inherited t220 layout overflows are being repaired separately as
signed `c528f4b1`; they are unchanged by this protocol checkpoint.

## Joined acceptance

The checkpoint's remaining joins are implemented in signed Sophia `6251aa79`
and Hagia `12d3142`. The complete device-hidden native-protocol-family gate
passes all eight phases, including independent Nim/C/Rust wire and real-Hagia
controls. The [paired acceptance record](ufhp04gq-workspace-overview-joins-policy-presentation-and-modal-input.md#final-joined-source)
owns final evidence and physical limits; the foundation plan records t243's exit.

## Connections

- [Contract](../../wm-presentation.md): authoritative record and lifecycle rules.
- [Plan](../plans/mjnpxubs-generic-wm-presentation-foundation-and-input-contract.md):
  admitted implementation scopes and acceptance exits.
- [Prototype investigation](egmb00jq-rendering-foundation-inventory-and-overview-ownership-correction.md):
  preserved feature-specific experiment and production negative controls.
