# Specula Development Audit

**Role:** optional, commit-pinned development analysis; never a runtime or
build dependency.

The profile activation and reattachment briefs and instrumentation maps below
refer to the former WM IPC transport at their source revisions. t269 retires
that transport; these retained design models and traces are not refinement
evidence for the WM file reactor. The profile reducer remains shared, and
current export/owner evidence is in Session's `policy_file_*` tests and
`tools/check_policy_protocol.sh`.

Specula complements Sophia's hand-maintained TLA+ models by deriving a bounded
model from a deliberately narrow source slice, validating implementation
traces, and hunting for counterexamples. Generated models are review input,
not product specifications or refinement proofs. Sophia retains only corrected
project-sized models and deterministic regressions.

Install or verify the pinned development tool on Void Linux with:

```sh
tools/install_specula_dev.sh
tools/install_specula_dev.sh --verify-only
```

Do not commit `.specula-output`, copied source trees, agent transcripts, TLC
state databases, generated patches, or raw traces. Record confirmed behavior
as the smallest deterministic Rust regression and update the owning TLA+
model, boundary map, research log, and milestone instead.

The desktop-profile startup activation protocol is modeled before wire
implementation under `profile-activation-protocol/`. Its checked design model
separates Sophia's local policy proxy from Hagia's actual authority state,
binds every completion to epoch/transaction/generation/digest, and keeps the
graphical launch gate closed through rejection, timeout, disconnect, and
rollback. The included trace is explicitly a design trace; it is not runtime
conformance evidence. Run the offline base search, focused hunts, and trace with:

```sh
SOPHIA_SPECULA_TLA2TOOLS_JAR=/absolute/path/to/tla2tools.jar \
    tools/check_profile_activation_protocol_model.sh
```

Post-promotion Hagia recovery is modeled independently under
`profile-reattachment-protocol/`. It freezes the globally active identity and
committed graphical state, requires a strictly newer authenticated epoch, and
admits normal policy configuration only after the replacement acknowledges the
exact retained candidate. This is a design prerequisite for production wiring,
not evidence that reattachment is already enabled. Run its base search, five
focused hunts, and complete design trace with:

```sh
SOPHIA_SPECULA_TLA2TOOLS_JAR=/absolute/path/to/tla2tools.jar \
    tools/check_profile_reattachment_protocol_model.sh
```

Both protocol checkers share `tools/lib/specula_tlc.sh`, including the pinned
jar checksum and isolated temporary model directory.
