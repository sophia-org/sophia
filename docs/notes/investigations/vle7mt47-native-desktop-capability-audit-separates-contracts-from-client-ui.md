---
id: vle7mt47
date: 2026-09-18
kind: investigation
status: implemented
tags: [investigation, shell, protocol, developer-experience]
---
# Native desktop capability audit separates contracts from client UI

## Baseline and method

Source baseline: Sophia `4049fea8f265e50ae6f72976e011f68231e72e50`.
The operator requested platform enablement rather than another desktop UI.
The [developer map](../../native-desktop-capabilities.md) is the capability
matrix; this note owns its evidence and limits. No runtime changes, new wire
numbers, application changes, live connections or hardware runs belong to this
audit. Existing tests below were inspected, not rerun for this documentation.

Trace each operation through schema, negotiated capability/configuration,
owning state, production dispatch, optional client helper and test. Record
connected/partial/specified/absent/external separately from evidence strength.
Absence findings are scoped to audited native public interfaces and service
wiring, not a claim that no similarly named utility exists anywhere.

## Source and test evidence

Paths are repository-relative. Tests identify concrete controls; their presence
is not full acceptance. A reducer or codec alone is not a developer endpoint.

| Group | Contract and production owner | Evidence and remaining boundary |
| --- | --- | --- |
| Admission | `protocol/sophia-shell-v1.kdl` r1–8; `crates/sophia-config/src/shell_components.rs`: exactly Bar/ApplicationLauncher/Dock, MAX_SHELL_COMPONENTS=3; Session `live_session/metadata_shell/component_launch.rs`, `component_session.rs`, `component_service.rs`, `component_lifecycle.rs`; runtime `shell_content/epoch_registry.rs` | Runtime `shell_component_transport.rs`: foreign-grant refusal, shared stores, handshake cleanup and legacy epochs; Session `shell_component_processes.rs`: protected children/Bemenu with supplied native facts. Not arbitrary-role admission or all failure schedules. t104/t105/t109. |
| Resources/pacing | r5 content capability; runtime `shell_content/{profile,accounting,resources,candidates}.rs`; Session metadata-shell content; backend production visual/native owners | Runtime `shell_content_resources.rs`, `shell_content_candidates.rs`, `support/shell_control_budget.rs`; Rust client lifecycle and backend owner fixtures. Exact Presented and Released differ. Retained-owner controls are not every KMS schedule. t097/t100/t102. |
| GPU execution | Config GPU selection; Session `live_session/metadata_shell/gpu.rs`, `gpu/sysfs.rs`; runtime `supervisor/protection.rs` | Explicit render-node grant and bounded projection exist. Earlier frozen controls/reviews keep their original scope; native Lom rendering is observed in captures. No aggregate VRAM guarantee or toolkit requirement. t097/t103. |
| Geometry/popouts | r5 output facts/allocation/scale records; runtime `shell_content/allocations.rs`; Session content `popout_rect` | `shell_content_allocations.rs`: scaled bounds, exact presented parent, physical origin, timeout/topology invalidation. Not complete outside-dismissal/parent-loss acceptance. t099/t106; allowance/arbitration t038/t083. |
| Actions/input | r5 ContentAction/ACK, r6 indicators, r7 launcher input; Session `component_service/native_input.rs`, `component_catalog/actions.rs`, content action ledger; Engine presented capture | `support/content_actions/client_roundtrip_tests.rs`, `native_launcher_input_owner.rs`; runtime focus/deadline controls; C native lifecycle and Rust lifecycle. Supplied presentation/WM facts are not a physical chain. Launcher text is not general IME. t100/t106/t107/t111. |
| Metadata | r6 indicators, WM committed projection/output-launch contexts; Session `shell_indicator_projection.rs`, `launch_origin.rs`; descriptor/tab issuer/recipient actions | Rust/Nim indicator/origin controls and corpora. Workspace tokens are not broker window identities. An application catalog is not a running-window feed. t043/t038/t041. |
| Catalog launch | r4 descriptors, r7 transient, r8 persistent catalog; Session `component_catalog/{opening,execution}.rs`, `session_actions/native_catalog.rs`, authenticated owner-loop first-surface attribution | `native_catalog_publication.rs`, `persistent_catalog_queue.rs`, `support/launch_origin_socket.rs`; C Bemenu/Rust Provlita. Some fixtures supply process/surface admission. Logical output placement is not physical connector observation. t105–t108. |
| Session/output control | Control schema; config `shortcut_candidate.rs`; Session live control/WM dispatch; `sophia-protocol/src/ipc/output_v1.rs` | Existing operations include logout, app launch, switcher/help, reload and WM restart. Not generic power/audio/network authority. No declarative output schema exists despite codec/topology owners. t022/t023, external services t113, reload t037. |
| Notifications/transfers | `sophia-portal/src/{notification,clipboard,drag_and_drop,screen_capture,file_handoff,uri_open}.rs`; broker/namespace contracts and frontend clipboard are separate | `sophia-portal/tests/notification.rs` covers validation, approval/denial and source revocation. Scoped searches found no NotificationPortal/DeliverNotification production use in Session/runtime and no arbitrary native notification role. Reducer/chrome presence is partial, not end-to-end provider delivery. Native history/capture access is not implied by X clipboard. t046/t109. |
| External/security services | No service-specific audio/network/tray/authentication grants in shell r8; no idle/lock/IME/accessibility role schema found | Existing system services remain authorities; no unrestricted host bus bind. t113 service access, t046 prompts, t110 idle, t111 text, t112 accessibility, t034 lock. Absence is scoped to the native contract. |
| Background | Current allocations support panel/popout plus special launcher; no Background component in config | Rasterizing an image does not grant background placement or stacking. t049/t109. |
| SDK/compatibility | Rust `sophia-shell-client/src/{lib,lifecycle,outbox,catalog}.rs`; C wire header max revision 8, negotiation/native lifecycle/upload modules | `tools/check_shell_c_wire.sh`, role corpus, Nim Narthex and C Bemenu cover different subsets. Helpers grant no authority or feature parity. Old C guide's r1–6/no-Bemenu claims are stale. t023 needs independent complete lifecycles, not only codec agreement. |

## Gap disposition

- Protocol/admission: t109 additional providers; t022 output schema; t110 idle,
  t111 text/IME, t112 accessibility.
- Authorization/service integration: t034 lock, t043/t038 window disclosure and
  scoped actions, t046 portals, t113 external services. None follows from GPU or
  content permission.
- Production integration: existing t099/t100/t106 popout, topology/reconnect,
  shared component ownership. Reuse those owners. Broader target-resolved
  gesture contracts remain t040; scene-sampling effects remain t047/t048 under
  Engine authority. Neither discrete shell input nor a GPU grant supplies them.
- SDK/documentation: stale single-client/content/GPU/revision claims corrected
  with dated pointers; no new SDK project is required.
- Validation: t023 independent lifecycles, t107 fairness/reclamation, t081/t108
  physical acceptance. Missing acceptance does not make existing code absent.
- Client-only: styling, notification history UI, clock/weather, search/ranking,
  dock appearance and settings layout create no Sophia tasks without a missing
  generic primitive.

## Reconciliation and next implementation

Reuse t022 and retain its wider family exit: this inventory does not align all
WM/output transports or remove accidental forks. Keep t022/t023 and physical
tasks open. Reword t104–t108 around implemented slices and remaining proof,
without promoting candidates. Narrow t043 away from existing workspace
indicators. Expand t034/t046/t049 criteria rather than clone their tasks.

The [gap plan](../plans/1sxw3fyj-native-desktop-protocol-gaps-after-the-three-component-baseline.md)
owns uncovered candidates t109–t113. First existing implementation need is the
generic popout/lifecycle chain t099/t100/t106; first new foundation is t109.
No complete desktop UI is scheduled and no candidate is promoted by this audit.

## Validation and physical limits

Check documentation links/anchors, task uniqueness/dependencies, one lane per
row, zk indexing and diff whitespace. No build, canonical or hardware run is
performed for this audit. Prior exact 4049fea8 canonical PASS is separate.

Capture `.artifacts/lom-panel-native/20260918T170952Z` names 4049fea8, records
exit 0 and eight matching requested/committed logical outputs, and contains no
runtime_fatal/transport_failed/reconnected records. Terminals remained until
teardown and recorded unsuccessful exits. This does not pass the clean-child
exit gate, prove physical mapping, or establish latency/restart/daily-driver
acceptance. Earlier failed captures remain failed.
