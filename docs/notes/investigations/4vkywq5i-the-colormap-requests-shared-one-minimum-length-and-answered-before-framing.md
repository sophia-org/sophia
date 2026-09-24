---
id: 4vkywq5i
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, colormap, wire, xts]
---
# The colormap requests shared one minimum length and answered before framing

## Question

XTS5 declared fifteen colormap purposes against t169. Nine of them want
BadLength for a request one unit long or short and got BadAlloc,
BadAccess, BadColor or nothing; CopyColormapAndFree wanted nothing for a
valid request and BadIDChoice for a reused or foreign id and got
BadColor; ListInstalledColormaps wanted a reply and got BadRequest. What
does a TrueColor-only authority owe these?

## Evidence

Every colormap request but AllocColor, AllocNamedColor, QueryColors and
LookupColor was decoded by one `decode_colormap_request` that required
eight bytes and read the colormap id, into `ColormapRequest { kind }`,
and answered by kind: unknown colormap BadColor; AllocCells, AllocPlanes
and CopyAndFree BadAlloc; StoreColors and StoreNamedColor BadAccess;
Install, Uninstall and FreeColors nothing. A request one unit long
passed the shared minimum and got the semantic answer; a request one
unit short of a twelve-byte minimum passed it too. ListInstalledColormaps
(83) was not decoded at all: the coverage concept left it because it owes
a reply, and inventing a list seemed worse than not decoding it.

## Finding and resolution

Framing first, then the static answers, which stand. Each request has
its own framing (`decode_colormap_request` by kind, `wire/constants.rs`):
Install, Uninstall and ListInstalledColormaps exact eight; AllocColorCells
exact twelve; AllocColorPlanes exact sixteen; CopyColormapAndFree exact
twelve, the new id through `validate_new_resource_id`; FreeColors twelve
plus pixels; StoreColors eight plus twelve-byte items; StoreNamedColor
sixteen plus the padded name. Two typed requests: CopyColormapAndFree is
a new colormap on the source's visual, since a static visual has no
allocations to move (the same `create_colormap` CreateColormap uses;
BadColor for an unknown source, BadIDChoice for an id in use); and
ListInstalledColormaps replies the one installed colormap, the default,
which is not an invented list: the setup advertises one installed map at
most and at least, and GetWindowAttributes already reports every window's
installed. AllocCells and AllocPlanes stay BadAlloc, the Store requests
BadAccess (read-only cells), Install, Uninstall and FreeColors validated
no-ops on a colormap that is always installed and never allocated from.
No ColormapNotify: nothing changes when nothing can be installed.

`tests/x11_wire/colormap_static.rs`, both byte orders: each request one
unit off is BadLength before anything else, then the static answers;
CopyColormapAndFree answers nothing and the copy allocates, a reused id
and a foreign id are BadIDChoice; ListInstalledColormaps on the root
replies exactly the default, on an unknown window BadWindow. Red on
master, green after. The probe case `colormap_static_answers` says the
same for the core profile, and opcode 83 enters the inventory.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] XTS: the thirteen FAIL rows of t169 retire and nothing else moves
      (`.artifacts/xts-xproto/run-t169/`, 305 passed and 84 declared);
      StoreColors 1 and StoreNamedColor 1 stay as the suite's own
      UNSUPPORTED. The core profile reads PASS, 128 of 128, the new case
      in both byte orders.
- [x] The gate on the committed candidate
      (`.artifacts/x11-profile-91b18fd6-{selected-core,xproto}/`): both
      scenarios PASS, xproto with 305 passed and 84 declared.

## Connections

- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md) --
  the rows this closes.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  ListInstalledColormaps was the last undecoded colormap request.
