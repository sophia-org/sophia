# Role coverage plan: t252 B7/B8

Base: `db64c4d8b`, updated through `ae60576f8`. Role field/value rules are pinned
to KDL commits `ee5e7f809` and `ae60576f8`; the latter also corrects owner outcomes.
The director owns the task and milestone note. Codec layouts
and validation come only from the KDL; the admitted shell file document supplies
lifecycle expectations. Each language has separately written code and vectors.

## Phase 1: independent codecs

Go: add `internal/shelloracle/records_{catalog,indicators,native,role_candidates}.go`
and `records_roles_test.go`; extend dispatch in `records.go`. Cover the 17 new
record kinds (two objects, eight events, seven candidates), including all rows.
Keep the live verdict at 54 until the runtime scenarios actually execute.

C: add `sophia_shell_files_roles.h` and `shell_files/{catalog,indicators,native,
role_candidates,text}.c`, extend native record dispatch and validation, and add
`tests/sophia_shell_files_roles_test.c`. Large objects use caller-owned buffers
and borrowed row views with typed accessors; no multi-megabyte record union or
hidden allocation. Native fixed records and bounded candidates use typed values.
Preserve unchanged-on-error encoding/decoding and explicit buffer lifetimes.

Tests: one independently authored valid vector per new kind in each language,
plus zero/maximum row counts, every truncation boundary, unknown kinds, reserved
bytes, oversized counts, malformed/padded text, duplicate slots, identities,
conditional geometry/state rules and encode/decode round trips. The Go decoder
also validates outgoing extended records before an oracle submits them.

## Phase 2: live role coverage (runtime hash and plan approval required)

Extend the oracle to **96 named checks: the existing 54 plus 42 below**. Update
both verdict producers and the independent Rust exact-name parser together.
Setup failures, skipped cases and unknown names can never produce a pass.
The [fixture sequences](ROLE-FIXTURES.md) specify requests, barriers, expected
events and negative controls for each name, including validation precedence.

| IDs | Names and expectations |
| --- | --- |
| 55–64 | `r6/profile`, `r6/indicators`, `r6/announcement`, `r6/qid`, `r6/second-pin`, `r6/old-pin`, `r6/fresh-generation`, `r6/activation-custody`, `r6/activation-echo`, `r6/stale-activation` |
| 65–84 | `r7/profile`, `r7/catalog`, `r7/opening`, `r7/allocation`, `r7/permit`, `r7/candidate-custody`, `r7/prepared`, `r7/no-focus-before-presented`, `r7/presented`, `r7/focus-binding`, `r7/text-input`, `r7/input-ack`, `r7/query-disarms`, `r7/repaint-focus`, `r7/accept-input`, `r7/activation-custody`, `r7/activation-outcome`, `r7/stale-input-ack`, `r7/focus-revoked`, `r7/closed` |
| 85–96 | `r8/profile`, `r8/catalog-identities`, `r8/catalog-old-pin`, `r8/catalog-fresh-generation`, `r8/allocation`, `r8/permit`, `r8/candidate-custody`, `r8/presented`, `r8/activation-custody`, `r8/activation-echo`, `r8/stale-generation`, `r8/stale-slot` |

Add `scenarios_{indicators,native,catalog}.go`; extend session object reads to
their KDL caps and add role-specific offers/expected api profiles. Add Rust
fixture support by role under `tests/support/shell_files_oracle/`, driving real
catalog, native input/focus, allocation, resources, candidates and indicator
owners through their public APIs. Start with three role fixtures; use separate
fixtures for negative cases whose normative result revokes an epoch. Scheduling
barriers separate Prepared from Presented and input from repaint. The harness
revokes on owner errors, verifies typed activation/ack echoes and checks cleanup.

C phase 2: extend `shell_files/{client,events,objects}.c` and the public session
header for role negotiation, large caller-owned snapshot storage, native events
and typed submissions. Add a C r7 peer and a new runtime integration test, plus
an r8 peer for persistent catalog/candidate/activation. Both use real exports,
discover epoch from api and exercise the asynchronous event/submit path. No
Bemenu repository changes or adoption claim. C indicators receive codecs in
phase 1; their live role coverage belongs to the Go oracle in this slice.

## Validation layers and runtime dependency

The normative KDL now supplies text/identity rules, all 17 input kinds, revision
relations, allocation geometry and candidate rules. The admitted lifecycle
document supplies role negotiation and activation/ack outcomes.

Candidate byte decoders enforce field bounds and row layouts. Normal record
decoding also runs value validation: NativeCandidate requires one surface,
nonempty placements, equal target/row counts, unique displayed slots and a
selection from those slots (zero exactly when there are no rows). File submit
refuses value violations with EINVAL and journals nothing. CatalogCandidate's
one-surface/nonempty-placement checks belong to its owner. Current opening,
catalog membership, target uniqueness and overlap also belong to owners.
Separate byte entry points let negative controls construct records without
conflating these failure paths.

`ae60576f8` resolves owner failures: wrong/missing permits transfer Submitted
custody then revoke without an outcome; stale opening/catalog/state yields
Rejected/reason 1; malformed owner geometry, target identities or unavailable
slots yield Rejected/reason 3. The negative-case hold is lifted. Phase 2 still
awaits the B5 runtime handoff. Indicator outcomes follow that commit's corrected
linked-admission rules. No phase-1 test claims a live owner outcome.

## Gates and handoff

Offline Go test/vet and unchanged C1 build, native and existing C gates, new
runtime tests, fmt, targeted Clippy and layout. Nice 19, jobs 2, private targets
and caches; **no builds or tests 00:45–04:00 local**. Separate signed phase-1
codec commits from phase-2 client/scenario commits. Send hashes and results to
the director for review/merge and task-note updates. No authentication, native
rendering, latency or attended-session claim.
