---
id: q2rdcytg
date: 2026-10-11
kind: investigation
status: awaiting-physical-acceptance
tags: [investigation, x11, randr]
---
# RANDR output property discovery must agree with metadata and values

## Question

An ordinary graphical client's output discovery received `BadRequest` for
RANDR major 132, minor 10 (`RRListOutputProperties`). Can the X authority
provide accurate property discovery without a client-side RANDR disable hint?

The decoder advertised RANDR but had no minor-10 branch. Its unknown-opcode
fallback produced the reported error. `RRQueryOutputProperty` was missing too;
the existing value getter synthesized `non-desktop` as `CARDINAL`, although
the property is `INTEGER/32`. Interned conventional property names were not
evidence that an output actually had those properties.

## Evidence

Implementation commit: `f8426a2e0`, based on `e855fd75a`. This is a separate
X-authority repair; no authenticated-attach work is included. The gate captured
the complete modified/untracked file set dynamically, and every changed crate
file was compared with that capture before the signed implementation commit.

Packages are under `~/.local/state/sophia/development-evidence/`:

| Package | Result | SHA-256 of its self-excluding SHA256SUMS |
| --- | --- | --- |
| `randr-properties-control-01` | Expected exit 101, one named failure | `3edd7a3f50c1820c2e6ded54e1e4d7cfbcfe2427badfc6f4c6e9b055dae82598` |
| `randr-properties-short-serial-gate-01` | Full `cargo xtask check` passed | `d52161a6457edb76b94e14c554966397f884b4d4d0e766f0bf07c87c48aa0c31` |

The passing run has 548 Rust test-result summaries: 7,632 passed, zero failed,
105 ignored. It also passed the SDK checks, workspace Clippy and the remaining
repository checks. All eight new RANDR cases executed, including the independent
C/XCB peer. The peer uses XCB's own wire codec, not Sophia's helper functions.
Its development prerequisites are `/usr/bin/cc`, libxcb and libxcb-randr;
the reviewed host libraries were both version 1.17.0.

The decoder control moves only the minor-10 match arm to unused minor 254.
Enumeration then fails at **RANDR property enumeration must return a reply**,
before metadata or value assertions. The source and complete fixture set were
verified restored. No production mutation is retained.

Three preceding gate attempts remain retained, rather than relabelled:

- `randr-properties-gate-01`: an existing shell-content fixture exceeded the
  Unix socket path limit beneath the long worktree Cargo slot, before owner work.
- `randr-properties-short-gate-01`: workspace tests passed, but the host compiler
  cache could not write SDK build output through a mount-namespace-only alias.
  Cargo also reused fixtures with the old temporary path embedded; that attempt
  does not establish that the alias corrected their compiled paths.
- `randr-properties-short-rebuilt-gate-01`: the four fixtures referencing
  `CARGO_TARGET_TMPDIR` were rebuilt without changing their bytes. An existing
  Session socket-lifecycle test intermittently still observed its listener as
  active after dropping it. The exact test then passed alone. This investigation
  does not establish the cause of that intermittent failure.

The final invocation held the same worktree's cargo-slot lease throughout,
bind-mounted its root at `/tmp/srr-target`, disabled the compiler-cache wrapper,
and used `RUST_TEST_THREADS=1`. The compiled shell-content fixture was checked
to contain the short temporary path. No second target directory was invented.
Display authorization and `SOPHIA_*` opt-in variables were removed; `/dev` was
private. Internally concurrent protocol tests still ran their own concurrency.
The final source capture/restoration check passed. `randr-properties-binding-01`
records the logical-to-host target mapping and selected executed binary hashes;
build binaries are not copied into evidence.

## Finding and resolution

The X authority owns a bounded, immutable output-property definition:

```text
List / Query / Get
        |
        v
current exposed output? ---- no ---> RANDR BadOutput
        |
        v
one property definition ---> atoms / metadata / bounded value reply
```

Only `non-desktop` is currently listed: `INTEGER/32`, one zero value, immutable,
range 0–1, no pending value. `EDID` and other interned names are not fabricated.
Valid but absent metadata returns `BadName`; absent value reads return type
`None`. Invalid atoms return `BadAtom`. Immutable deletion returns `BadAccess`.
Offsets, lengths and type mismatches retain bounded read semantics, with the
invalid offset correlated in a `BadValue` error. Invalid Boolean fields and
malformed lengths are refused before dispatch.

RANDR reserves its four 1.5 error codes immediately after GLX's complete range
(184–187). The first error is advertised and property requests emit `BadOutput`
at offset zero. The range test prevents overlap, including unused GLX errors.
No Engine, WM, 9P, mode-setting or application-specific behavior is added.

## Validation and remaining work

Deterministic coverage includes both byte orders, header/payload counts and
padding, repeat reads without topology changes, invalid/removed outputs,
invalid atoms, absent properties, offset/length extremes, type mismatch,
pending reads, deletion refusal, malformed lengths and Boolean values.
Served socket tests and the independent peer continue successfully after a
correlated invalid-output error. No live display was contacted.

### Acceptance

t325 requires an installed candidate's ordinary graphical startup and rendering
with RANDR enabled and no RANDR-disable workaround. Candidate preparation and
installation are separate from this deterministic result. Do not stop an
existing client or change a private launcher to perform that acceptance.
General RANDR conformance, writable properties, EDID supply, native mode
management, unrelated client-backend initialization and package startup faults
remain outside this repair.

## Connections

- [X-authority contract](../../sophia-x-authority.md): the read-only property
  boundary and independent-client prerequisites.
- [RANDR protocol](https://www.x.org/releases/current/doc/randrproto/randrproto.txt):
  List/Query/Get request and reply layouts and error semantics.
- The installed xorgproto `randrproto.txt` specifies `non-desktop` as immutable
  INTEGER/32 with range 0–1; X server `randr/rrproperty.c` also refuses deletion
  through `GetOutputProperty` when the property is immutable.
