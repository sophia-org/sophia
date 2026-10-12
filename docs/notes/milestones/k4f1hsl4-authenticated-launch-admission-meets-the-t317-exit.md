---
id: k4f1hsl4
date: 2026-10-12
kind: milestone
status: recorded
tags: [milestone, 9p, security]
---
# Authenticated launch admission meets the t317 exit

## Result

[t317](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t317) meets its
original authenticated-admission exit. Normal supervised WM, lock, output,
descriptor-shell, bar, launcher and dock owners use Session-attested,
Factotum-verified launch authentication. The first successful attach fixes the
transport's principal, namespace and launch. Reset destroys afids and verdicts
without changing that identity; fresh authentication must prove the same one.

Authentication and current launch custody remain separate checks. The core
receives a verified result, while Session derives and publishes the immutable
admission context with the prepared role root. Retained operations recheck live
authority. The expected-peer availability filter and supervisor pidfds remain;
the authenticated lock branch no longer requires socket `SO_PEERPIDFD`.
X11 admission is unchanged.

## Exit reconciliation

The [implementation investigation](../investigations/etyvymxi-authenticated-attach-transport-state-and-factotum-handoff.md)
owns the detailed candidate bindings, executed positive cases, discriminating
controls and limits. The table maps the original exit and approved t133 handoff
to that evidence, rather than replacing them with a narrower completion rule.

| Requirement | Evidence and boundary |
| --- | --- |
| A required afid cannot be omitted, forged or self-verified | Core wire tests and the real Factotum role-afid join refuse plain attach, forged claims, an unverified afid and peer-written success. Normal role owners have no authentication-failure fallback. |
| Second identity cannot change retained authority | Core pin/reset tests vary principal, namespace and launch; the protected admission-context join preserves the exact context across Tversion. Reset and retained-admission guard controls discriminate loss of the pin and stale upload authority. |
| Custody belongs to an authorized launch | Protected handoff tests refuse unprotected spawn and configured fd claims; exact spawn generation controls activation. Worker custody detects exit before an owner poll. Bootstrap, forwarding and revocation controls keep delegation within the original launch. |
| Lock admission works without hard socket-pidfd dependence | The authenticated accept branch checks the live assignee and then afid/current custody without calling socket_peer_pidfd. Protected C and Rust lock joins exercise it; the legacy branch retains its separate requirement. |
| Atomic admission and revocation | Prepared root/context publication, full-fid refusal and abandoned-preparation tests prevent partial admission. Status-to-attach, retained-read and between-fragment controls distinguish final live checks from idle cleanup. |
| Bounded attempts, queues and owner progress | Core reset/deadline/transcript tests, Factotum correlation and quota tests, shared ingress/cleanup tests and blocking-send controls cover the named budgets. Locked-state connector-flood and launch-capacity tests cover availability refusal and backoff without unlocking. |
| Protected bootstrap and independent peers | Minimum Bubblewrap execution and descriptor-exposure controls qualify the handoff. Independent C/Rust SDKs execute the exchange and continue on the admitted root. External clients separately qualify their startup adapters and fresh launch generations. |
| CPU, memory, wakeups and bounded failure observations | The closure-observation package records process CPU ticks, RSS and worker pass/wait counters for authentication and a retained waiting read. Deadlines have separate failure oracles. These are scoped observations, not latency, peak-memory or zero-CPU claims. |
| Threat model and role inventory reconciled | The [admission review](../investigations/1pv291te-namespace-and-client-admission-security-gaps.md#t317-implementation-reconciliation-2026-10-12) and [contract](../../factotum-launch-authentication.md#transport-boundaries) record the trust root, intentional delegation, retained checks and separately owned roles. |

## Candidate and gates

Signed `dc81ad00193a9d2d8da4a3c86aaca9d2b653f45e` passed the complete isolated
`cargo xtask check`: **7,761 passed, zero failed, 111 ignored, 563 summaries**,
plus Clippy, source layout, SDK integrity and tooling/archive verifiers.
Package `t317-final-candidate-02` has manifest SHA-256
`2117da3b02bc23620c4d500e5efa14543b6812ca1cf591dc81db46ebf586a6b9`.
The preceding layout failure remains preserved as `t317-final-candidate-01`.

The signed merge candidate
`078da28d8886ee8de8a7ae8a2dde9b17d94475b6` incorporates master's independently
qualified RANDR changes at `118caa68deafdff4e64080bc1fdda123b0db65e1`.
Its complete isolated gate also passed: **7,770 passed, zero failed, 111 ignored,
564 summaries**, including Clippy, layout and the final tooling stages.
The worktree was clean at both ends and HEAD remained the recorded candidate.
Package `t317-integration-01` has manifest SHA-256
`8a9e9131a26d72d4c049264a4eeefd5d6ba913559ba8ba488840144f94d41aa3`.
All packages are under `~/.local/state/sophia/development-evidence/`.

The gate ran under the worktree's exclusive lease and cargo slot, with private
namespaces and devices/display access hidden. Ignored tests are not counted as
executed by that gate. Supplied-executable client joins, minimum-version checks
and negative controls retain their separately recorded runs and source pins.
The final-candidate package rechecks integrity of 110 investigation packages
and 11 client packages; integrity checking is not re-execution. Current generic
owner regressions ran in both final gates. SDK changes after the joined pins
are contract/provenance-only; the integration merge adds RANDR work, not an
authentication implementation change. The closure commit adds documentation
and corrects the lock transport's obsolete module comment, with no code change.

## SDK and client integration

The C SDK's local master is `e28fecd3e63133268d21538cf5107611ca0c6f31`;
the Rust SDK's is `d5fe721cd0f77b64ae8f6859ee39cf0d828faffa`. Both development
branches are published and Sophia pins those immutable revisions. No release
tag or remote-master publication is claimed.

The qualified client changes are on their local master branches:

| Client | Commit | Joined boundary |
| --- | --- | --- |
| Hagia | `60d6331cecdd874a098bd31f29afd0b912cab927` | Initial authentication and supervised restart, empty-scene commits, wrong-claim refusal |
| Narthex | `11eb0332859ed0e0b9244dd3ddafcea70c2b1b6e` | Descriptor negotiation at two launch epochs, idle liveness, revocation and wrong-claim refusal |
| Lom | `dc65f561f8c9522f6a09d3e5f386d32226228bfd` | Authenticated component negotiation twice, then expected GPU-permission refusal |
| Bemenu | `6e2efd96e2256a138ed9928387e83d3c30585937` | Launcher negotiation twice, connected liveness, revocation and reaping |
| Provlita | `da4a15adb9a6555441653a9dd9ec0e4d69f3d6af` | Authenticated component negotiation twice, then expected GPU-permission refusal |

Finished client and SDK worktrees were removed after inspecting ignored files.
Client-specific checks remain outside Sophia. Evidence records executable paths
and hashes, with no binary copies. This is source integration, not installation.

## Limits and next boundary

Session and the supervisor remain trusted. A cooperating capability holder can
authenticate another transport within its own delegation; the mechanism does
not prove an executable image, human identity or absence of intentional proxying.
There is no remote authentication or same-UID fallback for unlaunched tools.

t318 owns namespace composition and authorized developer launch. t324 owns
unlaunched credential provisioning. Metadata/control migrations remain t314
and t254; the live portal retains its approved protocol-authority socketpair
exception. WM qualification under t249/h006, backend/pixel joins and physical
acceptance remain separate. No live installation, session restart, rendered
client acceptance or physical-device claim follows from completing t317.
