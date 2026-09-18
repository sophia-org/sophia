# Headless M3 component controls

Run a closed exact-test suite on one clean committed revision:

```sh
cargo xtask check m3-components --suite=retained-maintenance \
  --output=/REPOSITORY/.artifacts/NEW-RUN \
  --target-dir=/REPOSITORY/.artifacts/OWNED-TARGET
```

The Rust runner shares the acceptance gate's immutable source snapshot,
single contained build, binary/configuration hashes, strict exact-test parser,
timeouts and process collection. The existing namespace launcher remains the
unchanged containment boundary. Device and live-session access is unavailable.

`suites.json` names every required component test. Missing, zero, ignored,
failed, timed-out or uncollected tests prevent a component pass. CLI filters,
replacement binaries and partial suites are unsupported. Add exact names to a
new named suite when another component needs a repeatable gate.

Diagnostics that need the acceptance module's private fixtures belong under
`x11_socket::routing_tests::m3_acceptance::diagnostics::`. Add their exact full
names to an explicit suite in `suites.json`; the prefix is not a filter and does
not discover or run other tests. Every other test under `m3_acceptance` remains
forbidden in component suites. Diagnostics cannot be bound in the acceptance
inventory. No fixture visibility change is needed for this separate child module.

The report's purpose is `m3_components`; its separate `components.verdict`
describes only that suite. Aggregate acceptance remains `NOT_RUN`, with all
20 acceptance rows unchanged. Component controls may use labelled fault seams;
passing them does not satisfy the integrated M3 acceptance inventory.
