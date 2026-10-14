---
id: jxcqbd04
date: 2026-10-14
kind: investigation
status: closed
tags: [investigation, topology, input, lifecycle]
---
# Audit remaining topology and delivery obligations before architecture acceptance

## Question

Which t031/t032 obligations still need work before architecture acceptance?
This is t328's bounded source audit, not implementation of the old backlog.

## Current resumption

Audited Sophia `8a12afc1f` on October 14. Two route-lease notification sites
still use deadline-free blocking sends and ignore disconnect errors. Queued
configure geometry still lacks a topology stamp at the queue boundary; whether
its surrounding layout owner prevents every stale dispatch remains unresolved.
The historical broker and configuration-publication claims need narrower
reproductions before they justify repairs. Input receipts and page-flip batches
have substantial newer custody and accounting protections.

The audit exit is satisfied by the dispositions below. t031/t032 remain open;
none of their implementation or physical exits is closed here. Recommend a
bounded route-lease repair before t252, and a queued-geometry reproduction
before topology-dependent architecture acceptance. These are proposed queue
changes, not promotion of the entire backlog. The task ledger owns execution.

## Evidence

The [original inventory](../sources/2026-08/legacy-roadmap-0015-135-migrate-and-promote-the-native-policy-path.md)
names configure packets, Interaction causes, pointer handoff, legacy WM queues,
callback reads, three broker swallow points, input receipts, route-lease sends
and config-time bound publication. It does not give exact identities for the
three broker sites or the configuration site. Their continued presence cannot
be assumed after the routing and policy rewrites.

Paths below are relative to the repository at the audited revision. Function
names identify the inspected owners; existing tests were read, not rerun.

| Obligation | Disposition and source evidence | Remaining bounded action |
| --- | --- | --- |
| Queued configure geometry | **Unresolved.** `crates/sophia-session/src/session_control.rs`, `PendingControl` and `SessionControlKey`, retain command, client, surface, transaction and timing, but no topology epoch. Queue and acknowledgement deadlines do not establish geometry freshness. | Hold a configure behind backpressure, change output topology while keeping the surface alive, then release dispatch. Trace the layout transaction as well as the queue; require rejection or recomputation before an obsolete rectangle can apply. Add an epoch only if the owner cannot already prove freshness. |
| Queued interaction geometry | **Partly covered; unresolved at the queue boundary.** `live_session/wm/public_policy.rs` stores `LivePublicPolicyCause` without a topology stamp; `policy_cause_subject_is_live` checks an Interaction's target exists. Its cancellation reducer removes queued Updates. Both topology paths cancel active floating gestures, but target existence alone is not proof that all retained coordinates are current. | Queue Begin/Update/End across hotplug and policy replacement, including a live surface whose output geometry changes. Require explicit cancellation or fresh coordinates, preserving the terminal interaction obligation. |
| Pointer-focus handoff | **Covered for the inspected transition paths.** `live_session/owner_loop/topology_phase.rs` resets pointer and keyboard handoff, cancels repeat and advances the application control epoch when a fresh rescan starts. `owner_loop/wm_phase.rs` does the same on policy change. | Preserve those resets in a generic regression spanning both transitions. No pointer-coordinate replay defect was established here. |
| Legacy WM queued requests | **Historical owner superseded; current cases partly covered.** `live_session/wm/public_policy.rs` re-resolves affected outputs and rejects stale OutputAction generations. Interaction coordinates remain the separate row above. | Reuse t249's queued-action generation evidence; do not restore or repair the retired legacy queue. |
| Page-flip callback reads | **Covered at the inspected read/custody boundary.** `crates/sophia-backend-live/src/drm/native_page_flip/reader.rs` performs one kernel read and retains every callback returned in that batch. `poller/native.rs` retains pending callbacks and drains them before further reads. | Preserve full-batch and saturated-sink tests; never cap the consumed iterator and discard its tail. This finding does not claim a new physical timing bound. |
| Broker swallow points | **Unresolved historical mapping; current metadata transport propagates failures.** `crates/sophia-runtime/src/broker_transport.rs::request` checks transaction/epoch and propagates framing errors. `crates/sophia-session/src/live_session/metadata_broker.rs` propagates admission, candidate and retirement failures. Shutdown cleanup is distinct from successful delivery. | Before t314 acceptance, identify the old sites through history and map each to its successor. Inject broker disconnect, malformed reply and wrong transaction at the public boundary; require a named failure and no successful publication. Do not infer three current bugs from the old count. |
| Unbounded input-delivery channel | **Transport remains unbounded; semantic custody is bounded.** `live_session/run.rs` still uses `channel()` for receipts. `routing/broker.rs` sizes `InputRecovery`; `routing/recovery.rs` records one terminal receipt per ticket and retains its credit until observation and routing completion. `sophia-session/src/input_delivery.rs` distinguishes flush, revocation and failure. | Stop receipt consumption and prove admitted tickets and queued terminal receipts plateau at the configured bound; check all producers use the ledger. Only replace the channel if that proof fails, preserving nonblocking terminal settlement. |
| Route-lease notifications | **Still present.** `crates/sophia-x-authority/src/x11_socket/routing/registry/delivery.rs::release_route_lease` and `send_route_lease_update` call `let _ = sender.send(...)`. `sophia-wake/src/sender.rs::send` delegates to blocking `SyncSender::send`, without a deadline. | Retain pending notifications under bounded custody and return to the owner loop on saturation; settle disconnect explicitly. Test a full receiver, disconnect, exact acknowledgement and progress for another client. A bare conversion to `try_send` would lose lifecycle messages. |
| Config-time bound publication | **Unresolved historical mapping.** Current `live_session/config/reload.rs` prepares application configuration before publishing it; its tests preserve active state after failed preparation. `routing/registry.rs::install_continuation_owner` reports a failed bound installation through `Option`, rather than proving caller handling. Neither establishes which site the old inventory meant. | Trace the original bound-publication call through history, then test its actual current caller with refused installation and incompatible retained bounds. Require explicit startup/reload failure or a reported retained bound; do not report successful application of an uninstalled limit. |
| Zero-discard gates | **Partly covered.** `tools/verify_live_session_persistent_evidence.sh` requires zero authority drops, callback rejection/saturation and native submit/retire failures. It is not evidence that every route-lease or configuration failure has a counter. | Extend each repaired boundary's generic gate with conservation accounting: accepted equals completed plus explicitly refused/cancelled plus retained pending. Require zero unexplained loss; legitimate stale refusals must not be mislabeled successful delivery. |

Session paths abbreviated as `live_session/...` above live under
`crates/sophia-session/src/`; routing paths live under
`crates/sophia-x-authority/src/x11_socket/`.

## Finding and resolution

The confirmed route-lease issue is frontend-to-Session notification, not the
best-effort Session release notice in `advance_application_input_security_epoch`.
That notice follows the control-epoch barrier, which independently clears old
input authority. Treating both ignored sends alike would obscure the actual
blocking call. The ordinary Session release path already distinguishes a full
queue from disconnect and relies on its bounded release deadline.

The queued-geometry gap needs a focused reproduction, not an assertion that
every missing field causes a live failure. Place that proof before acceptance
of topology-dependent shell/layout behavior; promote only the demonstrated
repair. Broker failure controls fit t314, and configuration-bound verification
belongs before accepting changed runtime bounds. They should reuse their
existing owners rather than create a second architecture project.

The bounded route-lease repair is the clearest proposed pre-optimization
blocker: a full queue can block its sender indefinitely by API contract.
No live hang was induced, and no claim is made that this caused a reported
desktop incident. t328 does not authorize its implementation.

## Validation and remaining work

Read the existing `sophia-session/tests/input_delivery.rs` receipt controls and
`tests/support/live_session/output_topology_owner_tests.rs` topology coverage.
The backend's `tests/libdrm_events_feature/native_page_flip.rs` includes pending
backlog, route replacement and bounded emission cases; `session_loop.rs` covers
draining pending callbacks without fresh readiness.

The [t249 launch/output evidence](5rzn1zue-wm-qualification-after-source-retirement-and-attended-output-recovery.md#launch-and-logical-output-recovery-2026-10-13)
was verified through `zk context`: stale output generations and queued actions
were exercised against the real peer, with negative controls. Its supplied
topology/completion facts do not prove physical hotplug or general configure
freshness. No build, runtime test, hardware access or new quiet window was needed
for this audit. Existing tests are source evidence, not newly passing results.

## Connections

- [t328 audit exit](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t328).
- [t031/t032 retained scope](../plans/queue-13-authority-and-lifecycle-hardening.md).
- [Task ledger](../../../todo.md) owns promotion and order.
