# Read-only shell reference sheets

`sophia_shell_v1` revision 3 adds shortcut catalogs and reference sheets. These
are generic native shell capabilities; they require neither X11, Qt, QML, nor
Quickshell. X11 remains the application frontend priority. Narthex implements
the reference client, while Hagia remains a blind window manager.

## Ownership

Session publishes the active shortcut profile as a bounded catalog: canonical
key/pointer chords, public action names, and optional user labels and groups.
It excludes application selectors, executable registrations, process identity,
window metadata, and invocation capabilities. Catalog slots are read-only row
identities. A shell cannot use one to invoke its named action.

The shell owns row labels, ordering, grouping, style, page requests, and startup
preference. Engine measures the text, computes geometry and page capacity,
renders through ordinary compositor commands, and ties input capture to the
exact retired output frame. No surface IDs, coordinates, textures, or graphics
API objects cross this extension. A sheet reserves no work area and submits no
WM transaction. Opening it preserves application focus and layout/camera state.

Compositor text uses the bundled **JetBrains Mono NL 2.304** by default. This is
the shared Sophia/Hagia/Narthex presentation default: Hagia and Narthex do not
load fonts or acquire a renderer. The normal live path composites the sheet on
the GPU using cached text textures and a one-pixel translucent-background
texture. The CPU fallback blends the same ARGB content for deterministic tests
and fallback presentation.

## Wire and lifetime

The [shell file schema](../protocol/sophia-shell-files-v1.kdl) defines the
Shortcuts object, ReferenceRequest and ReferenceOutcome events, and complete
ReferenceCandidate record. The [file contract](sophia-shell-files.md) retains
interface major 1 and revision-gated capabilities and limits. Socket frames
and counted catalog transfers have retired.

| Capability | Bit | Meaning |
| --- | --- | --- |
| `shortcut_catalog` | 3 | Complete active-binding facts, at most 256 entries |
| `reference_sheet` | 4 | Bounded read-only presentation candidates; requires the catalog |

The two capabilities are negotiated separately. The live reference-sheet
service requires both; catalog-only clients can receive facts without sheets.
The 256-entry bound is independent of the revision 1 16-descriptor bound.

| Kind | Direction | Message |
| --- | --- | --- |
| 108–110 | Session → shell | ShortcutsBegin, ShortcutsEntry, ShortcutsEnd |
| 111 | Session → shell | ReferenceRequest |
| 112 | Shell → session | ReferenceCandidate |
| 113 | Session → shell | ReferenceOutcome |

A catalog transfer shares a nonzero transaction, connection epoch and catalog
generation. It becomes usable only after the matching end, exact count, unique
nonzero slots, valid UTF-8, reserved-zero fields and bounds are validated.
Chords/keys and groups are bounded to 64 bytes; action names, labels and titles
to 128. Controls and bidi overrides/isolates are forbidden. Absent catalog
label/group values have zero length. Candidate keys and labels are nonempty.

Requests carry catalog, request, output and output-generation identities and
the last presented sheet epoch. Operations are startup=0, toggle=1, next=2,
previous=3 and dismiss=4. Candidates echo the request identities, supply a new
candidate generation and select unique catalog slots with shell-authored display
text. Style lengths are logical pixels; colors are straight-alpha ARGB. Only
the background may be translucent. Style bounds are enforced before projection.
There is no activation message or app capability in a reference candidate.

Only one request/candidate is in flight. Prepared=1 does not authorize capture.
Presented=2 reports the retired presentation epoch and Engine-clamped page and
page count. Rejected=3 and superseded=4 never update the shell's remembered page.
Input uses that presented identity. Pending operations coalesce; dismissal has
priority. The switcher supersedes the sheet, including an outstanding reply.
Catalog invalidation, shell failure, output changes, policy restart, VT/seat
release and input revocation withdraw the sheet and revoke capture. Consumed
presses retain their matching releases across withdrawal. A shell reconnect
does not repeat the startup request.

## Narthex behavior and configuration

Hagia's compiled default and the user's desktop binding use:

```kdl
shortcut {
    profile "daily"
    bind "Super+?" "session:shortcut-help" label="Show keyboard shortcuts" group="Session"
}
```

`Super+?`, `Super+Question`, `Super+Shift+/`, and `Super+Shift+slash` normalize
to the same physical chord. Defining two of them is a duplicate. Optional
`label` and `group` properties also work on `pointer-bind`; existing two-argument
bindings are unchanged.

Narthex's private file is `$XDG_CONFIG_HOME/narthex/config.kdl`, falling back to
`~/.config/narthex/config.kdl`. An explicit `SOPHIA_SHELL_CONFIG` selects another
file. Session mounts only that selected file read-only and passes its path;
it never parses shell vocabulary. No file means startup help is enabled.

```kdl
hotkey-overlay {
    skip-at-startup #false
}
```

Set the boolean to `#true` to disable the once-per-login display. Narthex groups
all configured bindings by purpose, preserves order within a group, and uses
optional labels or readable public-action names. There are no group headings,
search field, toolbar, or pagination footer. Page Up/Down and wheel input change
pages; the next ordinary key dismisses and is consumed. Modifier transitions
alone do not dismiss. Pointer input cannot activate windows through the sheet.
The emergency chord and VT routing retain precedence; the window-switcher chord
replaces help. Geometry stays fixed across pages of the same catalog/style/output.

## Reference and acceptance

Visual policy follows Triad `fb8fb27`, specifically
`src/daemon/hotkey_overlay_render.nim`: 24px padding, 10px row gap, 28px key/label
gap, 32px column gap, 48px screen margin, 14px body, 16px title, 4px square border,
and its six ARGB colors. JetBrains Mono replaces Triad's host-selected sans
font. Sophia's font rasterizer can produce different glyph-edge antialiasing.
Triad is a development comparison source, never a runtime/build dependency.

Regression coverage includes malformed and maximum-size wire transfers,
independent Nim encoding, real protected-socket startup/paging/dismissal,
alias/config validation, all 256 rows across fixed-size pages, consumed key
releases, wheel accumulation, exact retired frame identity with no hit targets,
and CPU text/alpha composition. A CPU preview can be generated with:

```sh
SOPHIA_REFERENCE_PREVIEW=/tmp/sophia-reference.ppm \
  cargo test --offline -p sophia-renderer-live --test reference_visual
```

Live acceptance remains one normal session: startup once, toggle, page, dismiss,
open the switcher, and verify unchanged terminal focus/camera and emergency
recovery. It does not reopen the 36-row comparison matrix. Sophia and Narthex
must be installed together; rebuild Hagia for the compiled binding and parser
support. Reloading only Hagia cannot install the Engine/protocol changes.

On 2026-09-06 the user reported the helper working as intended in the installed
session, alongside the Quickshell X11 panel. The retained source identities and
the subsequent desktop-composition work are recorded in the
[source note](notes/sources/2026-09/legacy-active-0001-2026-09-06-desktop-composition-belongs-to-the-session.md).
