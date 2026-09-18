# Private M3 acceptance

Run the dedicated Rust xtask subject from a clean committed checkout:

```sh
CARGO_TARGET_DIR=/absolute/repository/.artifacts/m3-finish/harness-target \
  cargo xtask check m3-acceptance \
  --output=/absolute/repository/.artifacts/m3-finish/acceptance-RUN \
  --target-dir=/absolute/repository/.artifacts/m3-finish/harness-target
```

Use the common repository's `.artifacts` directory, including from a worktree.
The output must be new, cannot overlap the target, and is never overwritten.
Do **not** replace the dedicated subject with bare `cargo xtask check`, which
has additional hardware checks. This subject never calls that gate.

The command snapshots one exact clean commit with `git archive`, records the
commit, tree, archive and extracted-content hashes, mounts that snapshot read
only, and builds one `sophia-x-authority` library test binary offline and locked.
It records actual features/profile, compiler hashes, configuration, binary hash,
exact commands, output logs, collection and the twenty-case verdict. The archive
has no `.git` link to the host worktree; content is rehashed inside containment
before and after execution. This is identity attestation, not signature review.

The existing `x11_conformance/isolation.py` boundary supplies isolated mount,
network, PID, user, IPC and UTS namespaces, a private `/dev`, a cleared environment
and validated descriptor inheritance. No GPU/input node, host runtime socket or
delegated descriptor is mounted. The small `containment.py` compatibility adapter
only invokes that existing boundary and execs xtask after its kernel validation;
all new inventory, build, timeout, process collection and verdict logic is Rust
in `crates/xtask/src/m3_acceptance/`. Missing containment fails closed.

Every case is mandatory. `inventory.json` preserves the exact 20 coordination
case IDs and requirements and enumerates their mandatory subcases. All bindings
are initially absent: the first run therefore reports **20 NOT_RUN** and exits
nonzero. Existing component tests are not mapped to integrated acceptance.
No case, filter, skip, arbitrary command or external-binary option is accepted.
Missing tests, zero tests, ignored tests, timeouts and incomplete actor/process
collection can never yield PASS. A failed or partial run remains inspectable in
`report.json` and `evidence/inner-report.json`.

## Adding an integrated case

Add the actual complete case to the product's private test module, then bind it
explicitly in `bindings.json`:

```json
{"schema":1,"cases":{"A.press_release_repress":"x11_socket::routing_tests::m3_acceptance::press_release_repress"}}
```

The test is run by exact full name against the one built binary. It must exercise
every listed subcase, collect its actors and emit exactly one complete line:

```text
sophia_m3_acceptance {"schema":1,"case":"A.press_release_repress","subcases":{"exact_press_release_bytes":"PASS","native_and_recipient_proofs":"PASS","repress_barrier_then_success":"PASS","overlapping_button_releases":"PASS"},"cleanup":{"actors_started":2,"actors_collected":2,"pending_actors":0,"complete":true},"observations":{"example":"replace with actual identities, bytes, proofs, receipts and credits"}}
```

This example documents the schema; it is not acceptance evidence. Actual test
observations must support the inventory requirement. An exact namespace/name
alone does not establish test quality; reviewers inspect the bound source.
The separate `m3_acceptance::diagnostics::` child module is reserved for exact
non-acceptance controls run through `m3-components`. Its tests are rejected as
acceptance bindings, and their passing component reports leave every acceptance
row `NOT_RUN`.
`--show-output` keeps the marker on its own line. A real test must publish its
collection evidence only after its service/worker actors have actually joined.
Process exit or namespace destruction does not substitute for those joins.

The Rust child owner separately waits for the test process and acts as a Linux
subreaper. It kills timed-out groups and collects adopted descendants. A test
that leaves descendants fails even when the harness successfully collects them.
The outer launcher separately collects adopted namespace-launch children; its
report requires every discovered child to be reaped, with none remaining. This
does not relax the stricter zero-descendant rule for individual case processes.
The outer namespace deadline is a final containment limit, not a product latency
or preemption claim.

## Harness self-tests

Add `--self-test` to the same command with a new output directory. This builds the
xtask test binary once and runs the Rust tests in `crates/xtask/tests/support/`
inside the same device-hidden boundary. These exercise synthetic parser data,
inventory/filter/cleanup rejection, an actual timeout and an actual orphan.
Their successful exit is labelled `harness_self_test`; all M3 rows stay NOT_RUN.
They do not establish that any integrated product case passed.

The default whole run limit is 1800 seconds, build limit 900 seconds and per-case
limit 60 seconds. `--timeout=`, `--build-timeout=` and `--case-timeout=` accept
1–1800 seconds. Allowance exhaustion leaves the remaining cases NOT_RUN and
prevents aggregate acceptance. The expected complete gate requires all twenty
cases on the same source/binary/configuration with successful collection.
