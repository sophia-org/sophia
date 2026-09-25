---
id: wkbrgzzu
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, x11, raster, conformance]
---
# Background None retains underlying pixels when a subwindow becomes viewable

## Question

Why does XTS XMapSubwindows purpose 9 see altered screen contents when a
subwindow with background None becomes viewable?

## Evidence and implementation

At `18569408`, skipping background painting left the new window without CPU
backing. Parent composition skipped that window, but GetImage of the child
returned zeros, and its first partial draw allocated a zero-filled buffer for
the whole window. None is not black and does not mean ongoing transparency.
The XTS purpose explicitly requires preserving existing parent or inferior
screen contents at mapping time.

When a subwindow without background becomes viewable, the
runtime now copies the parent's composed pixels into its backing. The copy
includes siblings, uses the child's interior origin (including its border),
and excludes the newly mapped subtree. Mapping an ancestor seeds descendants
in parent order. Later partial drawing and
GetImage use those same pixels. ParentRelative tiling and defined backgrounds
keep their existing paths. The shared root and foreign-namespace parents are
not sources for this copy. This change covers subwindows, not arbitrary
cross-toplevel framebuffer preservation.

The first wire regression failed with zero instead of the parent pixel before
the change. It covers MapWindow and MapSubwindows, an underlying painted
sibling, borders, partial drawing, presentation and retention independent of
later sibling drawing. A second test maps an ancestor over already-mapped
inferiors and includes a child clipped by its parent.

The initial standalone windows run still reported purpose 9 as FAIL (244
passed, 60 declared failures). Its setup clears each child with a temporary
background while unmapped, then sets background None. Skipping children with
existing backing incorrectly kept those off-screen pixels. The expanded
regression reproduced this, and the mapping snapshot now replaces that
backing too: this runtime does not promise backing store for unmapped
contents. An explicit-background sibling test ruled out redundant mapping as
the cause; the window table already makes an already-mapped child's map a
no-op. No lifecycle change was needed.

## Acceptance

The focused regressions, full authority suite and authority all-target Clippy
pass. The second standalone windows run completed all 304 purposes with
purpose 9 PASS; its only mismatch was the stale FAIL declaration, now removed.
Formal XTS windows, standing request-answer gates, core probe and final
candidate identity are still pending. No physical session or installation is
part of this acceptance.
