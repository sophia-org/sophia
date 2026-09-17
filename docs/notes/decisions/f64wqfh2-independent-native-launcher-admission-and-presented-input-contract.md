---
id: f64wqfh2
date: 2026-09-16
kind: adr
status: proposed
tags: [adr, shell, native-components, launcher]
---
# Independent native launcher admission and presented input contract

## Context and implementation status

The operator approved implementing the Bemenu port and selected preservation of
its Cairo/Pango UI. That approves the implementation direction, not a claim that
the new wire contract is already shipped. Revision 6 still admits one native
shell; its content input supplies discrete actions, not editable text or general
keyboard events. The revision-4 descriptor launcher is a different product: Engine
renders and edits its menu. Neither may silently be reinterpreted as the custom
Bemenu content launcher.

This is the t104 contract proposal. Live negotiation remains capped at revision 6;
component profile parsing is now implemented with an explicit live-startup
refusal until independent admission is connected. The [revision-7 codec checkpoint](../investigations/v7m2c9ra-native-launcher-wire-contract.md)
now supplies concrete byte layouts and Rust/C compatibility controls; formal
models and live lifecycle integration remain pending;
t104 is not complete at this checkpoint. The [source ownership audit and budget
proposal](../investigations/faon4eja-native-component-ownership-audit-and-reusable-c-wire-boundary.md)
now identify the actual join points, including the per-transport epoch pool and
output-only compositor identity. C framing/negotiation is implemented separately.

## Decisions for the first implementation

### Independent admission

Session admits configured components into separate protection domains, endpoints,
connections and content stores. Component identity is assigned from the validated
operator profile; peers cannot choose another component by sending its name.
Session mints both connection and content-grant epochs globally and monotonically;
zero and exhaustion refuse admission. Every replacement gets a fresh identity. Resource, allocation,
candidate, focus and action identities are interpreted in that connection scope.
Lom's identity and grant do not change when the launcher exits or reconnects.

Support two configured components initially: one bar and one application launcher.
The existing single-shell setting normalizes to one component with its existing
permissions. Reject profiles combining that setting with the new list, duplicate
IDs, or conflicting exclusive launcher providers before launching either child.
Reject unsupported roles rather than silently selecting a winner. The complete
requested/implemented/operator-permitted intersection still controls capability
negotiation. Merely reaching an endpoint grants no GPU, keyboard or launch access.

WM configuration owns the open-launcher keybinding and requests the existing
launcher operation. Session resolves its explicitly selected provider. Neither
Bemenu nor Lom installs global shortcuts, starts the other, or acts as a broker.
Provider failure closes that opening; it does not launch an unselected fallback.

### Placement and focus

A launcher opening names the current authorized logical output and a fresh
opening identity. It receives a bounded transient content allocation on that
output, above ordinary shell panels, with no work-area reservation. It does not
learn unrelated client coordinates or surface identities. Session owns placement
and captures committed viewport/scale/generation with the presented binding.
Topology changes invalidate old input authority; stale pixels never permit
click-through into an underlying application.

Session issues keyboard/text authority only after the exact opening's content is
Presented. The lease names connection, opening, output/allocation generation and
presented input binding. Prepared, local menu selection and pending raster changes
cannot install targets or authorize application activation. A successor presents
atomically with its new binding; old pending action responses remain owned without
keeping the old targets active.

Deliver committed UTF-8 text and semantic commands (left/right/home/end,
backspace/delete, previous/next/page navigation and accept). Engine retains XKB
and compose handling. No raw device stream, global key observation, helper-based
clipboard, IME protocol or touch is implied. Escape is a Session cancellation;
outside dismissal consumes the initiating click. Closing, revocation, output loss
or replacement invalidates the lease before further dispatch. Restore focus only
to a still-authorized previous target; otherwise let existing WM policy choose.

### Catalog and activation

Reuse the authorized application catalog and Session launch dispatcher. Do not
send executable paths/argv to Bemenu or allow bemenu-run's PATH/exec mechanism in
this protection domain. Catalog labels/keywords populate upstream libbemenu;
opaque slot/generation identities stay attached to menu items.

Every content candidate's input binding must identify its catalog generation,
opening, ordered catalog rows and selected row. Validate those against the exact
catalog sent to this component. The presented binding is the authority for Enter
and row activation. A query edit disarms activation until the matching updated
content has presented; do not launch an unseen locally selected row. Rapid edits
can supersede unpresented work but not discard accepted response/resource owners.

An activation has one retained exact outcome and may enqueue the existing Session
launch policy once. ACK disposition and launch admission remain separate facts.
Stale catalog/opening/lease/row requests refuse without executing; a late ACK cannot
undo or replay an admitted launch. Revalidate launch policy/catalog identity at
the existing execution boundary, preserving current descriptor-launcher behavior.

### Bounds and service

First-profile bounds (to encode and enforce before runtime enablement):

- At most two live components and one active launcher opening/focus lease.
- Existing catalog limits remain: 4096 entries, 128-byte labels, 256-byte keywords
  and query, 32 visible rows. One committed-text record is at most 256 UTF-8 bytes;
  reject invalid UTF-8 and oversize payloads without truncation.
- Share 64 MiB logical and 64 MiB backing ceilings. Bar staging/resident/retiring
  maxima remain 8/16/16 MiB; launcher maxima are 4/12/8 MiB. Keep the 4 MiB
  per-resource cap and a common 16-owner retirement inventory, with a retirement
  slot reserved for each live grant.
- Reuse negotiated content limits per connection. Do not multiply an existing
  session retirement limit merely by admitting a second client. Admission must
  reserve the entire connection's possible dead-epoch footprint against the
  common session budget before launching; refuse a replacement while retained
  consumers prevent that reservation. Numeric default budget changes, if needed,
  require the shared ledger audit, not an implicit doubled allowance.
- Visit each component once per service round with at most 32 inbound records and
  64 KiB of payload work, bounded further by its negotiated limits. Rotate the
  starting component each round. Preserve partial records and reject declared
  oversize framing before allocation. GPU work and resource release cannot block
  control dispatch. Bound outbound bytes/records with existing owned reservations.
- Input/control credit is reserved before issuing a focus/input/action obligation.
  No drop-on-full or replay after partial write. A stalled client stops receiving
  new obligations; explicit timeout/revocation retains referenced resource owners.

The client raster's 16 MiB local ceiling is not Session accounting, native GPU
residency or an upload credit. Wire conversion must acquire tracked immutable
storage. Cairo's borrowed mutable pixels cannot survive repaint as a submitted
resource. Sophia continues GPU composition; this client needs no render-node grant.

### Protocol/configuration checkpoint

Revision-7 vocabulary allocates the native-launcher capability and kinds 187–197;
its runtime admission remains disabled. Existing
revision-4 launcher messages keep their meanings and existing peers keep their
wire layouts. Do not reinterpret kind 179 as keyboard data, forge a parent window
or make a private Bemenu socket. Endpoint admission supplies the component scope;
wire records still carry connection/opening/binding identities for stale checks.

The schema checkpoint defines lease grant/revoke, text/edit delivery and ACK,
content-to-catalog row binding and exact activation/outcome byte layouts together.
All new kinds/bits must be checked against the complete protocol inventory before
allocation, with golden frames decoded independently in C and Rust. Missing
capability means intentional absence, not fallback to an ambient X/Wayland socket.

Required audit owners: LiveMetadataShell and supervisor inventory; content stores
and aggregate outbox; indicator/catalog publication routing; endpoint peer check;
content allocation, presented projection and input capture; resource retirement;
Session launch policy and WM operation dispatch. Preserve their current actual
owners rather than introduce a competing resource/focus ledger in a test host.

### Typed component selection checkpoint

The Session authority accepts this configuration shape for validation/staging:

```kdl
shell { enabled #true; }
session {
  shell-component "panel" "bar" {
    executable "/opt/lom"
    config "/home/user/.config/lom/config.kdl"
    gpu "direct"
  }
  shell-component "menu" "application-launcher" {
    executable "/opt/bemenu-sophia"
    // GPU access defaults to denied independently for each component.
  }
}
```

This is not yet a runnable desktop example: live Session refuses it before
legacy process resolution or child launch, naming unimplemented revision-7
admission. Neither explicit nor default legacy shell-process flags override that
refusal. Reload retains existing non-launch settings and reports its existing
`non_launch_settings` deferral; it cannot activate the new selection.

Identity is 1–64 ASCII letters, digits, hyphens or underscores. At most two
components may be selected, with unique identities and unique roles; supported
roles are `bar` and `application-launcher`. Each requires one absolute executable
path; configuration is optional and absolute. Arguments, unknown settings,
duplicate settings, typed/named positional values, unsupported roles and mixed
legacy `shell-client`/`shell-config` selections refuse. Executable/configuration
existence and protection-domain admission remain launch-owner responsibilities.
Profile staging discloses these paths only to Session, never to the WM fragment.

The `gpu` setting is an operator selection (`denied` or `direct`), not a content
capability or effective device grant. There is no implicit inheritance between
components. The legacy fields and their current behavior are preserved; legacy
normalization into the future common live inventory is not implemented here.

## Controls and acceptance

Before runtime enablement, model conflicting provider admission, replacement
without epoch reuse, stale focus/input, held retirement across revoke and control
credit saturation. Exercise production admission with two private clients. A
launcher crash/replacement must retain Lom's grant, current presentation and action
progress while old launcher consumers remain charged. Test interrupted/partial I/O,
stale and cross-client identities, changed query before Enter, outside dismissal,
output loss, scale/negative origin and exact application launch at most once.

Preserve existing one-shell and descriptor-launcher conformance. Run C sanitizer,
layout, Rust affected and exact-source device-hidden gates. A supplied Presented
record is explicitly fixture evidence, not native completion.

Only a separately attended exact release can establish two-output placement,
keyboard focus, real app start/focus restoration, stable Lom, restart isolation,
clean logout and measured latency. This proposal supplies no invented latency
pass or permission to install/run a desktop.

## Alternatives and connections

Engine-rendered descriptor launcher remains available but does not preserve the
custom Bemenu UI. A display bridge or GTK backend is outside this port. Replacing
Cairo with a client GPU renderer now would add an unrelated port and permission
surface; that remains a measured later option.

Tracks [t104–t108](../plans/ptil1ejw-modular-native-shell-components-and-independent-launcher-critical-path.md)
and the [component concept](../concepts/k2d9l42p-native-shell-components-compose-through-explicit-scoped-grants.md).
Normative shipped behavior remains [shell_v1](../../../protocol/sophia-shell-v1.kdl)
and the [descriptor launcher](../../application-launcher.md); this proposed ADR
must not be cited as current multi-client support.

## Implementation sequence and attended workload

The approved implementation plan follows t104 contract, t105 independent Session
owners, t106 composition/focus, t107 C lifecycle plus `bemenu-sophia`, then t108
attended acceptance. Keep the existing descriptor launcher and single-shell mode.
The C client is a pinned, provenance-recorded source dependency, not a Rust ABI.
Bemenu keeps upstream placement/styling defaults within the allocation; Session
selects the active output and validates every visible catalog row.

The approved workload target is five warm-up interactions followed by forty
state-changing interactions, twenty per output on healthy outputs at least
60 Hz. Input issuance to causally matching native presentation must have p95 at
most 100 ms and maximum at most 250 ms. Use nearest-rank p95 over all forty
intended outcomes, and report each output separately. Missing/rejected intended
outcomes fail rather than disappearing from the sample. Retain kernel versus
observation-time provenance; unavailable exact timing is not a timing pass.
Application startup is measured separately. These are declared workload gates,
not hardware guarantees or evidence from an earlier run.

The first ownership implementation is recorded in
[the shared-store checkpoint](../investigations/r3b9n7cf-native-component-storage-and-compositor-identity.md).
It does not yet enable revision 7 or a second live component.
