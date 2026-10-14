---
id: ikvvl6an
date: 2026-10-14
kind: investigation
status: closed
tags: [investigation]
---
# Review device negotiation gaps before architecture acceptance

## Question

Which remaining t069 obligations belong to Sophia, which depend on clients,
and which must precede optimization? This is t329's review at `96d945b76`.

## Current resumption

The review is complete; universal device negotiation is not. Retain t069's
normal-client and physical device-loss exits. The startup bundle lifetime repair
is present. Lost-generation admission, indefinite capacity retry, all-or-nothing
seat opening and identity-preserving resets still need bounded decisions or
experiments. Do not represent a browser's pre-submission allocation failure as
an Engine import defect. No new device access or browser acceptance was attempted.

Before t289, settle the lost-generation admission and retry outcomes and prove
that failed secondary-device opening cannot silently replace a valid inventory.
Physical reset/recovery remains a separately controlled gate, never an ordinary
test. Proposed repairs require their own bounded scope; this review does not
promote all of t069 or authorize a GPU reset in the live session.

## Evidence

Read the [t069 contract and retained checkpoints](../plans/6tewvlbh-universal-device-negotiation-across-sophia-clients.md),
including the September 25 D1 repair and the controlled no-override comparison.
The historical browser comparison and its 903-frame override control remain
evidence about their recorded builds, not current browser versions.

Current source boundaries:

| Boundary | Disposition | Next bounded experiment or decision |
| --- | --- | --- |
| Advertisement and connection identity | Immutable bundles remain in `crates/sophia-x-authority/src/device_bundle.rs` and `x11_socket/device_bundles.rs`. Installation rejects stale generations, unavailable bundles and capability changes. The configured bundle is taken once by `install_configured_device_bundle`, preserving the D1 repair. | Reuse `tests/device_bundle_config_lifetime.rs` and its retained mutation evidence. Do not reopen D1 merely because t069 remains open. |
| Admission during device loss (D2) | Still present: `X11DeviceBundles::current` returns the latest bundle without an availability check; `pin_connection_device` pins it. Resource access later checks availability. This preserves old connection identity but leaves a newly admitted connection unable to open that device. | Admit a connection after loss and before replacement. Specify a bounded refusal or clearly unavailable capability state for new connections, while preserving existing connection pins. Prove replacement affects only new connections. |
| Generation-capacity retry | `crates/sophia-session/src/live_session/render_devices.rs::poll_install` retains a prepared candidate after `BundleError::Capacity` and retries after RETRY. Individual acknowledgement waits have deadlines; overall capacity retry has no deadline or terminal record. | Hold all generation slots, advance a fake clock and observe retry state. Define a reported deferred outcome and bounded resource retention, then release a slot and prove exactly one installation. Do not require a session crash merely to bound waiting. |
| Seat-wide opening | `crates/sophia-backend-live/src/drm/render_inventory.rs::discover_render_devices_on_cards` collects a `Result` across every selected card; one failed open rejects discovery. The worker propagates this. In contrast, `render_devices/worker.rs::prepare_replacement` tries later compatible devices after preparation failure. | Inject one failed secondary open beside a healthy admitted default. Decide whether to retain the last valid inventory or publish an explicitly partial one; prove no invalid device becomes advertised. Keep admission and identity checks mandatory. |
| Reset without identity change | Inventory snapshots contain path, device/inode/dev_t and physical-device identity, explicitly not a liveness guarantee. Comparing those snapshots alone cannot detect a reset that preserves them. | First use an injected renderer/context failure with unchanged identity to trace invalidation and retained resources. Specify recovery before considering a separately authorized physical reset. No claim that every driver reset is currently unhandled follows from identity comparison alone. |
| Failure propagation | `render_devices.rs::poll_losses` and `poll_install` return errors on timeout, disconnected acknowledgements and non-capacity refusal. These remain failure edges, not transparent device recovery. | Exercise each with fake service endpoints and classify whether the caller must contain, reconnect or terminate. Preserve exact stale-generation and late-result controls. |
| Allocation, import and presentation | The t069 contract distinguishes measured import formats from allocation/export capabilities; exact incoming descriptors are tried at the destination, with bounded cross-device transfer and source lifetime. Accepted XR24 copied-Present advice is opt-in and generation-bound. | Reuse the named pixel, transfer and signaling evidence. Any changed implementation needs descriptor, pixel, synchronization and final-release controls; overlap in advertised modifiers alone is insufficient. |
| Client-internal allocation | The retained Chromium failure occurs before submission. Server import/transfer cannot repair a buffer it never receives. DRI3 hints are advisory under the current contract. | Reproduce with an exact current client/build on explicit devices, recording whether a descriptor reaches Sophia. Keep the upstream client fix and temporary launch adapter separate from generic server acceptance. No supported-application registry or executable-name policy enters Engine. |

## Finding and resolution

The pre-optimization concern is bounded, observable server behavior during
loss and replacement, not a promise to control every client's allocator.
Prioritize D2 and capacity outcomes, then the partial-inventory decision and
service-error containment. Existing tests already cover retained candidates,
stale preparation, late acknowledgements, worker deadlines and trying a later
compatible device; extend those seams instead of starting a hardware campaign.

The normal no-override client exit and physical loss/recovery remain open under
t069. Neither a passing source audit nor an adapted browser closes them. An
upstream client change should not indefinitely block unrelated 9P architecture
work when Sophia's own refusal and resource boundaries are explicit.

## Validation and remaining work

Read `crates/sophia-session/tests/support/render_device_coordinator.rs` and
`crates/sophia-x-authority/tests/device_bundle_config_lifetime.rs`. No tests were
rerun and no new physical acceptance is claimed. This review supplies t329's
inventory, proposed experiments and ordering recommendation; implementation
status remains in the task ledger.

## Connections

- [t329 exit](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t329).
- [t069 source contract](../plans/6tewvlbh-universal-device-negotiation-across-sophia-clients.md).
- [t328 delivery audit](jxcqbd04-audit-remaining-topology-and-delivery-obligations-before-architecture-acceptance.md)
  records separate route-lease and queued-geometry obligations.
