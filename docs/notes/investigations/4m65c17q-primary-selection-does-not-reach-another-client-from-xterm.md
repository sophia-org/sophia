---
id: 4m65c17q
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, x11, session]
---
# Primary selection does not reach another client from xterm

## Question

Selecting text with the mouse in xterm and pasting it into another window does
not transfer it. Why?

## Evidence

Reported from a live Hagia session on release `0.1.0-84d906f148d6`, by hand:
select with the mouse in xterm, then middle-click or shift+Insert in a kitty
window. Nothing arrives.

Ruled out already: xterm's `ctrl+shift+c` and `ctrl+shift+v`. Stock xterm 411
binds neither. Its only selection bindings are `Shift <KeyPress> Insert` and
`~Ctrl ~Meta <Btn2Up>`, both `insert-selection(SELECT, CUT_BUFFER0)`, plus
mouse drag to PRIMARY and CUT_BUFFER0. So the earlier key-based attempt never
set CLIPBOARD and never reached Sophia. **The mouse path is different: it is
the ordinary PRIMARY transfer and it should work.**

Not yet established: whether the owner is recorded
(`SetSelectionOwner`), whether the requestor's `ConvertSelection` produces a
`SelectionRequest` to the owner, whether the owner's reply is routed back as
`SelectionNotify`, and whether the two clients being in different namespaces
sends the transfer through the clipboard portal
(`crates/sophia-x-authority/src/clipboard.rs`, which has
`SameNamespace`, `UnknownRequestorNamespace` and `Portal` refusal cases).
CUT_BUFFER0 is a property on the root window and is a second, independent path
worth checking separately.

The live session emitted no selection records, but the only one that exists,
`sophia_live_selection`, is written at teardown, so its absence says nothing.

## Validation and remaining work

- [ ] Reproduce between two clients and capture which request first fails,
      with `SOPHIA_X11_AUTHORITY_TRACE=1` for the dispatch record.
- [ ] Establish whether the failure is the same within one namespace and
      across two, which separates a routing defect from a portal policy one.
- [ ] Check CUT_BUFFER0 independently of the selection protocol.
- [ ] Add a real-client smoke once the mechanism is known; nothing in the
      current gates exercises a cross-client selection transfer.

Open work is tracked as t124 in `todo.md`.

## Connections

- [A kitty window waits out a four-second layout budget before it opens](cnbxdj48-a-kitty-window-waits-out-a-four-second-layout-budget-before-it-opens.md) --
  same session, and the same reading method.
