---
id: o277q5cu
date: 2026-09-24
kind: investigation
status: closed
tags: [investigation]
---
# One isolated native family gate retains independent clients and owner lifecycles

## Question

How can [t023](../plans/queue-09-cp-15-2-one-family-level-conformance-surface.md)
make one family check reproducible without discarding the independent proofs?

## Existing evidence and change

`tools/check_native_protocol_family.sh` already invoked the historical WM,
shell and output checks. Its output still said output lacked a schema, and its
hand-picked runtime target list omitted later component, grant, fairness and
retirement cases. It provided no common device hiding, configuration isolation,
deadline or source-bound phase report. The implementation now delegates to
`cargo xtask check native-protocol-family`; the existing role scripts and
independent C/Nim clients remain intact.

The Rust runner preserves required Hagia/Narthex checkouts, archived WM r3
checksums and full lifecycle clients. It runs all protocol/runtime and Engine
integration targets plus output live-owner/client and the separate control
service checks. Fresh pseudodevices and private runtime/config/temp directories
prevent an inherited desktop from granting hardware access. Nested shell hosts
still enforce their protected admission. The WM script now locates the binary
in the selected Cargo target directory, preventing stale default-target reuse.

The report binds Sophia, Hagia and Narthex commit/diff identities, keeps each
phase's log and rejects source changes during the run as NORESULT. Missing
dependencies, any failed phase and the whole-run deadline fail closed.

## Validation and remaining gate

Clippy passes for xtask all targets. A deliberate one-second deadline stopped
the WM phase with exit 124 and a failing report after the isolation phase
passed (`.artifacts/t023-timeout-2`). The first trial caught a Bash-only script
being invoked through sh; selecting Bash fixed that orchestration error.
The complete retained family run remains the gate for this candidate.

The first complete run (`69eafa1f`, `.artifacts/t023-full-1`) reported PASS,
but manual count review invalidated it: the inherited output-owner command ran
zero tests without `native-session`. The corrected runner explicitly enables
that feature and refuses every empty or ignored-only Cargo phase. A regression
test pins the rejection. The old report is retained as evidence of the defect,
not accepted as task completion; the corrected full run must replace it.

Only WM r3 is stable, and its immutable client remains mandatory. Experimental
output has schema/codec/owner tests but no independent full-lifecycle client;
this change does not declare output stable. C descriptor/launcher and Nim
Narthex proofs are retained independently of generated bindings or Sophia crates.
Hosts supply topology, activation and presentation-completion facts. This is
not native display/input, GPU-permission or installed-session acceptance.

See the [conformance guide](../../native-protocol-conformance.md) for invocation
and evidence classification, and the [t022 audit](v2yrv8je-native-family-audit-makes-output-layouts-explicit-without-changing-frozen-wm-bytes.md)
for the schema and lifecycle contracts this entry checks.

## Accepted run

Candidate `60fb80e9f473954f9513b09c051c03120d272a62` passes every phase in
`.artifacts/t023-full-2`, with clean immutable Hagia `50336ce6` and Narthex
`50b9014d` clones. Retained evidence is copied to
`~/.local/state/sophia/development-evidence/native-family-60fb80e9`.
The corrected output owner runs nine tests. Protocol/runtime runs 437,
Engine 422, output client eight and control service 23, all passing; C/Nim
clients and immutable archived WM lifecycle/reconnect proofs also pass.
The WM matrix's one existing ignored native test is explicitly separate from
the mandatory executed independent-client proofs. Clippy, fmt, layout and
diff checks pass. The zero-test regression and deadline negative control pass.
This meets t023's deterministic exits without declaring experimental roles
stable or claiming physical acceptance.
