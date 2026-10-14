---
id: lv26isrv
date: 2026-09-07
kind: investigation
status: investigating
tags: [investigation, x11, rendering]
---
# Application cursor shapes do not follow hyperlink hover

## Question

Why does the pointer fail to change to the application's hand cursor when
hovering over a hyperlink?

## Evidence

On 2026-09-07, the user reported that the live desktop does not show the hand or
link pointer over hyperlinks. The report followed Brave testing in installed
Sophia `a44d1e163ceb7188edfebefc9871e1517576f07b`. This records a visible symptom;
it does not establish whether the client requested the shape, the frontend
published it, or the Engine selected and presented it.

## Investigation boundary

The [X11 readiness record](../plans/queue-11-parallel-production-readiness.md#t059)
already identifies authority-to-engine cursor plumbing as a limitation shared
by XFIXES cursor operations and stored RENDER ARGB cursors. That is a starting
point for investigation, not a confirmed diagnosis of this report.

Trace application cursor selection through frontend state and Engine cursor
presentation. X11 cursor semantics belong in the frontend; physical cursor
selection and rendering belong in the Engine. The WM should not infer cursor
shapes from application identity or pointer position.

## Acceptance

On 2026-10-14, niltempus promoted this work before Engine optimization after
the [Kitty protocol review](u399qgtv-kitty-protocols-as-interoperability-evidence-for-sophia-portals.md).
First audit the current implementation; the historical report does not prove
the defect persists. Reuse accepted evidence where it covers these exits.
Generic tests must cover shape replacement, leaving a target, focus/window
changes and owner teardown without retaining another client's cursor. Kitty's
OSC 22 stack and restoration are external interoperability probes: the terminal
owns that stack, and Sophia presents the resulting application cursor.

In an installed session, hovering over a hyperlink that requests a hand cursor
must show that cursor. Leaving the link must restore the cursor appropriate to
the new target. Repeat the transition and move between application windows to
check that a previous target's shape does not persist. Record the tested build
and application; protocol or state tests alone do not prove visible acceptance.

## Connections

`t067` in [todo.md](../../../todo.md) owns task status. This cursor-shape report is
separate from the [t066 input and redraw investigation](37xvg0y7-qt-menus-fail-to-open-while-pointer-leases-are-rejected.md):
successful clicking does not establish correct cursor appearance.
