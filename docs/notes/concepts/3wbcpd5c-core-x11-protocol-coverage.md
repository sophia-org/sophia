---
id: 3wbcpd5c
date: 2026-09-19
kind: concept
status: draft
tags: [concept, x11]
---
# Core X11 protocol coverage

What the X authority decodes of the core protocol's 127 request opcodes, and
what it does not. Ninety-seven are decoded. Every request that affects drawing
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

| Op | Request | What it is for | Who calls it |
| --- | --- | --- | --- |
| 6 | ChangeSaveSet | Reparenting window managers preserve a client's windows if the manager dies | Only a reparenting WM. Sophia's WM does not reparent. |
| 11 | UnmapSubwindows | Unmap every child at once | Rare; toolkits unmap individually |
| 13 | CirculateWindow | Raise the bottom child or lower the top | Legacy stacking idiom, largely unused |
| 114 | RotateProperties | Rotate a window's property values in place | Almost nothing; an ICCCM curiosity |

### Pointer and keyboard

| Op | Request | What it is for | Who calls it |
| --- | --- | --- | --- |
| 30 | ChangeActivePointerGrab | Change the event mask or cursor of a grab in progress | Drag-and-drop implementations mid-drag |
| 39 | GetMotionEvents | Read the server's motion history buffer | Tablet and gesture code wanting sub-frame motion |
| 41 | WarpPointer | Move the pointer programmatically | Games, pointer-lock emulation, some installers |
| 44 | QueryKeymap | The whole keyboard state as a bit vector | Toolkits checking modifiers without an event |
| 100 | ChangeKeyboardMapping | Rewrite keycode to keysym mappings | `xmodmap`, remapping tools |
| 102 | ChangeKeyboardControl | Bell, key click, auto-repeat, LEDs | `xset` |
| 105, 106 | Change/GetPointerControl | Pointer acceleration and threshold | `xset m` |
| 116 | SetPointerMapping | Reorder or disable buttons | Left-handed mouse configuration |
| 118 | SetModifierMapping | Which keycodes act as which modifiers | `xmodmap`, keyboard layout tools |

Sophia owns input through its own authority and Engine routes it, so several
of these would have to be answered by policy rather than served literally.
That is the decision the task below is for.

### Screen saver, hosts and access control

| Op | Request | What it is for | Who calls it |
| --- | --- | --- | --- |
| 107, 108 | Set/GetScreenSaver | The server's own blanking timer | `xset s`, screensaver daemons |
| 115 | ForceScreenSaver | Blank or unblank now | Lock screens |
| 109, 110 | ChangeHosts, ListHosts | The host-based access list | `xhost` |
| 111 | SetAccessControl | Enable or disable that list | `xhost +` |

Host-based access control is a security mechanism Sophia deliberately does not
have: admission is by namespace and peer credentials. Serving these would mean
either lying about an access list or exposing one that decides nothing. The
honest answers are probably a protocol error and an empty list, but that is a
policy choice, not an implementation detail.

### Connection lifetime

| Op | Request | What it is for | Who calls it |
| --- | --- | --- | --- |
| 112 | SetCloseDownMode | Whether a client's resources outlive it | Session managers, `xsm` |
| 113 | KillClient | Destroy another client's resources | `xkill`, window managers closing an unresponsive window |

`KillClient` is the one with real teeth: a window manager uses it when a
client ignores `WM_DELETE_WINDOW`, so a desktop without it cannot force a
window closed. It is also the one whose authority question is sharpest, since
it lets one client destroy another's resources.

### Colormaps

| Op | Request | What it is for | Who calls it |
| --- | --- | --- | --- |
| 83 | ListInstalledColormaps | Which colormaps are installed on a screen | Colormap-aware toolkits on pseudo-colour displays |

The rest of the colormap family is decoded and answered as a TrueColor visual
must. This one is left because it owes a *reply* rather than an answer, and
inventing a list is worse than not decoding it.

### Unassigned

120 through 126 are unassigned in the core protocol and 127 is NoOperation,
which is decoded.

## What this does not cover

Extension requests are a separate surface with their own coverage. The
compatibility matrix in `docs/x11-compatibility-matrix.md` states what is
proven for both, and `tools/probes/x11_conformance/manifest.json` carries the
per-opcode ledger the conformance gate enforces.

## Connections

- [Serve core fonts from a session-configured host path](../decisions/n520o0bl-serve-core-fonts-from-a-session-configured-host-path.md)
