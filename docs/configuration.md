# Sophia Configuration

**Role:** normative ownership, discovery, validation, and reload contract.

Sophia uses KDL 2 for user configuration. KDL 1 compatibility parsing,
includes, inheritance, and implicit multi-file merging are deliberately
unsupported.

## Files and ownership

The default user files are:

- `${XDG_CONFIG_HOME:-$HOME/.config}/sophia/config.kdl`
- `${XDG_CONFIG_HOME:-$HOME/.config}/sophia/wm.kdl`
- `${XDG_CONFIG_HOME:-$HOME/.config}/sophia/desktop.kdl`

`desktop.kdl` selects and configures the desktop through seven separate
authority sections. The [user guide](desktop-composition.md) covers component
selection, private settings, startup, and migration.

`config.kdl` belongs to the session and Engine/compositor mechanism. It owns
advanced application registrations, startup defaults, physical input source, XKB
RMLVO, repeat timing, output policy, namespace profile, external-WM launch
specification, diagnostic policy, and fallback compositor chrome plus hard
chrome limits. Cursor theme, nominal size, and semantic shape are compositor
mechanism too; they are not WM or shell rendering policy.

The compositor section accepts one bounded cursor selection:

```kdl
compositor {
    cursor theme="x11-core" size=16 shape="left_ptr"
}
```

`theme` is a 1–64 byte identifier, `size` is 1–128, and `shape` is one of the
public semantic roles (`left_ptr`, `text`, `pointer`, `move`, `wait`,
`crosshair`, or a supported resize role). The trusted session resolves the
selection once to immutable premultiplied pixels, dimensions, hotspot,
generation, and SHA-256 identity. CPU composition and the KMS cursor backend
consume that same asset; neither chooses a theme. A missing, malformed,
oversized, or over-deep Xcursor theme falls back visibly in diagnostics to the
built-in public-domain X11 core-font `left_ptr`. That built-in is the compiled
default and matches `XCreateFontCursor(XC_left_ptr)` rather than a
Sophia-specific drawing.

A desktop profile may select several registered startup identities in order:

```kdl
session {
    startup "terminal" "quickshell-panel"
}
```

The list accepts 0–32 distinct names and resolves them against the effective
Session registry. That registry includes profile commands, explicit advanced
core references, retained legacy registrations, and launcher defaults. Unknown
or aliased duplicate identities are rejected. An explicit empty `startup`
suppresses both core startup and launcher fallback. Session owns executable
paths and arguments; the WM never receives or launches them. Explicit
`--session-start=ID` selections
still override configuration. `--session-start-default=ID` is a launcher fallback
used only when neither the desktop profile nor the core registry selects a
startup list. The ordinary Hagia launcher uses this fallback for its terminal;
proof invocations retain explicit startup selections.

### Application commands beside bindings

The selected desktop profile supports literal executable/argument vectors:

```kdl
session {
    application "terminal" { exec "kitty"; }
    application "browser" { exec "brave-origin"; }
    application "work-browser" { use-core "brave-work"; }
    terminal "terminal"
    browser "browser"
}
shortcut {
    profile "desktop"
    bind "Super+Return" "session:spawn-terminal"
    bind "Super+b" { launch "browser"; }
    bind "Super+e" { exec "thunar"; }
    bind "Super+Shift+b" { launch "work-browser"; }
}
```

`exec` accepts a PATH basename or absolute executable and up to 32 literal
arguments, each at most 4096 bytes. It performs no shell expansion; use an
explicit `sh -c` command when shell evaluation is intended. Relative executable
paths and control characters are refused. At most 32 named and inline commands
may be declared together. Application names are bounded ASCII identities;
`__shortcut_` is reserved for trusted lowering.

A named application has exactly one `exec` or `use-core` body. `launch` resolves
only a declaration in this profile. `use-core` imports the complete named core
registration, including placement metadata; it does not merge fields from an
equal-name profile registration. Legacy terminal/browser/startup references
remain supported. Explicit CLI registrations replace complete specifications
and retain precedence. The launcher's `--session-app-default=ID=EXECUTABLE`
and `--session-action-default=terminal|browser=ID` supply absent defaults without
arguments specific to an application. Proof launchers keep explicit overrides.

Trusted preparation lowers inline commands into the Session fragment and
leaves only references in the Shortcut fragment. Policy contains neither.
The shortcut router uses a reserved, generation-scoped action range; policy
registrations cannot claim that range. A queued launch retains its complete
immutable command across reloads. Successful commands without placement metadata
release launch admission after spawning; they need not create a window.

Commands, role selections and bindings reload as one Session publication after
the physical key ledger becomes idle. Command-only edits reuse the accepted
policy configuration without restarting the WM or rebuilding output topology.
Launch generations are separate from policy connection/configuration identities.
Policy edits stage immutable files and wait for replacement configuration
acceptance; failed replacement restores the previous launch bundle and policy
paths. Core reload serializes with this preparation. Startup commands are never
replayed. Input, Shell, Broker and non-launch Session settings retain their
existing deferred behavior; Output changes use the topology transaction.

A registered session application may set `placement-class=N`, where `N` is a
nonzero opaque `u64`. For an action-launched application, the session attaches
that class only to the first newly observed surface. It never derives or sends a
title, app ID, PID, executable path, namespace, or match expression. The public
policy client may interpret the class for one admission; clients that do not
negotiate launch placement receive the exact pre-extension snapshot stream.

`wm.kdl` belongs only to a Sophia-native WM. It owns opaque action behavior,
bindings, workspace policy, native layout selection, timeout policy, and
active compositor-chrome preference. It cannot change input admission,
outputs, namespaces, executable registry entries, renderer or scanout policy,
or Engine hard limits.

No in-tree WM reads it. `sophia-wm-demo` lost its serving mode with the
experimental WM API v7, and its remaining subcommands are protocol proof
clients; Hagia is Sophia's native WM and speaks `sophia_wm_v1`. The `native`
and `standalone` tool profiles consequently run no window manager at all, and
a session without one registers no shortcuts, because shortcuts are resolved
against a policy client's configuration. Those sessions end when their
application does.

Native layout selection currently accepts:

- `layout "columns"` to divide the active work area among visible nodes and
  request matching client sizes;
- `layout "natural"` to preserve each node's current opaque allocation,
  constrain it to the work area, center it, and issue no policy resize.

`natural` is useful for single-purpose and diagnostic sessions but is not tied
to an application identity. Both layouts consume the same metadata-blind node
snapshot and remain subject to Engine constraint reconciliation and atomic
admission.

An external WM does not consume `wm.kdl`; it owns its native configuration and
speaks the blind, versioned `sophia_wm_v1` protocol directly. A WM never
overrides or mutates `config.kdl`. Sophia provides no legacy-WM compatibility
profile or synthetic X11 policy environment.

Sessions use the unified desktop profile at
`${XDG_CONFIG_HOME:-$HOME/.config}/sophia/desktop.kdl`. An explicit
`--desktop-profile=/absolute/path` wins, followed by the user's Sophia file,
the user's legacy `hagia/config.kdl`, `/etc/sophia/desktop.kdl`,
`/etc/hagia/config.kdl`, and the compiled profile. The installed launcher uses
its packaged profile as the last fallback. Discovery selects one source;
an invalid preferred source fails validation rather than selecting another.
Unlike Sophia's two
authority-local files, this source permits bounded top-level includes: depth
10, 64 files, and one MiB in aggregate, with owner/mode checks, cycle
detection, deterministic expansion, and per-value provenance.

The `policy` section transports ordered WM-owned KDL records. Sophia validates
the envelope, structural limits, and reserved Engine controls; the selected WM
owns setting names, layout names, value ranges, and duplicate-setting identities.
Repeated node names are preserved, so `view-name 1 "code"` and
`view-name 2 "web"` both reach Hagia. Sophia's `policy.<node-name>` labels are
descriptive, not unique WM setting keys. Encoded record order and contents survive
staging; staged-file provenance replaces the source-file provenance on reload.

`sophia config check --desktop-profile=...` reports
`policy_validation=delegated`: success validates the envelope, not WM semantics.
Use `sophia config print-policy --desktop-profile=...` to export a policy-only
profile for `hagia config check --config=...`. The Hagia TTY adapter checks
Sophia's envelope and passes only that exported policy to Hagia before
display-manager takeover. An explicitly selected different WM validates its
own vocabulary during protocol activation. Packaging also checks both.
Runtime still gives Hagia only its private
Policy fragment, and Hagia constructs a valid policy model before acknowledging
activation. Invalid values, duplicate WM settings, or unknown WM vocabulary keep
Sophia's graphical gate closed through the existing rejection/rollback path.
WM settings never grant renderer, scanout, input-admission, or session authority;
Sophia continues to validate the resulting proposals against Engine constraints.

The trusted session coordinator validates and partitions all seven desktop
authorities before constructing the graphical session. It stages owner-only
fragments with one generation and digest in the private policy runtime
directory and gives Hagia only the policy-fragment path. Hagia cannot read or
replace the shell, shortcut, session, input, output, or broker candidates.
The trusted side now also has a pure seven-authority activation reducer ported
from Hagia's exhaustively checked lifecycle model. One generation/digest must
prepare everywhere before activation effects are emitted, and promotion waits
for every matching activation completion. A prepare or activation failure
emits generation-wide rollback while retaining the active profile. Attempted
generations advance monotonically even after rejection, so delayed completion
cannot alias a retry.
An injected startup executor boundary converts each typed effect into its
matching authority call and returns the exact typed completion message. Its
prepare, activate, and rollback handlers borrow the seven public Hagia startup
owners. Six authorities settle through their authority-local participant
slots; Policy settles only after Hagia acknowledges the exact staged identity
over its private transport. The seam keeps filesystem or process work from
re-entering the reducer implicitly.
A synchronous startup driver now drains that boundary through the complete
prepare and activate barriers. The first failed operation cancels the
remaining phase, dispatches rollback to every authority, and retains the prior
active identity. Even when one rollback fails, every remaining rollback is
attempted and the exact unresolved recovery set is returned as an error.
The prepare barrier is also exposed as a separate typed startup driver. It
settles all seven prepare effects and returns either `Prepared` or `Rejected`
without emitting an activation effect. The existing full driver calls this same
function before requesting activation, so offline proofs and future
pre-graphical production wiring cannot drift into two implementations.
For public Hagia startup, Sophia invokes that prepare-only driver immediately
after staging and exact fragment admission and before display sockets, seats,
devices, or processes. The dispatcher has seven fixed authority fields that
borrow the separate policy, shell, shortcut, session, input, output, and broker
owners. Success retains the coordinator and every participant at the same
`Prepared` key with no active identity; any local failure rolls all seven slots
back and aborts startup. Before graphical construction, the launch gate then
activates the six local owners, starts Hagia with the exact owner-only Policy
fragment, and promotes the coordinator only after Hagia's matching completion.
Timeout, disconnect, identity rejection, or local failure rolls every owner
back and leaves the graphical gate closed. There is no prepared-only
`sophia_wm_v1` production branch; the former proof switch is a compatibility
no-op.
Each authority now has one shared pure participant model behind that contract.
It admits only a strictly newer generation and a different digest, makes exact
prepare, activate, and rollback retries idempotent, restores the exact previous
identity on rollback, and rejects a same-generation digest mismatch. The last
admitted full key remains bounded participant state after cleanup so an exact
retry cannot be confused with an identity collision. Because the synchronous
driver stops preparation at the first failure but rolls every authority back,
an idle authority consumes an exact unseen rollback key as a no-state
tombstone. That prevents local reuse of a generation it was never asked to
prepare while preserving its active identity.
Shortcut candidates are prepared into bounded typed chords before staging:
at most 256 key or pointer bindings, normalized modifier/key identities, no
duplicate chord, and no reserved Ctrl-Alt-Backspace override. Every target is
explicitly authority-qualified, for example `policy:switch-layout` or
`session:close-window`; pointer bindings cannot invoke session capabilities.
This preparation does not activate the candidate or grant an unavailable
capability.

The desktop profile recognizes `session:window-switcher` as a session-owned
action; it is not registered by the WM. The compiled profile enables the shell
and binds the action to `Super+P`. Core and admitted XI explicit pointer grabs
now participate in Engine's application lease arbitration, so an application
owner takes precedence over shell capture. The action is valid only in a
normal `sophia_wm_v1` session with the shell enabled.
`session.window-manager` accepts an absolute executable path and up to 31
string arguments. `session.shell-client` and `session.shell-config` each accept
one absolute path. These are Session-owned settings, never WM policy. Explicit
`--wm-process` and `--shell-process` selections win over the profile; the profile
wins over core `external-wm` and the launcher's `--wm-process-default` and
`--shell-process-default`. Explicit WM replacement discards arguments from the
replaced selection. The final compatibility fallback looks for `narthex` beside
the absolute WM executable.

The session launches the native shell with `--serve` in a metadata-shell Bubblewrap
domain. It receives sanitized descriptors and opaque actions, never
application identities or raw input. `SOPHIA_SHELL_CONFIG` overrides the profile's
private shell file selection. Explicit selections require an existing file;
the session mounts only that file and does not parse its contents. The inherited
Narthex setup retains its optional `narthex/config.kdl` default. An explicitly
selected replacement does not inherit Narthex's private configuration.
Packaging records separate hashes for `hagia` and `narthex`.

`sophia config print-effective --desktop-profile=...` shows the parsed component
and startup choices before launcher overrides. `print-component` with
`--component=window-manager` or `--component=shell-client` prints just the
profile selection (or core `external-wm` fallback), or nothing when it must
come from the launcher. These are offline
inspection commands; neither grants admission nor starts a process. Component
and startup changes apply at the next login. WM reload retains the current
process selections and never replays login applications.

`shell { panel N; }` tells the shell to request an exclusive strip of work area,
`N` pixels deep, along the bottom edge of the output it presents on. The intended
authority contract requires Session to enforce the configured depth. Absent or
zero means no reservation, which is what every profile written before the key
existed requests. A depth beyond the
`sophia_shell_v1` reservation maximum is refused when the profile is read
rather than when the shell first claims, and a panel with no enabled shell is
refused outright rather than ignored. The compiled profile makes no reservation.

Session also passes this value through `SOPHIA_SHELL_BAR_THICKNESS` as a private
hint, but treats the profile as authoritative: allocation admission refuses a
panel deeper than the configured allowance, including every nonzero panel when
the allowance is absent or zero. [The admission investigation](notes/investigations/gl2ooa99-shell-reservation-admission-ignores-the-configured-panel-depth.md)
records the repaired boundary.

The accepted [replacement design](notes/decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
uses independent `content #true` and `gpu "direct"` choices, with GPU access
defaulting to `gpu "denied"`. The parser accepts those values and rejects the
retired `gpu-memory-bytes` form with an actionable migration diagnostic rather
than silently weakening a requested quota. Content may remain CPU-rendered with
`gpu "denied"`; direct GPU permission also requires an enabled shell process.

`content-input #true` separately admits the bounded discrete-input workflow for
presented shell targets. It defaults to false and is invalid unless `content
#true` is also set. This permission does not grant pointer coordinates, motion,
keyboard input, application identity, or an ambient toolkit input stream. Engine
selects a target from the exact native-presented content candidate, captures the
press/release sequence, and sends only the target-bound action identity. The
shell acknowledges that action and may echo its already-published opaque
indicator action; Session reports acceptance only after the WM admits the
operation. This keeps drawing, input admission, and WM authority as separate
profile and lifecycle decisions.

Direct mode exposes exactly the selected render node under its kernel
`/dev/dri/renderD<minor>` basename and binds its kernel identity to the shell
connection epoch. It also supplies a generated read-only sysfs view containing
only that render minor's bounded Linux PCI discovery facts; it does not mount
host `/sys` or the whole physical-device directory. Lom matches the selected
Vulkan adapter back to the granted render `dev_t`. It requires no custom kernel
or `dmem` controller. It accepts GPU
execution/availability risk without a hard aggregate VRAM guarantee. The
content ledger bounds known immutable resource and compositor-backing credit;
it does not measure every driver allocation. Native GPU acceptance remains a
separately authorized hardware test.
The [launch task](notes/plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t097)
owns that migration, device identity and actual startup evidence.

The claim rides on the shell's candidate rather than a request of its own:
Engine admits it against the realized output topology, and it reduces the work
area only when the candidate's pixels present, so the strip and the windows
that clear it commit together. Withdrawal is a later candidate that reserves
nothing, through the same path. Losing the shell connection retains the
presented claim beside the retained pixels — the work area does not grow while
nothing can present into the strip — and a reconnected shell re-claims at its
fresh epoch. The descriptor claim lives for as long as the switcher is visible.
The content path now owns independent panel allocations, resources, candidates,
work-area bands, composition and native-retirement outcomes within the same
admitted native shell. Production admission is still closed by the GPU-domain
gate, and semantic content input remains pending. The original delivery gap is
recorded in
[indicator delivery does not yet provide native panel presentation](notes/investigations/wjctvtsk-indicator-delivery-does-not-yet-provide-native-panel-presentation.md).
The desktop-profile form of `config check` runs the same typed shortcut,
session, input, and output preparation used by graphical startup; it performs
no device discovery or activation.
That preparation boundary first verifies that all seven authority candidates
match the profile's exact generation and digest, including authorities whose
payload is consumed by another process. The startup loader returns the raw
provenance-bearing profile, exact activation key, and derived bundle together,
so it does not validate and then immediately prepare the same values again.
Live-session assembly partitions that bundle once: session, input, and output
move into typed owner records backed by their authority-local slots, while the
shortcut payload is retained only until transfer to the public shortcut owner.
No consumer reparses the source profile. This is candidate selection only, not
authority activation.
An authority-local fragment loader now admits the staged format through the
same absolute-path, regular-file, one-MiB, owner, and mode checks as other
trusted configuration. It rejects symlinks, malformed or duplicate markers,
multiple/crossed authority sections, duplicate settings, and any generation or
digest that differs from the coordinator's exact activation key. The result is
the existing provenance-bearing raw candidate DTO; loading still performs no
activation.
One generic authority-local candidate slot now couples that DTO (or a prepared
typed candidate) to exactly one participant model. The slot is the sole owner
of active, previous-active, and candidate payloads; its pure prepare, activate,
and rollback functions delegate identity changes to the participant reducer.
Exact retries compare semantic payload while ignoring staging-path provenance,
but changed settings at the same key fail closed. Fragment admission or payload
rejection returns without mutating either identity or payload state.
The trusted session authority now has one deterministic preparation seam for
its effective application configuration. It parses explicit CLI application,
argument, startup, and action selections into a bounded immutable overlay,
then applies that overlay and the canonical typed session candidate to a clone
of the trusted application registry. The ordering preserves CLI superiority,
and any unknown, duplicate, ambiguous, or over-limit reference rejects the
clone without changing accepted state. This preparation is retained admission
data. The launch reload transaction uses the same preparation before publishing
the effective registry and shortcut router together.
Startup now places the canonical typed session payload in the generic
session-owned slot before deriving that effective configuration. The retained
slot is exactly `Prepared`, advertises the profile's activation key, and is
checked against the canonical bundle in tests. It is intentionally not
activated by configuration assembly.
The generic slot exposes one `with_candidate` constructor so authority owners
do not repeat `new` plus `prepare` sequencing. The public shortcut owner now
uses the same constructor, retains its typed shortcut payload in `Prepared`
state, and resolves action registrations from the slot payload instead of
bypassing participant state. Shortcut installation is still outside the global
activation barrier and is not claimed as active profile state.
Input and output now likewise have cohesive typed owner records around their
prepared slots. Keyboard/pointer overlays and output reconciliation read those
owners' candidate payloads. The transient typed bundle is discarded after
partitioning, so startup does not keep a second coordinator-owned copy.
For a public Hagia session, trusted startup next creates an owner-only policy
launch context before display sockets, seats, input/output setup, or process
launch. It stages the complete profile, re-admits all seven fragments through
the owner-safe loader against the exact activation key, and retains named raw
policy, shell, and broker owners plus the prepared shortcut owner. The
session, input, and output owners remain in their typed configuration records.
The prepare-only coordinator borrows those seven records, settles the complete
barrier, and stores its `Prepared` model before startup can continue. Any
failure rolls every owner back and aborts before graphical startup; dropping an
unused context removes its files and directory. Public policy launch later
consumes that exact context rather than staging again.
When native scanout is requested, startup additionally projects the already
owned DRM capabilities and reconciles the output candidate before launching a
graphical client. A second pure boundary converts the validated reconciliation
into a stable-`OutputId` activation plan carrying exact requested and rollback
states, the shared generation/digest, and optional startup focus. It rejects
capability drift between projection and planning and exposes no connector,
CRTC, property, or file-descriptor handle. This remains admission only: it
performs no atomic test, output apply, or candidate activation.
The next authority-local boundary is also present as a pure coordinator. It
emits typed test, apply, and rollback effects and accepts only completions for
the exact profile generation and digest in the expected phase. Test rejection
discards the candidate without rollback; an apply failure cannot settle until
rollback succeeds or reports a terminal recovery failure. No executor is wired
to these effects yet, so startup behavior remains unchanged.
The production public-policy session invokes this driver and the participant
transitions before graphical construction. Authority-local state may become
active while the synchronous batch is settling, but no graphical consumer is
admitted until every authority has activated the same key; failure rolls all
participants back before launch. This identity promotion does not itself apply
an output modeset or install a watched reload source.
The synchronous startup barrier remains distinct from live reload. An explicit
`session:reload-profile` action applies the bounded launch/shortcut transaction
above and existing policy/output mechanisms. It does not reactivate all seven
authority participants. The startup Session participant retains its original
payload; a separate exact source snapshot identifies the applied launch slice.
Core application preparation overlays only that slice's commands and roles.
There is no automatic desktop-file watcher or general cross-authority live
visibility barrier.

## Host scripting access

In the desktop profile, `session { control "host-admin"; }` enables the
experimental session control socket. `control "disabled"` is the default;
other strings and non-string values are rejected. This admission setting is
startup-only, so changing it requires a new session. The CLI and inherited
environment cannot enable control.

The session needs a private, owned `XDG_RUNTIME_DIR` and Linux peer-pidfd and
namespace inspection support. If prerequisites fail, it logs control as
disabled and continues desktop startup. It exports `SOPHIA_CONTROL_SOCKET` to
host applications for `sophia msg commands`, `sophia msg policy 'NAME'`, and
`sophia msg session restart-wm`. See [scripting](scripting.md) for the precise
host trust boundary. Profile reload is not a scripting command yet.

## Discovery

Each domain resolves exactly one source at startup:

1. an explicit absolute `--config=PATH` or `--wm-config=PATH`;
2. the corresponding XDG user file;
3. `/etc/sophia/config.kdl` or `/etc/sophia/wm.kdl`;
4. the compiled default.

Sophia does not rediscover a different source during reload. Deleting,
renaming, or making the resolved file unreadable retains the last-known-good
snapshot. Recreating that same path makes it eligible again.

File-backed configuration must be an absolute, regular file no larger than
one MiB. On Unix it must be owned by the effective user or root and must not be
group- or world-writable.

## Validation and transactions

Both files require `schema 2`. Schema 1 chrome syntax is rejected with a
targeted migration diagnostic. Parsers reject unknown nodes and properties,
duplicate singleton nodes and properties, type annotations, invalid IDs,
unbounded strings or vectors, invalid cross-references, unsafe paths, and the
reserved Ctrl-Alt-Backspace emergency chord. A SHA-256 digest identifies the
exact accepted bytes.

Reload watches the resolved parent directory, so atomic editor replacement is
supported. Events are quiet-debounced for 100 ms and forced after one second
of continuous change. Parsing and validation produce an immutable candidate
before any live state changes.

Core reload is whole-file atomic:

- application registry changes affect later launches;
- repeat timing applies only after the shortcut/key ledger is idle;
- fallback chrome and diagnostics apply live;
- input source, XKB, outputs, namespace, cursor asset, and external-WM launch
  changes mark the entire candidate `pending_restart`;
- a pending-restart candidate does not partially apply its otherwise-live
  fields.

Application reload preparation uses the retained CLI overlay and the active
desktop session selections. A prepared profile is eligible only before a
session profile has activated; a staged replacement never supersedes active
selections. The candidate core snapshot, resolved launch table and local command router
publish together. Core and desktop launch preparations cannot overlap. An invalid resolved reference rejects the reload while preserving the
active snapshot, launch table, and any earlier pending-restart candidate.
Reload never replays startup applications.

The active native WM chrome preference wins while that WM is healthy. Engine
still validates its geometry and renders/damages the chrome. An external WM
does not advertise chrome-policy ownership, so it uses `config.kdl`. A failed
or restarting WM retains the last complete visual chrome and geometry until a
replacement policy can relayout atomically.

### Mirrored outputs

A named desktop output may list physical mirror members and choose how its one
logical scene maps to unequal native modes:

```kdl
output {
  named "DP-1" {
    mode "2560x1440@60"
    mirror "DP-2"
    mirror-fit "fit"
  }
}
```

`mirror-fit` accepts `fit`, `cover`, or `exact`. `fit` is the default and keeps
the complete logical scene visible with explicit bars when aspect ratios differ.
`cover` fills the head and clips overflow. `exact` performs no scaling and
centres the logical scene, clipping it when necessary. Configuration owns this
operator choice; the target architecture has Engine normalize it and derive one
immutable transform per head, while the backend only executes those plans. The
current backend projection is transitional and must not become a second layout
authority.

Chrome has two explicit roles. A focus ring is painted only for the focused
managed surface. A frame may be painted for every managed surface with
focused and unfocused colors. Engine reserves one stable clearance equal to
the maximum enabled width, then derives client content geometry by insetting
the WM's outer allocation. Focus and color changes repaint without resizing.
A clearance change is prepared through the ordinary atomic resize path; the
old complete style and client geometry remain visible until every matching
client buffer is ready.

The prepared desktop profile carries one generation and bounded
focus-ring/frame policy. Shortcut and policy authorities prepare that
generation before activation; Engine swaps the immutable chrome and shortcut
state only with the coordinated profile activation. Public policy snapshots
then carry the active configuration without a private reload frame or
Engine-owned workspace reducer.

## Commands

Validate the discovered files without starting a graphical session:

```sh
cargo run --offline -q -p sophia-cli -- config check
cargo run --offline -q -p sophia-cli -- config check --wm
cargo run --offline -q -p sophia-cli -- config check \
    --desktop-profile=/absolute/path/to/hagia/config.kdl
```

Inspect the parsed, default-expanded snapshots:

```sh
cargo run --offline -q -p sophia-cli -- config print-effective
cargo run --offline -q -p sophia-cli -- config print-effective --wm
```

Use `--config=/absolute/path` for the core domain and
`--wm-config=/absolute/path --wm` for the native-WM domain. The example files
are [config.kdl](../examples/config.kdl) and [wm.kdl](../examples/wm.kdl).

## Guarded native hot-reload proof

From a logged-in TTY 3, run:

```sh
tools/start_sophia_native_hot_reload_tty3.sh
```

For an installed release, select `Sophia Native Chrome Proof` in greetd. It
uses the same sequence without a checkout or build and automatically reserves,
finalizes, and checksums a commit-pinned archive. After normal logout, run
`sophia-verify-native-chrome` from a text session.

The launcher uses a private runtime `wm.kdl`; it does not modify the user's
default configuration. After two Kitty surfaces are visible, it advances a
2-pixel ring to 6 pixels, rejects an invalid edit, rejects deletion while
retaining the last-known-good state, applies a 4-pixel frame-only policy, then
applies a 2-pixel ring with a 6-pixel focused/unfocused frame. Each width
change must cross a matching two-surface resize epoch before the reduced
chrome-set observation can advance. Intermediate retired frames remain visible
for three seconds so the physical proof can be inspected instead of merely
logged.

The proof retains a commit-bearing ordered sequence log. Validate its verifier
logic without physical hardware with:

```sh
tools/check_sophia_native_chrome_verifier.sh
```

## Application catalogs

### Client launch adapters

`sophia client-launch` adapts a registered client's command line before replacing
itself with that client. The Chromium adapter queries the current local X display
using its MIT cookie, opens the default DRI3 device, and resolves its DRM render
node by kernel identity. It verifies the reopened node before supplying
`--render-node-override`. Discovery has a one-second deadline; missing authority,
DRI3, device evidence, or ambiguous device identity refuses the launch. It does
not select a GPU by enumeration order or vendor name.

```sh
sophia client-launch --adapter=chromium --argv-style=direct -- /path/to/brave
sophia client-launch --adapter=chromium --argv-style=wrapper -- brave-origin --
```

`wrapper` is for Go-style launchers that **consume** their first `--` before
executing the browser. Launcher options precede that separator; browser options
follow it. The adapter inserts its switch before any browser-owned `--`, preserves
explicit `--render-node-override=PATH` and `--hardware-video-device-path=PATH`, and
refuses conflicting physical-device selections. The switches require `=PATH`;
duplicates and empty values are refused. Paths and arguments are passed directly
to `exec`, without a shell. Discovery descriptors close before exec, which
preserves the launch PID and process group.

Add `--check-only` before the command separator to report the selected render
node and adaptation status without launching anything. Diagnostics omit browser
arguments, URLs and authentication data. Device verification applies at launch;
the adapter cannot preserve a device through later hot-unplug or redirect a
browser instance that was already running.

Register the helper as the application's executable to use it from both a
desktop-profile binding and a registered catalog entry. For example, replacing
the existing Brave registration (adjust absolute paths for the installation):

```kdl
application "brave-origin" id=5 executable="/usr/local/bin/sophia" {
    arg "client-launch"
    arg "--adapter=chromium"
    arg "--argv-style=wrapper"
    arg "--"
    arg "/home/you/.local/bin/brave-origin"
    arg "--"
}
```

Add `application "brave-origin"` to the selected `application-catalog` to expose
that registration there. Install a binary supporting the helper **before**
activating this recipe. It preserves the wrapper's normal update behavior.
Unregistered desktop entries and arbitrary terminal commands retain their own
launch recipes. This adapter belongs to the CLI; Engine, the renderer, the blind
WM and the X11 contract contain no Chromium-specific device policy.

### Catalog selection

Core `session` blocks may define named `application-catalog` records with an
explicit `launch-policy="trusted-host"`, approved absolute source directories,
registered application names and an optional terminal adapter. Desktop profiles
select a catalog by name and bind `session:application-launcher`. Catalog policy
and selection take effect at the next login. See
[application launcher](application-launcher.md) for the complete configuration.
