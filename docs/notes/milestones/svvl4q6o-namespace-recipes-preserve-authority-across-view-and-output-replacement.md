---
id: svvl4q6o
date: 2026-10-12
kind: milestone
status: recorded
tags: [milestone, namespaces, 9p]
---
# Namespace recipes preserve authority across view and output replacement

## Result

[t318](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t318) meets its
namespace composition and launch exit. A policy-selected recipe produces an
immutable per-identity endpoint directory and versioned discovery. Names,
service identity and permission remain separate. New views change new
connections; retained handles keep their original owner and live grant checks.
Ordinary applications select approved policy automatically. The developer
launcher accepts an independent executable only under explicitly authorized
recipe and HostDomain policy.

The [implementation investigation](../investigations/w1gyz791-namespace-views-retain-service-identity-across-publication-and-revocation.md)
retains the individual candidate bindings, failed attempts, positive runs,
negative controls and supplied-fact boundaries. This is completion of the
existing exit, not a reduced replacement for it.

## Exit reconciliation

| Requirement | Qualified owner and evidence |
| --- | --- |
| Private equal names; shared identity and aliases | Compiler and `namespace_views` tests distinguish private references, shared export identity and independent grants. The foundation scope control and endpoint-grant controls discriminate foreign scope, revoked authority and changed service generation. |
| A-to-B rebind and retained handles | The authenticated inspection join reads the old A fid, a fresh fid on the old root, new B and the earlier A alias. Separate snapshot-refresh and late-alias-lookup controls fail at their named assertions. |
| Admission/view replacement | Recipe publication and registry tests retain old naming on failure while authority reductions stay revoked. The view-revocation control proves failure cannot restore authority. Fresh authenticated connections select the new view. |
| Directory composition | Compiler tests cover deterministic prepend/append order, identical-leaf deduplication, conflicting references/scopes/kinds and rejected flags. Permission belongs to the captured endpoint; composition does not provide a permission fallback. |
| Invalid input, bounds and publication failure | Compiler/include tests exercise syntax, missing source, escape, cycles, changed inputs and limits. Materialization fault tests clean partial candidates and retain the old view without undoing revocation. |
| Parent/child and delegated streams | Reuse the [t317 custody result](k4f1hsl4-authenticated-launch-admission-meets-the-t317-exit.md): bootstrap, exact spawn, forwarding and revocation controls. t318's protected namespace-client and independent SDK joins exercise that owner with selected endpoint grants. Recipe text alone supplies no custody. |
| Revoked bounded transfer | The authenticated portal recipient join uses the common coordinator and lease; generation, lock and replacement retire retained and waiting reads while role access survives. Omitting the final source-generation check fails the named watch assertion. Source bytes and policy are supplied. |
| Reused output number | `output_generation::reused_output_number_cannot_revive_a_retained_grant` supplies an output inventory and bounded record handler. The production common lease and recipient export refuse old read/watch handles after output 7 changes from generation 9 to 10, preserve output 8's grant and allow a fresh generation-10 grant. The one-comparison control fails the old-read assertion. This is common authority qualification, not a renderer or topology-adapter join. |
| Observer and authorized launcher | Observer selection excludes X and host-admin reach; participation requests cannot grant approval or injection. Developer codec, transport and Session-owner tests cover allowed launch and denied recipes. The permission control turns the named denial into an incorrect Started result. |
| Host reach and required confinement | The repository gate repeats the real Bubblewrap path/descriptor exclusion and confined X authentication tests from t142, plus selected-recipe fail-closed tests. No production containment change occurs in the final follow-up. |
| Shared/confined recipes and ordinary app UX | Configuration, identity-selection and live application-owner tests cover the five launch paths. The selection control fails when namespace custody is bypassed. niltempus confirmed normal terminal and browser launches in the installed release. |
| SDK adoption and independent executable | The [client guide](../../namespace-client-guide.md) documents discovery, independent C/Rust SDK authentication, retained handles and a generic observer example. The [developer interface](../../developer-launch-files.md) requires explicit trusted authorization; a same-UID caller or requested role does not grant authority. |

## Signed candidate and gates

The implementation commits `b8d99a419` and `ca807f1e8` were integrated in
`99860d58f`. Signed follow-up `e47bdaead60fd665ab910a6b4713576f250b5af8`
adds the output-generation regression and updates stale architecture text.
Every file captured by `t318-gate-12` matches that signed commit. Its isolated
`cargo xtask check` passes: **7,913 passed, zero failed, 118 ignored across 587
summaries**, plus lint, layout, SDK and archive checks. Summary totals include
invoked children. The source/index snapshots remain unchanged across the run.
The package's self-excluding SHA256SUMS has SHA-256
`3b2abe376f4d51f66a7dca65f5d3c2f4649175274381295a16df689d9f1b71bf`.

`t318-output-generation-01` and `t318-output-generation-control-01` have manifest
identities `d2f12db99168ffc6471fb52826cb29b1419b3f316fafa0b3b6ff44ace058040e`
and `58ee1e7e4f68734d44a1e24d6b99a845fe953b536dcbaabf6bcb297be1eb05d9`.
The control omits only the common lease's retained source-generation comparison.
It exits 101 at `reused output number must not preserve the old grant's read
authority`: the stale record is readable instead of ESTALE. It does not show
replacement pixels being returned. Source restoration, candidate hashes and
the passing exact rerun are recorded. Authentication and output facts are
supplied in this focused export-handle test; the earlier SDK joins qualify
actual authenticated fids separately.

The running release `niltempus-7b8051ace95f8581d57e` uses Sophia `99860d58f`
and integration `a400815`. niltempus confirmed usable Super+Enter and Super+B
launches. `t318-live-acceptance-01` records that observation and the executable
identity. Comparing the installed source to the final candidate across crates,
Cargo and vendor files finds only the new test: production and SDK bytes are
unchanged. No reinstall is needed for this test/documentation follow-up.
The installed WM and shell remain at their previously qualified client pins;
this observation does not close t249's separate client qualification.

## Remaining boundaries

Session and the supervisor remain trusted. Delegation stays within the original
launch authority; authentication does not prove an executable image or prevent
intentional proxying. The output inventory, record handler and admission used
by the focused output test are supplied; no new capture ABI or production
capture adapter was added. Renderer capture and its CLI remain t319; local
credentials for independently launched clients remain t324. Physical output
return, native pixels and latency remain their own gates. The current
[task queue](../../../todo.md) returns to t249 without waiving any WM join or
measurement requirement.
