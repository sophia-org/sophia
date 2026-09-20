# XTEST adapter acceptance

Run from a clean, committed checkout:

```sh
cargo xtask check m5-acceptance \
  --output=/absolute/repository/.artifacts/m5-run \
  --target-dir=/absolute/repository/.artifacts/m5-target
```

The output directory must be new. Use a fresh target for each source snapshot.
The runner archives the committed source, builds offline inside containment,
and records source, executable and process-collection identities, as the M3
and M4 gates do. Builds use the same bounded job count. CPU affinity and
scheduling priority may be set on the outer command to leave room for the
desktop. An explicit `--case-timeout` overrides the default case deadline.

`report.json` is the result. The eight inventory rows are obligation groups
from the M5 execution contract, not requests, so a failing row names a
behaviour: registration and admission, version negotiation, cursor comparison,
fake input encoding, fake input effects, the processing barrier, cancellation
and half-close, and grab control. All eight must pass before M5 passes. An
unbound row, or a bound test absent from the built binary, remains `NOT_RUN`;
a failed row keeps the aggregate failed. A nonzero exit may therefore mean
either failure or incomplete acceptance. Read the report rather than inferring
a verdict from the exit alone.

Every row runs the `sophia-session` integration target `xtest_acceptance`,
because admission, cancellation and the barrier are Session obligations the
standalone conformance host does not hold. Its case tests carry `#[ignore]`,
so an ordinary `cargo test` cannot claim acceptance; only the gate includes
them. Each case prints one `sophia_m5_acceptance` record naming every subcase
of its group as `PASS` and accounting for every actor it started.

The inventory is compiled into the gate and its `plan_source_commit` pins the
revision of `docs/notes/plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md`
the contract was derived from. A checkout whose copy differs in any row, or
in that revision, is refused before anything runs.

Use `--self-test` for the runner's own controls. Those results leave every
acceptance row `NOT_RUN`. Independent evidence for the wire is the twenty
XTEST cases of `tools/probes/x11_conformance/xtest_manifest.json`, which
t093 owns; M3 and M4 keep their own gates and must pass again on the same
source. M5 does not enable XTEST discovery, install anything, or establish
native desktop acceptance.
