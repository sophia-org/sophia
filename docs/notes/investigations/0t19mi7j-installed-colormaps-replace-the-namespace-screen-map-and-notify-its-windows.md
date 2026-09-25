---
id: 0t19mi7j
date: 2026-09-25
kind: investigation
status: closed
tags: [investigation, x11, colormap]
---
# Installed colormaps replace the namespace screen map and notify its windows

## Question

Why do XInstallColormap purposes 1 and 4 fail when installation requests
succeed, and what answers and lifecycle notifications must change together?

## Evidence and resolution

At `721e29d5`, InstallColormap and UninstallColormap were no-ops,
ListInstalledColormaps always named the default, and the wire encoder always
set GetWindowAttributes.map_installed. Window attribute notices separately
treated only the default as installed. The local reference in
`~/src/xserver/dix/dispatch.c`, `dix/colormap.c` and `mi/micmap.c` validates the
map, replaces the single installed map with loss before gain, and restores
the default when the active non-default map is uninstalled or freed.

The runtime now owns one installed map per namespace screen. Installation
reports windows naming the old and new maps, including the root and unmapped
windows. Repeated install and uninstall of an inactive/default map do nothing.
Replies and attribute-change notices use that state. Freeing an installed map
restores the default, then clears surviving window attributes; disconnect and
retained-client cleanup carry these notifications with their own resource
release rather than leaving them for another request to drain.

Colormap routing filters recipients by namespace, especially for the shared
root ID. This routing has its own small source file; it leaves the request
watermarks and pointer-selection lock ordering unchanged. No hardware palette
or Engine color-management state is introduced for TrueColor.

## Validation

Socket regressions cover both byte orders, multiple selectors, loss/gain
ordering, installed-list and attribute answers, repeated requests, uninstall,
free, disconnect and namespace isolation. Eleven colormap-filtered wire tests
pass. The standalone XInstallColormap scenario passes three purposes and
declares the suite's three UNTESTED purposes. Colors gains XFreeColormap-3 and
XUninstallColormap-2; their stale declarations are removed, for 43 passed and
59 declared outcomes. The colors scenario does not include XInstallColormap,
so a separate six-purpose manifest is retained for that scenario.

Signed candidate `b8203dc77ad0b1abe1fe1d42198d5890a07b93be`, rebased onto
`ca4dd912`, passed the main-tree layout gate and full X11 chain:

- selected-core, all profiles: 76 passed, 23 declared outcomes;
- xproto: 339 passed, 50 declared outcomes;
- colors, all profiles: 43 passed, 59 declared outcomes;
- XInstallColormap: 3 passed, 3 suite-declared UNTESTED purposes;
- events with XTEST: 123 passed, 72 declared outcomes.

All profile reports name the clean candidate and confirm unchanged source
afterward. Native-input passed 40/40 where selected; XTEST passed 44/44 on
every scenario. The independent core probe passed 162/162. Rebased all-feature
authority/session suites passed 2,976 tests across 74 groups, with 39 existing
ignores. Workspace all-target Clippy, formatting and diff checks passed.

Reports, XTS journals and suite/probe logs are retained at
`~/.local/state/sophia/development-evidence/t213-b8203dc7`, with verified
`SHA256SUMS`. Main-tree artifacts are also under
`.artifacts/x11-profile-b8203dc7-xts-{selected-core,xproto,colors,XInstallColormap,events}`.
This closes t213. No live installation or physical acceptance was part of it;
legacy writable palettes and physical color management remain outside scope.

## Connections

[Client color references](q53mtoq1-truecolor-allocations-belong-to-clients-and-rgb-channels.md)
owns allocation semantics; installation does not change those counts.
[The frontend contract](../../sophia-x-authority.md) records the installed-map
and namespace boundary.
