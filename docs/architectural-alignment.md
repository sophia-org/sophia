# Architectural Alignment And Evidence Policy

**Role:** architecture-verification policy and evidence map.
**Status:** TLA+, Alloy, and SMT gates are active; implementation refinement,
Spin/Promela, dependency-policy automation, and fuzz promotion remain future
work.

Sophia uses several small models because no one abstraction answers every
architecture question. A solver result is evidence only for the checked model,
scope, and correspondence. It is not a claim that the Rust implementation is
equivalent to the model, that all larger structures are safe, or that an
authorized component reveals no information.

## Division of responsibility

| Evidence layer | Active tool | Owned question | Explicit limit |
| --- | --- | --- | --- |
| Temporal transition safety | TLA+ Tools 1.7.4 | Presented versus committed state, epochs, capture/arbitration, shell/work-area/WM coordination, pacing, retirement, bounded queues, and stated fairness | Hand-maintained project models, not refinement proofs |
| Bounded relational topology | Alloy 6.2.0 with SAT4J | Role admission, protection-domain composition, namespace/portal reachability, action-capability identity, visible target ownership, trust precedence, target identity, and independent coordinate authority | Finite scopes; no event ordering or implementation linkage |
| Arithmetic and wire bounds | Z3 4.16.0 over SMT-LIB2 | Region containment/clipping, quantization and budgets, payload prefixes, field representability, and record-size multiplication | Formula-specific results; symbolic shell limits are not ratified schema values |
| Executable behavior | Offline Rust and C99 tests, QEMU, focused physical proofs | Codec agreement, reducer behavior, real clients, backend integration, and hardware-specific claims | Only the exercised workloads and named environments |

The models are complementary, not translations. TLA+ remains the owner of
time and lifecycle. Alloy does not duplicate capture cancellation or queue
pacing; it searches static authority and overlap structures. Z3 does not model
roles or event order; it discharges arithmetic predicates that are needlessly
opaque in a transition model. There is no shared generator DSL between them.

The active model inventory, action/boundary correspondence, negative controls,
tool pins, and commands live in [`validation/tla`](../validation/tla/README.md)
and [`validation/architecture`](../validation/architecture/README.md).

## Promotion rule

An architecture statement becomes a solver-backed gate only when all of these
conditions hold:

1. The owning document states the authority boundary and threat being checked.
2. The model admits attempted bad states rather than making the result true by
   an unreachable action guard.
3. The intended property passes and a focused weakened rule produces the
   expected counterexample or satisfiable witness.
4. The runner pins or version-checks the solver, uses explicit bounds and
   deterministic settings, rejects errors and `unknown`, and leaves solver
   state outside the repository.
5. The model documents which current or target implementation boundary it
   corresponds to and what it omits.
6. Any model-discovered implementation bug becomes a deterministic executable
   regression before either side is changed.

Installing a command is not evidence. A tool becomes part of Sophia's
validation surface only with retained input models or corpora, expected
outcomes, a reproducible unattended runner, and a documented interpretation.

## Active relational and arithmetic tranche

The authority Alloy model checks role-specific admission, namespace ownership,
portal-mediated cross-namespace access, WM metadata blindness, forbidden
protection-domain role composition, and independent coordinate grants. The
action-capability model checks issuer families, issuer/recipient and revocation
epochs, operation class, target generation, and activation replay. The
presented-target model checks the future interaction snapshot:
targets stay within authority-owned visible pixels, higher-trust targets and a
deterministic equal-trust order win, modal membership is exact, identities are
unique across authority/session/slot/generation, and a coordinate recipient
cannot issue its own grant.

The target-geometry SMT model proves only bounded data-minimization mechanics:
intersection clipping, quantization, capability-epoch rate quotas, and target
and distinguishable-outcome budgets. It deliberately does not claim zero
telemetry. Action identity and a sufficiently fine target partition can reveal
user choices, which is why count, precision, and rate remain part of the
budget.

The WM file-row SMT model consumes constants generated from
`protocol/sophia-wm-files-v1.kdl`. It checks count representability and bounded
row products, with a negative control for unchecked 32-bit multiplication.
Rust/C99 file codec tests and neutral row vectors remain the executable
evidence; the SMT result does not replace them.

## Security debt the models do not close

The models harden target choices early, but they do not turn target-resolved
input into an implemented shell interface. `sophia_shell_v1`, concrete target
quotas and wire discriminants, accessibility projection, and runtime tracing
remain pre-schema work.

They also do not repair the whole application path. The installed pointer path
now publishes an independently retired interaction projection and semantic
epoch per output, rather than selecting from newer committed state or merging
heads. Ordinary, passive-grab, and explicit core/XI pointer ownership
establishes exact Engine-visible leases with frontend confirmation,
profile-scoped retention, ordered release, and VT/seat epoch quarantine.
Explicit preparation is passive and bounded, routes nothing before activation,
and rejects saturation. A saturated private X input queue now quarantines its
owning client and rejects tracked delivery without terminating the shared
frontend service.
Recreated XIDs now receive fresh Sophia surface generations and removal retires
the exact frontend route, so frozen input cannot bind across that ABA boundary.
The remaining runtime debts are recorded in `docs/target-resolved-input.md`,
the linked investigations under `docs/notes/`, and `todo.md`. Compiled descriptor
capture now shares application-grab arbitration; lock-authority epoch integration and broader
shell capture remain incomplete.

## Reproducible checks

Run the complementary architecture gate with the pinned Alloy archive and the
stable Z3 release:

```sh
SOPHIA_ALLOY_ARCHIVE=/absolute/path/to/alloy-6.2.0-linux-amd64.tar.gz \
  tools/check_architecture_models.sh
```

Run the temporal models independently:

```sh
SOPHIA_TLA2TOOLS_JAR=/absolute/path/to/tla2tools.jar tools/check_tla.sh
```

Then retain the executable protocol and workspace gates described in
`docs/validation.md`. A local Z3 5.x checkout may be used as an optional
differential through `SOPHIA_Z3_DIFFERENTIAL`; stable Z3 4.16.0 remains the
required result.

## Deferred tools

Spin/Promela may be admitted when a concurrency question is not already clear
in the TLA models and a concrete worker/channel correspondence exists.
`cargo-deny` and dependency graph checks require a reviewed policy file and
documented exception process. Fuzzing requires promoted targets, seed corpora,
resource limits, crash retention, and deterministic regressions. Until those
artifacts exist, these tools are candidates rather than implemented layers of
Sophia's validation stack.
