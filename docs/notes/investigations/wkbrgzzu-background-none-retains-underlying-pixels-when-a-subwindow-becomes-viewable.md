---
id: wkbrgzzu
date: 2026-09-25
kind: investigation
status: closed
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

Signed candidate `1b9d16bf5c2e4c0591e8cfa5748ff8bf20f3e8e4`, rebased onto
`48d2f54c`, passed the main-tree layout gate and the complete X11 chain:

- selected-core, all profiles: 76 passed, 23 declared outcomes;
- xproto: 339 passed, 50 declared outcomes;
- windows, all profiles: 245 passed, 59 declared outcomes, including purpose 9
  PASS with its former failure declaration removed;
- events with XTEST: 123 passed, 72 declared outcomes.

Each profile report records the same clean candidate and unchanged source
afterward. Native-input passed 40/40 where selected; XTEST passed 44/44 on
every scenario. The independent core probe passed 162/162. The all-feature
authority/session suites passed 2,973 tests across 74 groups, with 39 existing
ignores. Workspace all-target Clippy, formatting and diff checks passed.

Reports, XTS journals, suite/probe logs and the red/green regression are
retained at
`~/.local/state/sophia/development-evidence/t218-1b9d16bf`, with verified
`SHA256SUMS`. Main-tree runs are also under
`.artifacts/x11-profile-1b9d16bf-xts-{selected-core,xproto,windows,events}`.
This closes the subwindow acceptance in t218; it does not claim arbitrary
cross-toplevel framebuffer preservation. No physical session or installation
was part of this acceptance.
