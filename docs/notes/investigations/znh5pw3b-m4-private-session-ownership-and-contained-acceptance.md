---
id: znh5pw3b
date: 2026-09-18
kind: investigation
status: investigating
tags: [session, x11, security, validation]
---
# M4 private Session ownership and contained acceptance

M4 exposes the private ordered service through Session. The question is whether
that caller preserves M3's admission, execution and collection boundaries while
driving real Engine commits. The [t093 plan](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md#m4-execution-contract)
owns the exit criteria. This note records implementation findings and their
evidence; it does not close t093 or enable XTEST.

## Contained evidence

The harness archives signed source, builds inside the namespace boundary and
records exact executable hashes. Every source snapshot uses a fresh contained
target. Host-side Clippy and compilation are not behavioral acceptance. Builds
run with two jobs at reduced CPU and I/O priority.

Evidence directories below live beneath the root checkout's `.artifacts/`.
The `m4-c0acc0f2-source` name belongs to a reusable snapshot checkout; each
report's full source identity, rather than that directory name, identifies its
run. Completed evidence directories are retained under their original names.

| Source | Evidence | Result and scope |
| --- | --- | --- |
| `8901fedd` | `m4-8901fedd-source/.artifacts/self-test-01` | Ten harness assertions passed, but the run failed collection: a nested namespace monitor escaped its immediate owner. Not acceptance. |
| `c216480c` | `m4-harness-self-test-02` | Ten harness tests passed after the Rust launcher acquired and collected its nested monitor. M4 remained `NOT_RUN`, 0/8. |
| `c0acc0f2` | `m4-c0acc0f2-source/.artifacts/m3-harness-regression` | Twenty-four M3 harness tests passed. This was not a rerun of M3's twenty acceptance rows. |
| `4e66989d` | `m4-c0acc0f2-source/.artifacts/acceptance-4e66989d` | Failed compilation: the independent host and fixtures lacked the new explicit `session_generation`. No behavioral result. |
| `e577b519` | `m4-c0acc0f2-source/.artifacts/acceptance-e577b519` | Build succeeded. Construction and authorization both failed at actual X setup. The source had created an empty namespace registry, then tried to admit into a namespace it had never registered. |
| `47dd3c6a` | `m4-c0acc0f2-source/.artifacts/acceptance-47dd3c6a` | Aggregate failed, 2/8 passed: authorization in both byte orders and connection identity. Construction found live listener displacement. Two nested-host fixtures also failed; those failures are separate from the listener defect. |
| `30035134` | `m4-c0acc0f2-source/.artifacts/harness-30035134` | Ten runner controls passed, including kernel namespace and delegated-descriptor checks. Acceptance remained `NOT_RUN`, 0/8. |
| `652208c4` | `m4-c0acc0f2-source/.artifacts/acceptance-652208c4` | Six groups passed together with no failed rows: construction, authorization, connection identity, containment, no ambient fallback and evidence integrity. Commit routing and lifetime were unbound; the aggregate remained `NOT_RUN`. Source was attested inside containment and unchanged afterward. |
| `34fac795` | `m4-c0acc0f2-source/.artifacts/acceptance-34fac795` | The same six groups passed. The newly bound commit-routing control failed waiting for a committed admission; lifetime remained unbound. The aggregate is `FAIL`, with the exact source attested and unchanged. |

## Findings

**Namespace admission.** `NamespaceRegistry::new` creates an empty namespace
table. Session initially supplied a configured ID to `admit` without installing
its context. `with_namespace` now installs the explicit context and seeds future
namespace allocation past it. The passing authorization and connection rows on
`47dd3c6a` exercise that repair through real sockets.

**Listener ownership.** The construction control starts a second service at the
first service's live path. On `47dd3c6a`, both report Ready. The common bind helper
unlinks any existing socket inode, treating its type as proof of staleness. A live
listener therefore loses its pathname. The private path needs an exclusive bind
that never unlinks an existing path; a connect probe would still race, and a
sidecar lock would not establish ownership over listeners that do not use it.
`2ce8dc4b` adds that exclusive private bind. The integrated control on `652208c4`
requires the occupied-path failure, then connects to the original listener
again. It passed; the legacy reclaiming entry is a separate API and unchanged.

**Commit ordering and custody.** Source review found that the first Session
bridge combined mapping facts from several batches, then used the final Engine
snapshot to describe earlier commits. It also missed removal-only batches and
dropped refused effects. The bridge must retain each intake and committed effect
in order, capture geometry from that commit, and keep the exact command through
refusal. Later effects must not overtake a refused head. These are source
findings; a compiling rewrite is not their acceptance evidence.

The first integrated draw exposed a second mapping error. MapWindow publishes
its lifecycle fact in one batch; a subsequent drawing response carries its
transaction without repeating that fact. Rebuilding the mapped set from each
batch independently forgets the earlier map and suppresses the committed draw's
admission. The bridge needs ordered lifecycle state for the same surface and
admission, retired by withdrawal/removal. This does not authorize using a later
map for an earlier effect or inferring mapping from Engine snapshot presence.
The next wire fixture also waits for a real GetGeometry reply and preserves
nonempty commit reports, distinguishing malformed requests from a missing join.

**Collection and observation.** Readiness must come from the prepared service's
port, and status reads must not drive recovery. An unwind needs its actual
custody evidence; an empty worker list cannot substitute for a report that was
never returned. A keeper reading taken before its Drop does not describe its
post-join availability. Dropping the controller must still join when adapters
retain submission handles. The lifetime row remains unbound while these facts
are joined and tested.

**Nested host fixtures.** The failed fixture attempted X setup from outside the
host's nested PID namespace and received a connection reset. It did not establish
a defect in the service's input path. The host-entry controls now exercise the
explicitly delegated stop pipe, real readiness and collection without claiming
an X peer inside that nested host. The public Session controls separately use
real X peers in their contained scope. Readiness has its own private artifact;
socket-path creation alone is insufficient because it precedes preparation.

**Exact checkpoints.** `605285ca` was committed without all dependent Session
changes. Its descendant `30035134` failed an immutable snapshot compilation with
missing bridge/config/outcome fields. A Clippy result against a working tree
cannot qualify an incomplete signed checkpoint. The failure remains distinct
from the preceding successful five-row build.

## Acceptance boundary

M4 has eight required groups. The evidence-integrity group checks deliberate
changes to a preceding real Session result and its actual executable identities;
it does not claim new Session behavior. Session lifetime tests use a separately
attested library test binary for labelled test-only faults. Other rows use the
public integration target. Missing rows stay `NOT_RUN`; a failed row keeps the
aggregate failed.

The final source still requires the full M4 gate, the separate M3 twenty-row
gate, relevant negative controls and repository validation. No M4 result here
claims physical input, a native display, XTEST discovery, or t094 completion.
