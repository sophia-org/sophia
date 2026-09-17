---
id: faon4eja
date: 2026-09-17
kind: investigation
status: investigating
tags: [shell, native-components, protocol, validation]
---
# Native component ownership audit and reusable C wire boundary

## Scope

Source audit starts at published `89074209`. This records why a second shell
socket alone does not implement the approved Bemenu-plus-Lom design. No display,
device, installed profile, input branch or native owner was changed by this slice.
The [t104 proposal](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md)
remains proposed. This is not multi-client/runtime acceptance.

## Actual single-owner boundaries

| Owner and source | Observed assumption | Required join before enablement |
| --- | --- | --- |
| [Config components](../../../crates/sophia-config/src/types.rs), [parser](../../../crates/sophia-config/src/session_candidate.rs) | One `shell_client`/config and one set of shell permission fields | Validate the complete startup component list; normalize legacy single-shell; reject mixed forms, duplicates and exclusive launcher conflicts before spawning |
| [Session config](../../../crates/sophia-session/src/live_session/config.rs), [owner loop](../../../crates/sophia-session/src/live_session/owner_loop.rs) | One `Option<LiveMetadataShell>` drives input/content/launcher/recovery | Bounded component inventory; provider routing and fair visits; failure/recovery names one owner |
| [Metadata shell](../../../crates/sophia-session/src/live_session/metadata_shell.rs) and [launch](../../../crates/sophia-session/src/live_session/metadata_shell/launch.rs) | Connection counters, catalog, GPU policy, supervisor, transport and reservation coordinator all belong to that object | Retain per-component owners but mint connection identities from one Session issuer; no duplicated epoch tuple or role-global restart |
| [Role endpoint](../../../crates/sophia-runtime/src/policy_socket.rs) | `shell.sock` under an exclusively created private parent; protected peer required | Reuse the endpoint under a separate Session-created parent per component, exposing only that parent's endpoint to the child; no UID-only or client-selected component admission |
| [Transport negotiation](../../../crates/sophia-runtime/src/shell_transport.rs) | Requires descriptor bit 0 and automatically grants work-area bit 1; highest negotiated revision is 6 | Preserve legacy behavior for legacy peers. New launcher revision must admit its explicitly permitted capabilities without mandatory descriptor disclosure or automatic reservation permission |
| [Epoch pool](../../../crates/sophia-runtime/src/shell_content/epochs.rs) | `active: Option<ContentEpoch>`; each transport constructs a separate 64 MiB pool even though the type documents Session-wide accounting | Move the actual pool to Session; bound its active inventory and address stores by exact grant. Share byte/backing/dead-epoch accounting, not an aggregate counter disconnected from actual stores |
| [Content session](../../../crates/sophia-session/src/live_session/metadata_shell/content.rs) | One presented candidate per output, one action ledger, and allocation IDs owned by that shell | Keep per-grant semantics; projection must merge components rather than replace a neighbor's output state; focus and catalog actions must remain scoped |
| [Visual runtime](../../../crates/sophia-backend-live/src/production_visual_runtime.rs) and [intake](../../../crates/sophia-backend-live/src/production_visual_runtime/compositor_graphics/shell_content.rs) | `shell_content` is keyed only by OutputId; a later shell replaces the prior map entry; retirement debt records one grant per output | Extend the existing owner with component/grant-qualified content and all exact retirement claims represented by a composed native frame; neither numeric freshness nor another client settles an old claim |
| [Compositor identity](../../../crates/sophia-engine/src/compositor_graphics.rs) | Shell node identity contains output/candidate/surface/placement, no component or grant | Include authoritative component/grant scope before combining independently numbered candidates; retain it through lowerer caches, native metadata and presented input |
| [Descriptor launcher](../../../crates/sophia-session/src/live_session/metadata_shell/launcher.rs) | One opening/query/catalog worker and Engine-rendered menu; its busy/capture state lives inside the same shell | Reuse authorized catalog/execution policy; custom content needs its own negotiated presented-text/binding records, not reinterpretation of descriptor candidate or request-generation fields |
| [Input owner phase](../../../crates/sophia-session/src/live_session/owner_loop/physical_input_phase.rs) | One shell receives launcher input and indicator/content actions; launcher failure recovers that shell transport | Explicit provider selection, focus arbitration, exact client dispatch and isolated revocation. Keep WM-owned global shortcuts and current presented-transform rules |

These are source observations, not reproduced multi-client failures. In particular,
instantiating two current transports would reserve two independent pools, and the
current output map cannot represent two clients on the same logical output.

## Budget decision for the next contract revision

Keep the existing shared **64 MiB logical and 64 MiB backing ceilings**. The first
modular profile proposes the existing bar's 8/16/16 MiB staging/resident/retiring
maxima (40 MiB reservation), plus launcher 4/12/8 MiB (24 MiB reservation). Backing
reservation is resident+retiring: 32+20=52 MiB. Keep the existing 4 MiB per-resource
ceiling for both; the renderer must negotiate/clamp dimensions before allocation.
The Bemenu raster's separate 16 MiB local ceiling does not authorize a 16 MiB
upload.

Live reservation plus actual dead-epoch usage must remain under the same common
ceiling. With both complete reservations occupied, a replacement requiring the
same full reservation must wait until its prior epoch drains; a held old consumer
cannot be ignored to make a reconnect succeed. The peer's grant remains stable.
Unused reserved credit is accounting, not allocated pixels. Keep the 16 retained
epoch limit global, not 16 per new component. Failed setup returns its reservation
only after its exact owners are disposed; nothing here is a GPU VRAM quota.

This is a proposed profile, not an implemented budget change. The actual pool
refactor and accounting controls remain t105. It is now numerically possible to
admit the two intended components without multiplying the Session ceiling.

## Identity and negotiation decisions

Session should issue globally unique nonzero connection epochs within its live
session. Component names select configuration only and are not authorization.
The implementation plan now mints both connection and content-grant epochs
globally. This supersedes the initial local-grant-counter option: it preserves
the actual pool's strict two-field admission watermarks without a second replay
ledger. Compositor and retirement identities still
need the complete grant; connection minting alone does not repair output-only maps.

The existing descriptor-required revision-6 negotiation remains unchanged. A new
custom-launcher revision must remove automatic descriptor/reservation authority
for that mode. It must define a transient role (the current content validator
accepts only panel/popout), focus/text lease, atomic candidate-to-catalog binding,
activation and exact outcomes together. No packet IDs/bits are allocated by this
slice, and no new wire support is advertised. Welcome remains exactly 28 bytes
for revisions 1–6.

## Implemented C foundation

The public [C wire API](../../../bindings/c/README-shell.md) supplies allocation-free
bounded envelope I/O and typed revision-1–6 negotiation. It borrows a caller-owned
fd and two disjoint buffers, owns a queued frame through partial writes and holds
one received frame until explicit consumption. It does not create a socket,
perform admission, change fd flags, close the descriptor, infer a display or link
the Rust ABI. There is no message-payload or semantic lifecycle acceptance hidden
inside a successful envelope parse.

Real private socket controls cover one-byte fragmentation, retained incoming FIFO,
partial writes, mutation of the caller's original payload after enqueue, occupied
outbox refusal, actual kernel EAGAIN, truncation/EOF, oversize-before-body-read,
wrong direction and BrokenPipe without SIGPIPE. Wrapped-syscall controls cover
repeated EINTR and zero byte budget. The independent corpus reader checks 70
Rust-produced envelope round trips, with typed base hello/welcome checks only.
The 54-kind direction/transaction inventory is compared with the published KDL.
New C files have a 1,000-line ceiling, with a cohesion notice at 800.

The canonical `xtask check` and `tools/check_shell_protocol.sh` now call the bounded
C gate. `xtask check layout` remains unchanged and hardware-free. Full content
lifecycle, paired ACK/action reservation, negotiation state/replay, Session
admission, focus, native presentation and application execution remain open.

## Evidence

Retained outputs live under `.artifacts/shell-c-wire/`. The device-hidden C gate
passes: seven private-socket/codec groups, one mocked EINTR/storage group, 70
corpus frames and the 54-kind schema/layout inventory. The same gate passes with
Clang AddressSanitizer and UBSan, without disabling sanitizer categories. Four
separate compiled mutations fail their intended behavioral assertions: replacing
an occupied outbox, losing the partial-write offset, ignoring required capability
agreement and exceeding the syscall budget. Mutations affect only the disposable
snapshot; each source is restored afterward. These are author-run controls, not
an independent review. Exact-source canonical validation is recorded separately
once the signed checkpoint exists.
All private sockets in these tests are freshly created pairs. No live Session
endpoint, display, render node or input device is accessed.

## Subsequent ownership work

The [shared-store checkpoint](r3b9n7cf-native-component-storage-and-compositor-identity.md)
introduces an exact-grant two-owner registry and scopes compositor node identities.
The single-transport facade delegates its actual stores to that owner. Session
still constructs one facade per transport; moving the shared registry to Session
and joining multiple component projections remain outstanding. This record must
not be read as current two-client admission.
