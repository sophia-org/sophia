# Complementary Architecture Models

**Role:** bounded architecture-decision gate, not an implementation refinement
proof or a claim of unbounded security.

These models give different questions to different solvers:

- TLA+ under `validation/tla` owns temporal ordering, epochs, presentation,
  capture cancellation, pacing, and progress assumptions.
- Alloy owns bounded relational topology: role admission, namespace and portal
  authority, protection-domain role composition, issuer-scoped actions,
  visible target ownership, trust precedence, identity uniqueness, and
  independently issued coordinate grants.
- Z3 owns arithmetic obligations: region containment and clipping,
  quantization and rate/count budgets, and current `sophia_wm_v1` byte/count
  bounds.

There is deliberately no TLA+/Alloy/SMT translator or shared modeling DSL.
Each small model is hand-maintained beside an explicit correspondence map. A
passing result means only that the stated predicates hold within the checked
scope or formula. It does not show that the Rust implementation refines the
model, that a malicious authorized shell reveals no information, or that
unmodeled concurrency is safe.

## Models and correspondence

| Model | Question | Current or target boundary |
| --- | --- | --- |
| `AuthorityTopology.als` | Can an access cross a role or namespace without exact admission or an independently issued portal grant? Can a WM protection domain compose with metadata-bearing roles or observe application metadata through a colluding principal? | `NamespaceRegistry`, role-specific supervised endpoints, X resource ownership, portal grant admission, and the target session protection-domain policy. Exact PID/UID admission alone does not implement process isolation. |
| `ActionCapabilityTopology.als` | Can an action cross issuer families, recipient or authority epochs, revocation/expiry, target generations, or activation identities? | Future policy, broker, shell, and session action-capability admission. The model fixes identity relations but no wire fields or limits. |
| `PresentedTargetTopology.als` | Can a target outside owned visible pixels, below a higher-trust target, outside modal scope, or with a reused identity receive a hit? Can a shell issue its own coordinate grant? | The future last-presented interaction snapshot and authority/session/slot/generation target identity described in `docs/target-resolved-input.md`. No production shell schema exists yet. |
| `PolicyOperationBinding.als` | Can numeric action ranges, token substitution, or a target lacking the advertised permission invoke a session operation? | The committed `SnapshotBinding.session_operation_slot`, separately advertised opaque operation token, and optional target permission in `sophia_wm_v1`. |
| `TargetGeometryAndDisclosure.smt2` | Do containment, intersection clipping, quantization, capability-epoch rate limits, and target/outcome quotas imply their bounded disclosure obligations? | Future target-schema validation and the target-resolved disclosure reducer. Limits remain symbolic until measurement and schema evidence choose values. |
| `PolicyPresentationGeometry.smt2` | Do fullscreen, ordinary, and minimized placement rules keep geometry and focus consistent with output/work-area facts? | Canonical policy-projection validation: fullscreen is exactly output bounds, nonfullscreen geometry is within the work area, and minimized surfaces cannot hold focus. |
| `OutputTopologyPublication.als` | Can a release-ready scanout, pointer, RandR, and policy bundle contain different topology epochs, or can input resume before current policy and presentation? | The native session owner's fail-closed topology transition and complete publication barrier. Intermediate IPC settlement is sequential and remains quarantined; this is bounded decision evidence, not a Rust refinement proof. |
| `OutputTopologyGeometry.smt2` | Does a complete positive-width horizontal output sequence produce nonoverlapping contiguous bounds and the exact root width without overflow? | `output_topology_from_engine_outputs_at_generation`, pointer output bounds, and X frontend root geometry. |
| `WmFileRowsBounds.smt2` | Do file row counts fit their fields and row products fit wide arithmetic? | `protocol/sophia-wm-files-v1.kdl` row layouts, including capability-gated rows, and the neutral record codecs. |

`sophia-wm-file-rows-facts.smt2` is generated from the WM file schema by
`sophia-policy-protocol-gen`. The SMT proof names those facts but does not copy
their numeric values. Socket envelope and chunk-prefix claims retired with
the WM socket codec; file admission and complete-record bounds are exercised
by the protocol tests and independent WM file peer.

## Positive properties and negative controls

The gate checks both sides of every promoted rule. Alloy assertions must have
no counterexample, while attack predicates that omit one rule must produce a
witness. Z3 secure queries must be `unsat`, while deliberately weakened
queries must be `sat`. The retained negative controls cover:

- ambient role inference, cross-namespace access without a portal, shell
  self-issued coordinates, WM metadata observation, combined WM/shell domains,
  and protection-domain metadata collusion;
- cross-issuer action type confusion, wrong recipient roles, stale/revoked
  capabilities, recipient or target-generation substitution, and repeated
  activation identity;
- numeric session-operation inference, substituted operation tokens, and
  operation requests carrying an unpermitted target;
- targets outside allocations, occluded and lower-trust interception,
  ambiguous equal-trust overlap, identity reuse, and self-issued grants;
- torn output-consumer publication, partial output sets, stale presentation,
  input before current policy settlement, output-generation reuse, and empty
  published topology;
- unclipped and unquantized coordinates, frame-local rate-limit reset, and
  unbounded target partitioning;
- fullscreen geometry that is merely contained, ordinary geometry outside the
  work area, and minimized focus; and
- unchecked narrow multiplication of record counts and widths.

These attacks are satisfiable states in the weakened models, not unreachable
branches hidden behind the property being checked. A property is promoted only
when its positive check and corresponding negative witness both pass.

## Reproducible command

The unattended runner uses Alloy 6.2.0 with SAT4J, symmetry 20, explicit model
scopes, and Z3 4.16.0. It verifies the official Alloy Linux amd64 archive by
SHA-256, regenerates nothing, checks the schema-derived SMT facts for drift,
rejects Alloy command errors, and rejects `unknown`, solver errors, missing
witnesses, or unexpected witnesses.

```sh
SOPHIA_ALLOY_ARCHIVE=/absolute/path/to/alloy-6.2.0-linux-amd64.tar.gz \
  tools/check_architecture_models.sh
```

Pinned Alloy artifact:

- release: <https://github.com/AlloyTools/org.alloytools.alloy/releases/tag/v6.2.0>
- archive: `alloy-6.2.0-linux-amd64.tar.gz`
- SHA-256: `5a5494a4bac6e243e471590bb44a91e25a35794a5af1ae1f332be30b9c54a9e7`

The stable gate accepts exactly Z3 4.16.0 from `SOPHIA_Z3` or `PATH`. A local
Z3 5.x build is an optional differential, never the sole gate:

```sh
SOPHIA_ALLOY_ARCHIVE=/absolute/path/to/alloy-6.2.0-linux-amd64.tar.gz \
SOPHIA_Z3_DIFFERENTIAL="$HOME/src/z3/build/z3" \
  tools/check_architecture_models.sh
```

The differential must produce byte-for-byte identical labeled `sat`/`unsat`
results. It is useful for detecting solver-version sensitivity; it adds no new
architectural claim.

Spin/Promela concurrency models, dependency-policy gates, and fuzz targets are
follow-on work. Installed commands without checked-in models, policies,
corpora, expected outcomes, and reproducible runners are not validation
evidence.
