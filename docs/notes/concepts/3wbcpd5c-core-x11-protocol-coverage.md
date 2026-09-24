---
id: 3wbcpd5c
date: 2026-09-19
kind: concept
status: draft
tags: [concept, x11]
---
# Core X11 protocol coverage

What the X authority decodes of the core protocol's 127 request opcodes, and
what it does not. Ninety-nine are decoded. Every request that affects drawing
is among them; the thirty that remain are listed below with what each is for
and who calls it, so the next person deciding whether to implement one is
deciding rather than discovering.

## The rule this follows

An opcode the authority does not decode becomes `BadRequest`. That is not a
neutral outcome: Xlib's default handler prints and returns, but a client may
install its own, and xterm's exits the process. So "not implemented" is a
choice about whether a client survives meeting it, and several requests below
are decoded precisely so the answer can be a proper protocol error instead.

## Not decoded

### Window and hierarchy

All decided (t166): ChangeSaveSet, UnmapSubwindows, CirculateWindow and
RotateProperties are in the table below.

### Pointer and keyboard

All decided (t166): QueryKeymap, ChangeKeyboardMapping, SetPointerMapping
and SetModifierMapping are in the table below. Sophia owns input through
its own authority and Engine routes it, so each is served as far as that
authority can honestly report it, and no further.

### Screen saver, hosts and access control

All decided (t166): the screen saver and the pointer and keyboard controls
are advisory state, and the host list is empty, enabled and unchangeable.
See the decided table below.

### Connection lifetime

Both decided (t166): SetCloseDownMode and KillClient are in the table
below. `KillClient` is the one with real teeth: a window manager uses it
when a client ignores `WM_DELETE_WINDOW`, so a desktop without it cannot
force a window closed.

### Colormaps

The whole colormap family is decoded and answered as a TrueColor visual
must; ListInstalledColormaps, the last of it, is in the decided table below.

### Unassigned

120 through 126 are unassigned in the core protocol and 127 is NoOperation,
which is decoded.

## Decided and now decoded

Two of the thirty were decided by running XTS5 against the fixture host,
which is the use that made the question concrete rather than theoretical.

| Op | Request | Decision | Why |
| --- | --- | --- | --- |
| 41 | WarpPointer | Serve it | Every XTS test's harness positions the pointer with it, and a client that asks for the pointer to move means it. The move happens and `QueryPointer` agrees. |
| 115 | ForceScreenSaver | Serve it, as a no-op that validates | Every XTS test's startup calls `XResetScreenSaver`. This authority blanks nothing and keeps no idle timer, so both defined modes are accepted and move no state, and a mode outside the pair is the Value error the protocol names. |
| 6 | ChangeSaveSet | Serve it | A client saves another's windows, never its own (BadMatch); when it departs, each saved window still alive is given to its nearest ancestor outside the departed range (the root when none) and re-mapped if it was mapped, with UnmapNotify, ReparentNotify (21, new) and MapNotify to whoever selected on it, rather than destroyed with the departed client's subtree. |
| 112 | SetCloseDownMode | Serve it | Destroy is the teardown as it was. RetainPermanent and RetainTemporary keep the departed client's resource range alive -- windows mapped, properties in place -- until a KillClient names one of its resources, or AllTemporary for a temporary one; selections end with the connection either way, as the reference ends them. Ranges are never reused, so a retained one is unambiguous. |
| 113 | KillClient | Serve it | The owner of the named resource is disconnected, and its own teardown frees its resources under its own close-down mode; a resource in a retained range frees that range now; AllTemporary frees every retained temporary range; a resource nobody holds is BadValue carrying it. |
| 116 | SetPointerMapping | Serve it | The list is validated (nine entries, no logical button named twice, else BadValue carrying the offender), refused Busy while a button whose entry changes is held, and otherwise stored per namespace and applied where the routing maps a physical button: the physical button is what is held, the logical one what a client receives, and a zero entry is held and delivered to nobody. GetPointerMapping reports it, and MappingNotify(Pointer) reaches every client of the namespace, the requester first. |
| 100 | ChangeKeyboardMapping | Serve it as an overlay | A per-namespace keycode-to-keysym table starts as the compiled XKB keymap's and is rewritten by the request (a keycode outside min..max is BadValue carrying it); GetKeyboardMapping and XKB GetMap both report the table, so the two views agree, and MappingNotify(Keyboard) names the first keycode and count. What it does not change: xkbcommon's compiled keymap still drives the modifier state and keysym of each key event, so a remapped key is reported remapped and delivered as compiled. That is the overlay's honest limit, recorded here rather than hidden. |
| 118 | SetModifierMapping | Serve the current map, refuse another | The request is normalised into eight keycode sets and compared with the compiled keymap's modifier map (which GetModifierMapping now reads instead of a literal): equal answers Success and MappingNotify(Modifier); different answers Failed and no event, since xkbcommon owns the modifier state events carry and a map it did not compile cannot be served. A keycode outside min..max is BadValue. |
| 44 | QueryKeymap | Serve it | The thirty-two bytes are the keys this routing has seen go down and not yet up, per namespace, written at the routed and private key transitions; a repeat is not a transition. |
| 11 | UnmapSubwindows | Serve it | Every mapped child, top to bottom in stacking order as the protocol orders it, each through the path UnmapWindow takes with its UnmapNotify; children already unmapped are skipped, since an event for them would report a transition that never happened. |
| 13 | CirculateWindow | Serve it, without Expose | The lowest child occluded by a sibling above it goes to the top (RaiseLowest), the highest occluding one to the bottom (LowerHighest), occlusion being the siblings' rectangles meeting; a CirculateNotify to the child and its parent's SubstructureNotify selectors. A client selecting SubstructureRedirect on the parent is asked instead, with a CirculateRequest naming the child, as a map becomes a MapRequest. No Expose: windows are retained surfaces here, and raising one uncovers nothing that was lost; XTS5's CirculateWindow purposes pass without it. |
| 30 | ChangeActivePointerGrab | Serve the mask, validate the cursor | The event mask of an active grab the requester holds is rewritten; without such a grab the request has no effect, as the protocol says. The cursor is validated (BadCursor) and not applied: cursor display is config-driven here, the same debt WarpPointer carries. A bit outside the pointer events is BadValue carrying the mask. |
| 114 | RotateProperties | Serve it | The named properties' values move delta places along the list, a PropertyNotify per moved value; a property missing or named twice moves nothing (BadMatch), an atom nobody interned is BadAtom, an Engine-owned property refuses (BadAccess) as ChangeProperty does. |
| 39 | GetMotionEvents | Serve it, with no events | This authority keeps no motion history, which the protocol allows: a valid window gets an empty reply, an unknown one BadWindow. |
| 102, 103 | Change/GetKeyboardControl | Serve them as advisory state | What a client sets it reads back -- bell, click, LEDs, repeat flags -- validated as the protocol validates it (an unused mask bit and an out-of-range value are BadValue, a led without a mode BadMatch), and acted on by nothing here: the session owns key repeat and no bell rings. `xset` sees a server that keeps its word. |
| 105, 106 | Change/GetPointerControl | Serve them as advisory state | Acceleration and threshold, stored and read back, a zero denominator BadValue; the Engine owns the pointer's acceleration, so nothing moves differently. |
| 107, 108 | Set/GetScreenSaver | Serve them as advisory state | Timings and modes stored and read back, -1 restoring a default, a mode outside its set BadValue; nothing here blanks a screen, and ForceScreenSaver stays the validating no-op it was. |
| 109, 110, 111 | ChangeHosts, ListHosts, SetAccessControl | Report an empty list, refuse changes | Host-based access control is not a mechanism this authority has: admission is by namespace and peer credentials. ListHosts replies no hosts with access control enabled, and ChangeHosts and SetAccessControl are BadAccess, the protocol's error for a client not authorised to change the list. `xhost` sees a locked-down server. The three are decoded with their exact framing so a short or long request is BadLength first. |
| 83 | ListInstalledColormaps | Serve it | The reply is the one installed colormap, the default, which is not an invented list: the setup advertises one installed map at most and at least, and GetWindowAttributes already reports every window's colormap installed. With it (t169) the rest of the family got the protocol's own framing, so a request one unit off is BadLength before it is anything else, and CopyColormapAndFree became a new colormap on the source's visual, a static visual having no allocations to move. |

**What WarpPointer does not do**, recorded because the gap is real rather
than hypothetical: a warp must generate motion and crossing events as if
the user had moved the pointer, and it does not. There is no path in this
authority from a request to the input fan-out, which only real input
drives, and the conformance host opens no session, registers no surface and
configures no injector. Building one is a new synthetic input origin and
the same design question XTEST parks behind an injection policy. The
position moves, the events do not, and the manifest's coverage entry says
so. Nothing in the selected XTS scenario reads those events; the tests that
would (`Xlib11` MotionNotify, EnterNotify, LeaveNotify) are not selected and
could not pass here anyway.

The measurement that drove both: before opcode 115, all 58 purposes of the
selected scenario were UNRESOLVED and nothing had run. After it, 31 passed.
After WarpPointer, the three purposes that had died in the harness reached
their own assertions and now fail on focus reversion, which is a real gap of
its own rather than a missing request.

## What this does not cover

Extension requests are a separate surface with their own coverage. The
compatibility matrix in `docs/x11-compatibility-matrix.md` states what is
proven for both, and `tools/probes/x11_conformance/manifest.json` carries the
per-opcode ledger the conformance gate enforces.

## Connections

- [Serve core fonts from a session-configured host path](../decisions/n520o0bl-serve-core-fonts-from-a-session-configured-host-path.md)
