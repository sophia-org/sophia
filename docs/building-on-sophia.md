# Building on Sophia

This is the map for anyone who wants to build a desktop on the Sophia display
server — a lean tiling window manager in the dwm or niri tradition, a shell in
the Noctalia class, or a full desktop environment along the lines of XFCE or
COSMIC. It tells you which component owns what, which protocol each piece
speaks, and how the pieces fit together. Each section links to the document
that owns the details. This one owns the shape.

**Current interfaces and target:** the native role protocols described below
remain implemented and supported. The accepted
[9P2000.L direction](sophia-9p-control-bus.md) targets their progressive public
replacement, together with a separate 9P application frontend alongside X11.
Its file API remains design work. The goal is independent WMs, shells and
applications using generic 9P clients or mounted file I/O while retaining the
same admission, metadata and lifetime boundaries.

Panel UI is supplied by the user's chosen shell. Sophia does not add a built-in
workspace bar or reserve a fixed strip merely because a WM publishes indicators.
Engine validates and commits descriptors, composites pixels, and enforces input
and reservation ownership; the shell chooses its UI and placement.

Start with the [native desktop capability map](native-desktop-capabilities.md)
for the source-audited implementation status and remaining protocol gaps.
Sophia enables downstream desktop development; its roadmap does not require
shipping a complete desktop UI. Integrated and independent clients use the
same authority boundaries. Current modular admission covers three explicit
roles, not arbitrary additional providers.

Native shell reference sheets use revision 3's read-only shortcut catalog and
bounded presentation candidates. See [reference sheets](shell-reference-sheets.md)
for the generic wire, private shell configuration, and shared JetBrains Mono
presentation default. WM clients do not render this UI or receive its contents.

Descriptor application launchers use revision 4's catalog and presented
activation exchange; revision 7 adds a separately admitted client-rendered
launcher and revision 8 a persistent catalog consumer. Session owns source
policy and execution. The client owns search, ordering and its own raster;
Engine owns presentation and input authority (and descriptor drawing). The WM
receives the operation opening the menu, not catalog contents. See
[application launchers](application-launcher.md) and the capability map.

## The One Rule

Sophia doesn't divide the desktop by feature. It divides it by who may see
pixels.

- **Engine** owns the composed scene, its presentation, and access to foreign
  scene pixels. Clients may create and read their own content; reading another
  domain's pixels requires a portal grant.
- **Policy clients** — the window manager and the shell — decide what happens.
  They draw nothing, or they draw blind.
- **Portals** move data between confinement domains: one transfer at a time,
  brokered, with an identified recipient and the user's consent.

Every design question in this document resolves against that rule. A feature
that needs to read the screen belongs to Engine, or behind a portal decision.
A feature that needs application metadata is either refused or becomes a
portal the application opts into. There are no exceptions for convenience,
because every exception is exactly what the confinement exists to prevent.

Rendering has a corollary, the Compositing Operator Rule from
`docs/compositor-graphics.md`: Engine admits a drawing primitive only when the
client physically cannot perform the operation itself. A future content shell
rasterizes its own widgets. Engine does the blur, because blur reads pixels the
shell must never see. The confined descriptor tier cannot rasterize widgets;
Engine renders its fixed chrome from sanitized descriptors.

## Bring Your Own Language

Sophia's protocols are byte-level wire contracts over Unix sockets: a fixed
frame header, fixed offsets, explicit widths, reserved fields that must be
zero. There is no required SDK, no blessed binding, and no library you must
link. If your language can open a socket and read bytes, you can build on
Sophia.

This isn't an aspiration; it's how the existing clients work. Hagia and
Narthex are written in Nim and depend on nothing from this repository. The
archived window-manager client is 438 lines of plain C99, compiled directly
with no binding — the compatibility gate builds those exact sources and runs
them against the live server, so a wire change that breaks them is rejected
as a break, not absorbed as a refactor. The shell has its own independent C
client at 367 lines. Sophia's own codecs are Rust. Three languages already
speak the same bytes, and yours would be the fourth.

The protocol specifications live in `protocol/*.kdl` as language-neutral
descriptions: every message, field, width, and bound. The shared corpus of
golden frames, malformed frames, and fixed records gives you conformance
testing from the first day — your decoder either parses the same bytes the
Rust, C, and Nim decoders parse, or it doesn't, and no one has to take your
word for it either way.

## Application Frontends And Shell Toolkits

`sophia_shell_v1` is a native Sophia role protocol independent of the protocol
applications use. An X11 application connects to the X Server Frontend; a
native shell connects to its separately admitted shell endpoint. A future
Wayland or native application frontend could translate into Engine's existing
authority boundaries while the shell keeps speaking the same role protocol.
The [9P application frontend](sophia-9p-authority.md) is an accepted target but
remains separate implementation and acceptance work; X11 is today's application
path. Choosing 9P for both desktop roles and applications does not combine their
grants or make application content equivalent to shell content.

Lom is the driving content client, using a downstream Xilem/Masonry/Vello
adapter. Toolkit types, rendering integration and private configuration stay
downstream; they are not Sophia dependencies or public wire types. Other native
toolkits and languages must be able to build the same behavior from the
published protocol. Narthex remains the independent descriptor reference;
Quickshell and Noctalia remain sources of workflow and feasibility evidence.

The historical [reference-client audit](shell-reference-client-audit.md) starts
with one panel and one interactive popout. CPU content, direct GPU launch
admission and discrete input now have production paths; the capability map
distinguishes their tested scope from remaining lifecycle/native acceptance.
An ordinary X11 Quickshell panel
exercises the application frontend and does not acquire the native shell role.

### Rendering Your Own Shell

Implement the shell presentation lifecycle once: negotiate permitted features,
obtain exact allocation dimensions, render your own pixels, submit immutable
resources with matching targets, and handle pacing, outcomes and release.
Engine enables input only from the exact native-presented candidate. Your
toolkit objects, shader programs and widget state do not enter the public wire.
The optional Rust client library can help with this lifecycle; an independent
implementation needs no Sophia libraries.

Execution permission is separate. A CPU-rendered client needs no GPU grant;
Lom's chosen GPU path requests an explicit default-denied render-node permission
and reads its result back into the existing CPU-byte transport. The accepted
[execution decision](notes/decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
targets stock Linux without a custom kernel, hard aggregate VRAM guarantee,
mandatory GPU bridge or Vello dependency in Sophia. Direct GPU access accepts
driver/resource-availability risk and grants no foreign pixels, application
display, general input or KMS authority. Its launch path is implemented;
[configuration](configuration.md) describes the explicit operator selections.
That does not establish complete resource/recovery or daily-driver acceptance.

Measure the first image path before introducing another transport or GPU
execution service. A display-dependent toolkit may need a downstream adapter;
the shell contract does not supply a private X11/Wayland server. The
[paired plan](notes/plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md)
names the server and client tasks, so a developer can implement an adapter
without taking ownership of compositor policy or the entire desktop.

## The Components

A complete desktop is three processes beside Engine, each one independently
replaceable:

| Component | Protocol | Reference | May draw? | Sees |
| --- | --- | --- | --- | --- |
| Window manager | `sophia_wm_v1` (r3, frozen) | [Hagia](https://github.com/sophia-org/hagia) | no | geometry, window facts |
| Shell | `sophia_shell_v1` (r6, experimental; older negotiated capabilities supported) | [Narthex](https://github.com/sophia-org/narthex), [Lom](https://github.com/sophia-org/lom) | descriptors: Engine pixels; content: own images, production admission still closed | authorized presentation facts |
| Broker | `sophia_broker_v1` | in-tree | no | redacted descriptors |

The window manager never learns titles, application identities, or pixel
content. The current descriptor shell never learns surface identities or
coordinates: conformance evidence records
`surface_ids_disclosed=0 coordinates_disclosed=0`
on every run. The broker issues and revokes the opaque action capabilities
that let a shell activate a window it can't name.

This is the X11 process model — the server owns the display, the window
manager is just a client — plus the one thing X11 never had: the clients are
confined. Under X11, any client can walk the window tree. Under Wayland, any
layer-shell client can draw over your bank window. Here, neither is possible.
The macOS comparison is instructive too: macOS split WindowServer from
Dock.app but left no sanctioned seam for third-party window management, which
is why tiling tools there have to disable system protection to work at all.
`sophia_wm_v1` is that missing seam.

## Scripting And Live Control

[Scripting Sophia](scripting.md) defines the experimental `sophia msg` interface
for any conforming WM or shell. The session admits and authorizes callers,
routes commands to their responsible owner, and reports correlated outcomes.
WM action semantics remain in the WM; shell behavior stays within the shell
role. Neither client serves a scripting socket.

The CLI and Linux session endpoint implement discovery, registered argument-free
WM actions, and confirmed WM restart. Policy success follows Engine commit;
restart success follows the intended replacement's first usable commit.
`session { control "host-admin"; }` explicitly enables the startup-only listener;
the default is disabled. The [control v1 wire](sophia-control-v1.md) supports
independent clients with one outstanding request per stream and no automatic
mutation replay. Linux peer admission checks the session UID and pinned user,
mount, and PID namespaces. It excludes Sophia's protected roles without
claiming universal sandbox attestation. Desktop control grants no application
data access. Reload, delegated callers, and generic shell commands remain
unadvertised future work.

## The Ladder

**A window manager**, in the dwm, niri, or xmonad tradition. One binary
speaking `sophia_wm_v1`. The interface carries thirteen capability bits, from
`bindings` alone up to `translation_groups`; negotiate the ones you need and
ignore the rest. A minimal tiler is a reducer — snapshot in, projection out —
and you inherit the session's shell (or none), the portals, and the
compatibility layer for free. Start from `protocol/archive/sophia-wm-v1-r3/`,
which is self-contained: the frozen spec, a generated C codec, a worked
client, and checksums. Hagia is the full-width reference.
`docs/sophia-wm-api.md` and `docs/wm-v1-freeze-surface.md` own the details.

For smooth positional transitions, submit final placements with optional
[translation groups](window-transitions.md). Engine owns the GPU timeline,
frame pacing and presented input geometry; the WM retains camera and navigation
policy. Shell clients do not acquire rendering or animation authority.

Don't mistake this rung for a lesser desktop. What makes niri daily-drivable
isn't that the compositor does everything; it's that the environment around
the window manager is complete — clipboard, screenshots, screen capture,
notifications all work. Sophia gives the WM rung the same completeness
through portals and the session, and the window manager reaches them without
gaining an inch of authority: its keybindings map to opaque session-operation
slots. It asks for slot N; the session decides what slot N does; anything
that moves data gets a portal decision behind it. Hagia already spawns a
terminal, launches a browser, closes a window, and logs out this way. Lock,
screenshot, wallpaper, and audio ride the same pattern and are queued. The
target for Hagia plus Narthex is exactly this product: a complete,
daily-drivable, niri-class desktop where the WM owns policy, portals own
data, and nothing owns more than its job requires.

**A shell** — bar, launcher, switcher, notifications; the Noctalia class. One
binary speaking `sophia_shell_v1`, launched by the session into its own
protection domain. Revision 1 carries the descriptor switcher and bounded
work-area reservations: you order sanitized entries, Engine renders and captures
them, and you receive an opaque activation. Revision 2 adds persistent tab-group
descriptors for WM layouts. Narthex remains confined to descriptors; Engine owns
the bars' geometry, GPU rendering, and hit testing. A future content capability,
derived from a real shell's enumerated needs (`docs/sophia-shell-v1-direction.md`),
would let you rasterize your own widgets and hand over bounded content-addressed
textures for Engine to composite. It would still grant no screen reads.
The bar isn't a separate component — "shell-owned" covers a
small status strip and a full panel set alike. How your users configure it is
covered below in Two Configs, Two Owners — the short version is that your app's
settings are yours, and only the operator's envelope goes through Sophia.

**A desktop environment**, in the XFCE, COSMIC, or macOS tradition. This is
not a fourth protocol. A desktop decomposes into the pieces above, plus
portals, plus ordinary applications:

| Desktop feature | Where it lives |
| --- | --- |
| Panels, dock, launcher, OSD, lock, tray | shell |
| Workspace and window switcher | shell (descriptor capability) |
| Work-area reservation for panels | shell candidate, session-capped depth |
| Drag and drop, clipboard | portals |
| File handoff, URI open, notifications | portals |
| Screenshot, screen recording | portals |
| Desktop icons | shell surface launching through opaque actions |
| Settings and control centre | edit KDL; core config hot-reloads live, the desktop profile applies at session start. A GUI is an optional third-party editor |
| File manager | an ordinary application |
| Session save and restore | session authority, not the shell |
| Per-app menu bar | menu-export portal (below) |
| Panel plugin API | the shell's own affair, inside its domain |

One row deserves a word: a third-party plugin ABI isn't a Sophia concern.
Plugins run inside the shell's protection domain and share its authority. A
shell that loads plugins is trusting them with everything it has, and Sophia
neither knows nor cares. What Sophia guarantees is the blast radius — a rogue
plugin gets the shell's capability set, not the desktop.

### A Desktop Is a Composition, Not a Second Platform

On traditional Linux, the distance between a niri-class environment and a
KDE-class one is architectural. The desktop environment brings its own
compositor, its own session manager, its own config daemon, its own portal
backends — a parallel platform, not a window manager with additions. And the
real difference between the two was never the feature list; it's who does the
integrating. In a WM environment, the user assembles the parts and wires them
together. In a desktop environment, the project ships a tested-together whole:
one settings system every component reads, a GUI that writes it, changes that
propagate live, sessions that restore your applications and not just your
windows.

On Sophia, the architecture stops varying. Engine, session, portals, and
broker are fixed, and both a WM environment and a full desktop are
compositions of the same parts over the same wires. The integration glue that
makes a desktop feel whole — traditionally a soup of session bus daemons — is
the platform itself: the profile for configuration, session-operation slots
for actions, portals for data, the broker for metadata. Even the WM rung
inherits it, which is why that rung is complete rather than spartan.

The unified settings system is usually the deepest thing separating the two,
and Sophia already has the parts that matter. The desktop profile carries
seven typed authority sections — policy, shell, shortcut, session, input,
output, broker — with digests, validation, and a full prepare–activate–rollback
activation machine, model-checked in TLA+. Core configuration is live: the
session watches its file and reloads on change, waiting for input to fall idle,
revalidating, and applying atomically or keeping what runs. The desktop
profile's seven sections currently apply once, at session start. Its activation
reducer is built to accept a newer generation, so re-running it is not the
obstacle; the missing pieces are a watcher on the profile path and a live
re-handoff of the Policy authority to the already-running window manager, since
that authority is reached over the wire rather than settled inside the session.
That is a genuine increment, not a toggle — but every hard part, the
seven-authority prepare–activate–rollback machine, is done.

Either way the interface is the KDL file, not an application. Edit it by hand,
with `sed`, or with an editor someone writes. A settings GUI is therefore not
core work — it is an optional third-party application over a file format that
is already the interface, and it belongs outside this repository the same way
a shell backend does.

So the honest distance from the WM rung to a full desktop is a short list, in
rough dependency order: production content admission and input in
`sophia_shell_v1` (the wire and CPU lifecycle already exist), authorized status
feeds beyond the current workspace indicators,
application-session restore in the session authority, and a live
reload path for the full profile, which needs a profile-file watcher plus a
re-handoff of the Policy authority to the running window manager over the wire.
A theming story across applications is the one genuinely unsolved item, since
applications are protocol clients and their toolkits theme themselves, but
that is a hard problem every desktop shares rather than a Sophia gap. A
settings GUI is not on the list at all: the config file is the interface.

Which yields a sentence no other platform gets to write: on Sophia, a desktop
environment is a superset composition, not a second platform. Moving from
niri-class to KDE-class changes what you ship — never how it's wired — and
the climb is also the trust gradient: a complete confined desktop at the
bottom rung, more expressiveness for more granted trust above it.

## Two Configs, Two Owners

For the user's view of component selection, see
[Desktop composition](desktop-composition.md). The session-owned desktop
profile selects the WM, native shell, and login applications. Its preferred
user path is `sophia/desktop.kdl` under the configuration root; the legacy
`hagia/config.kdl` remains a fallback. Those selections belong to Session and
are excluded from the WM's staged Policy fragment. Installed executable paths
are launcher defaults, so a desktop profile can replace a component without
editing the session script.

A developer building on Sophia will want their users to configure the thing they
built. They can — and the config the user edits is the developer's own, not
Sophia's. There are two layers, owned by two parties, and keeping them straight
is the difference between an afternoon and a bad week.

**Your app's config is yours.** A shell's bar colors, widget choices, fonts,
module layout, behavior — none of it crosses into Sophia's authority, so Sophia
neither sees it nor imposes anything on it. Pick your own format, read your own
file, watch it and hot-reload it however you like. This is bring-your-own-config
to match bring-your-own-language: a shell written in Zig can read TOML its own
way. Sophia mandates a format only at the authority boundary.

**The profile is the operator's envelope.** The desktop profile — the seven
authority sections Sophia owns — is not where your users tune your app. It's
where the operator grants what your app is *allowed* to do: shell enabled, may
reserve up to N pixels of work area, these keybindings map to these
session-operation slots, input repeats this fast. Sophia validates it because it
crosses the whole session. Your app configures freely inside the envelope; the
envelope itself is granted, not claimed. Your config file may ask for a 40-pixel
panel, but the shell *requests* 40 through the reservation mechanism and Sophia
caps it at whatever the profile allowed. A user cannot, through your app's
settings, quietly grant your app more of the screen than the operator permitted
— which is the same property that stops your app drawing a phishing prompt.

The existing unified profile also carries a `policy` section as a transport for
WM-owned settings. Sophia preserves those ordered KDL records within the checked
envelope; it does not maintain the WM's setting vocabulary or interpret layouts,
workspace names, gaps, or scratchpad dimensions. The selected WM validates that
fragment before acknowledging activation. Adding a spatial-policy setting in
Hagia therefore needs no Sophia parser update. See [configuration](configuration.md)
for the distinction between envelope checks and WM semantic validation.

**An action is a request, not a thing your app does.** This is the seam every
developer arriving from X11 or Wayland gets wrong. When a user binds a key to
"launch a terminal," your app does not spawn the terminal. It asks Sophia
through an opaque session-operation slot, and the session decides what that slot
does. Your config expresses the intent — this key, that action — but the effect
routes through the authority that owns it. Same for anything that moves data: a
portal decision sits behind it. You express what the user wants; Sophia decides
whether and how it happens.

## The Menu-Export Portal

The per-application menu bar, macOS and Unity style, is the one classic
desktop feature that refuses to decompose into the table above. It needs
application metadata to flow to the shell — the menu tree of the focused
window — and that's precisely what `docs/sophia-policy-ipc.md` forbids the
shell from having.

Sophia's answer is neither to refuse the feature nor to open the boundary.
It's a portal. An application opts in to exporting its menu tree to an
identified shell, through the same brokered, consent-carrying mechanism as a
clipboard paste. An application that doesn't export simply has no global menu
and loses nothing else. The shell renders what was exported and dispatches
selections back as opaque actions.

This inverts the usual design. Every existing global-menu implementation has
the shell read the application: DBusMenu announces, the shell consumes, and
anything on the bus can watch. Here the application publishes to an identified
recipient, or nobody sees anything. It's the difference between a directory
the world can read and a letter with an addressee. And because it's the same
shape as every other portal, it needs no new protocol family.

Status: design direction, not yet specified. It would join the portal set in
`docs/namespaces-and-portals.md` as an eighth transfer kind.

## Why There's No Desktop Protocol

Protocol families here are cut along authority boundaries, never product
categories. "Desktop" is a product category. A `sophia_desktop_v1` would need
surfaces, work-area claims, and activation — everything `sophia_shell_v1`
needs — and the two would drift apart while third parties guessed which one
to implement. The spanning mechanism is capability negotiation inside a
single family, and it's proven: `sophia_wm_v1` carries a trivial tiler and
Hagia's full policy surface on the same frozen wire through optional capabilities.

The [content-shell contract](content-shell.md) makes the session's operator
policy the gate for content capability. Selecting a shell does not grant every
capability it requests. The grant is explicit, established at startup, and
recorded in effective-profile and launch evidence. The current `content` setting
expresses policy, but the production GPU gate still fails closed. Accepting a
new execution design does not enable an installed profile.

## Two Kinds of Shell, Named Honestly

Sophia has two architectural models for native shells. Descriptor mode is
implemented; the CPU content lifecycle is implemented in the same family while
production admission and discrete input remain incomplete.
Narthex remains the maintained descriptor reference, including its native
application launcher. Neither model is a requirement to use a particular toolkit.

| Developer choice | Descriptor shell today | Content shell contract |
| --- | --- | --- |
| Visual design | Supported ordering, selection, visibility, and appearance settings | Own widgets, typography, artwork, and internal layout |
| Drawing | Engine renders fixed feature vocabulary | Shell rasterizes its content; Engine validates and composites it |
| Input | Engine supplies exact presented actions and feature-specific input | Same authority, with separately admitted interaction extensions |
| Effects | Only the feature's admitted vocabulary | Own-content artwork plus negotiated Engine effects and transitions |
| Trust | Less freedom to misrepresent visual meaning | Arbitrary artwork requires trusting the shell's presentation more |

Suppose an Aurora developer wants a custom panel with an illustrated button and
an anchored popout. Aurora renders those widgets and proposes their content and
targets. Engine places and presents them, then sends an action for the exact
button the user activated. Aurora changes its local state and submits new
content. It need not translate its toolkit into an Engine widget tree.

Aurora can generate effects from its own artwork. A backdrop blur instead needs
foreign scene pixels, so it requests a supported Engine effect without receiving
those pixels. A novel scene-sampling effect needs separately trusted renderer
integration under the graphics contract, not a shader uploaded by the shell.
Engine retains transition timing and presentation scheduling.

Arbitrary artwork can imitate another interface or mislabel an action. Validating
a presented target does not prove that its label is honest. Descriptor constraints
reduce that freedom; they do not establish a general phishing-prevention claim.
Content permission still grants no foreign pixels, WM authority, or process
execution. A custom launcher therefore needs explicit identity and activation
semantics beyond the first panel/popout workflow.

One admitted native shell may combine both capabilities, for example a custom
panel with a descriptor launcher. This contract does not add
multiple native shell clients. See [Content Shells](content-shell.md) for the
behavioral contract and [desktop composition](desktop-composition.md) for the
user's component choices.

## Verification Culture

Anything that claims conformance can prove it, and nobody has to trust
anybody:

- The protocol corpus — golden frames, malformed frames, fixed records — is
  shared. Sophia's generated codecs and every independent client parse the
  same bytes. Rust, C, and Nim clients already pass it.
- Reference clients live in separate repositories with no Sophia build
  dependency, so a wire change that breaks them is a compatibility break, not
  a refactor.
- Physical proofs on real hardware bind the exact signed commit and binary
  digest of every component into an archived record. Your desktop can adopt
  that machinery or ignore it; the protocols don't care.

`docs/validation.md` owns the details.

## Where to Start

| You want to build | Read next | Copy from |
| --- | --- | --- |
| A window manager | `docs/sophia-wm-api.md`, `protocol/archive/sophia-wm-v1-r3/README.md` | the archived `client.c`, then Hagia |
| A shell | `docs/sophia-shell-v1-direction.md`, `protocol/sophia-shell-v1.kdl`, [paired plan](notes/plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md) | Narthex for descriptors; Lom for the developing content adapter |
| A full desktop | this document, then both of the above | Hagia and Narthex, as the split to imitate |
| Portal-using apps | `docs/namespaces-and-portals.md` | — |

The shell interface remains experimental. Revision 1 provides a switcher and
bounded reservations; revision 2 adds tabs, 3 reference sheets, 4 the launcher,
5 content vocabulary and 6 indicators. Available vocabulary is not a granted
workflow. The [implementation record](lom-content-implementation.md) names
existing boundaries and the production/acceptance gaps still open for content.

### Tabbed WM layouts

The [tabbed-layout protocol](tabbed-layouts.md) is an example of the descriptor
shell tier. A WM commits opaque group membership and bar geometry alongside its
layout projection. Sophia remaps those facts into sanitized, recipient-local
shell descriptors. The shell confirms its candidate through `sophia_shell_v1`;
Sophia renders and presents the bars through its normal GPU composition path.
Neither a private WM–shell channel nor application metadata in the WM is needed.
Richer raster content still requires a separate, explicitly negotiated capability.
