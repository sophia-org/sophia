# Private Session acceptance

Run from a clean, committed checkout:

```sh
cargo xtask check m4-acceptance \
  --output=/absolute/repository/.artifacts/m4-run \
  --target-dir=/absolute/repository/.artifacts/m4-target \
  --case-timeout=120
```

The output directory must be new. Use a fresh target for each source snapshot.
The runner archives the committed source, builds offline inside containment,
and records source, executable and process-collection identities. Builds use
two jobs. CPU affinity and scheduling priority may be set on the outer command
to leave room for the desktop.

`report.json` is the result. All eight inventory rows must pass before M4
passes. An unbound row remains `NOT_RUN`; a failed row keeps the aggregate
failed. A nonzero exit may therefore mean either failure or incomplete
acceptance. Read the report rather than inferring a verdict from the exit alone.

Most rows run the public `sophia-session` integration target. Lifetime uses
the separately attested Session library test binary for labelled fault seams.
Evidence integrity uses the gate's test binary to tamper with a preceding real
Session result. Its actor evidence refers to that preceding invocation; it does
not claim another service execution.

The thin `native_input_conformance_host` requires kernel namespace activation
and one delegated control pipe. Its paths, topology, credentials and lifetime
are explicit. Host-entry controls test readiness, containment and collection;
the public Session controls separately test real X peers and wire delivery.
Neither discovers an installed display or uses physical input devices.

Use `--self-test` for the runner's own controls. Those results leave every
acceptance row `NOT_RUN`. M3 keeps its separate twenty-row gate and must be
rerun on the final integrated source. M4 does not enable XTEST discovery or
establish native desktop acceptance.
