# Tabbed layouts

Hagia's `frame-tree`, `notion`, and `i3` (`split-tree`) layouts use the
`sophia_wm_v1` revision-3 extension lane and `sophia_shell_v1` revision 2.
The boundary follows [Building on Sophia](building-on-sophia.md): Hagia owns
private topology and selection, Narthex owns descriptor presentation, and Sophia
owns validation, geometry, rendering, input, and presentation. There is no
Hagia–Narthex socket and no application metadata in the WM protocol.

## Committed layout facts

WM capability `tab_groups` (bit 11) admits projection record kinds `0xff01`
and `0xff02`. Their fixed layouts are in the [generated wire reference](generated/sophia-wm-v1-wire.md).
They follow the complete ordinary chunk prefix, use consecutive ordinals, and
are excluded from revision-3 begin/end chunk and item counts. Existing revision-3
record sizes and message numbers remain unchanged.

A group names an opaque output, a WM-local group identity, a logical rectangle,
focus state, selected member, and ordered generational surface handles. The
transfer is bounded to 1,024 groups and 2,048 member occurrences. A surface may
represent a child in more than one ancestor group. Duplicate members within a
group, missing surfaces, cross-output placement, invalid selection, fullscreen
bars, and out-of-work-area geometry reject the entire proposal. Empty frame
cells have no selected member and no activation action. Groups are committed
with their client placements; a rejected proposal publishes neither.

## Descriptor protocol

The shell envelope and interface major stay at 1. A revision-1 shell continues
to receive messages 96–102. Revision 2 plus capability `tab_groups` (bit 2)
admits `TabsBegin` (103), `TabsGroup` (104), `TabsEntry` (105), `TabsEnd` (106),
and `TabsCandidate` (107). Work-area reservation capability bit 1 documents the
existing candidate edge and thickness fields; it is advertised by the server.

A tab transfer has one transaction, connection epoch, and snapshot generation.
`TabsBegin` declares aggregate group and entry counts. Each group is followed
by exactly its declared entries. Each entry embeds the existing single-entry
descriptor-snapshot payload, with its output and epochs validated against the
transfer. `TabsEnd` completes the transfer. Limits are 1,024 groups, 2,048
entries, and 128 UTF-8 bytes per sanitized label.

Narthex receives recipient-local group and occurrence slots, an opaque output,
selected slot, focus, sanitized labels, trust and attention, and opaque broker
actions. It receives no SurfaceIds, coordinates, icons, or application identity.
`TabsCandidate` confirms the exact ordered group slots from the current snapshot.
It cannot change membership, selection, or geometry. Existing candidate outcomes
and activation messages are multiplexed by transaction and candidate generation.
Tab actions persist across switcher opening and withdrawal.

Sophia enables an action only for the candidate whose commands have crossed the
presentation boundary. Layout, output, metadata, broker, WM connection, or shell
connection changes revoke its interaction. An activation acknowledgement is
resolved again against the current broker grant before Sophia enqueues WM focus.
Live tab and switcher candidate/activation work uses bounded nonblocking queues
and polling. The revision-1 synchronous transport facade remains available for
standalone clients and conformance hosts.

## Rendering and fallback

GPU composition is the default. Bars lower to ordinary compositor rectangle and
text commands, using the existing mixed-layer GPU renderer and text texture
cache. Font rasterization on a cache miss can use the CPU; that does not make the
bar a CPU-composited framebuffer. Visible bars prevent direct scanout. Fullscreen
projections suppress bars so ordinary scanout eligibility can return.

Bars occupy space inside the WM allocation rather than claiming another global
work-area reservation. They draw beneath client surfaces, including floating
windows. Pending or unavailable descriptors produce neutral, numbered,
noninteractive bars. Empty cells remain keyboard-addressable. The compositor
command bound is 10,240 to accommodate the bounded client and tab primitives.
Input currently treats client occlusion conservatively: an overlapping client
disables the entire affected tab target until it is uncovered.

The descriptor service currently runs through the legacy `LiveMetadataShell`
owner. Independent shell components disable that owner; their content and
indicator services do not yet include its tab descriptor exchange. A component
transport capability or a packaged descriptor client alone does not establish
that exchange. In a component desktop without it, the neutral, noninteractive
fallback above applies. The physical reference run below selects Narthex as a
separate shell configuration, rather than adding it beside the components.

This descriptor tier deliberately provides fixed Engine chrome. Rich shell
raster content uses a separate capability; no blind-content transport is implied
by revision 2.

## Verification and operator acceptance

Deterministic tests cover tree projection and cloning, tab transfer validation,
atomic policy commit, neutral chrome, shell supersession, activation, and stale
presentation rejection. `TabDescriptorPresentation.tla` checks generation and
capture lifetime; its two negative controls weaken candidate freshness and
shell-loss revocation independently.

Before declaring physical acceptance, run Hagia/Narthex on the operator's test
TTY and inspect all four names, multiple outputs, empty frames, nested tabs,
focus/resize/move, hidden-member clicks, shell restart, title changes, fullscreen,
and floating-window occlusion. Confirm native GPU composition, retirement, input
alignment, and recovery in the retained evidence. Offline checks do not prove
physical scanout behavior or advance the unrelated CP14 comparison rows.
This acceptance is owned by the CP-14.3 development-workflow checklist in
[the active roadmap](notes/indexes/plans.md); it requires no comparison run.
