---
id: q53mtoq1
date: 2026-09-24
kind: investigation
status: investigating
tags: [investigation, x11, colormap]
---
# TrueColor allocations belong to clients and RGB channels

## Question

XTS XFreeColors-7 expects BadAccess when a client frees a colour it has not
allocated. Sophia discarded the pixels and plane mask and returned success.
Task t212 owns allocation accounting on the existing TrueColor visuals.

## Source and repair

Compared `dix/colormap.c` in the local Xorg checkout at
`9ba1d707b8770409a5e061d7120aef3f76be7723`: AllocColor, CopyFree,
FreeColors, FreeCo, RGBMASK and ALPHAMASK. TrueColor keeps references per
client and per RGB channel, including repeated allocations. Its immutable
palette does not make allocations unowned. Alpha bits are permitted on the
ARGB visual but do not name a fourth allocated channel.

The runtime now counts component references by namespace, client and colormap.
AllocColor and AllocNamedColor add references; LookupColor does not. FreeColors
decodes its pixels and mask, frees valid components even when another component
fails, and expands mask combinations per channel (at most 256 combinations,
not 2^32 combinations of whole pixels). CopyColormapAndFree transfers only the
requesting client's references. FreeColormap removes every client's references
on that map. Client resource destruction clears the departing client's counts;
retained close-down modes keep them until the retained resources are destroyed.

The reference-count maps hold at most 256 entries per channel for each
client/colormap pair. No colour identity or allocation authority moves into
Engine, WM or shell policy.

## Validation and remaining work

Candidate branch: `colormaps/t212`, signed implementation commit `f4d51797`,
based on `d8eb04f6`. Main-tree gates on the combined t217/t212 candidate remain
to be recorded before completion.

The new socket regression passes in both byte orders. It covers client
separation, repeated allocations, recombined channel values, partial frees,
copy transfer, plane masks, empty requests, invalid pixels, named allocation
versus lookup, and ARGB alpha. A negative control passing an empty pixel list
to the FreeColors runtime (the old no-op behaviour) fails at the first foreign
client free, which receives a reply instead of BadAccess. The production
cleanup fixture proves ordinary client destruction drops its references,
preserves the peer's references on the default map, and drops every client's
references when the map itself is destroyed and its XID reused.

Initial XTS evidence is in the main checkout at
`.artifacts/xts-colors-t212-initial/`. Its host hash is
`1dd8916267334df931a5fdf14ac712592d27d6530421dbfe961d67bd0422590c`.
All 102 purposes ran; 41 passed. The declaration correctly rejected the run
because XFreeColors-7 changed from FAIL to PASS. No other disposition changed.
The declaration is regenerated from that journal. Purposes 6 and 9 still
expect BadValue where dix returns BadAccess: duplicate references and permitted
ARGB bits in purpose 6, and per-channel error ordering in purpose 9. These are
retained suite/reference differences, not remaining untracked allocations.

The first full test run hit sandbox socket denials. The unrestricted rerun
exposed one old test asserting FreeColors was a no-op; that expectation and
the independent core probe were corrected. Final authority tests pass,
2030 tests in total, including all 509 wire tests. Initial native-input and XTEST profiles passed
40/40 and 44/44; the core profile failed only its two old no-op expectations.
The final core rerun on `f4d51797` passes all 162 executions; evidence is in
the main checkout at `.artifacts/conformance-core-f4d51797/report.json`.
Authority, negative-control, clippy and raw layout logs are retained under
`.artifacts/t212-f4d51797/`. Main-tree XTS gates remain required. Clippy passes
with warnings denied. The raw source-layout script reports standing debt;
`cargo xtask check layout` reconciles those reports against the debt ledger.
This change raises the input-discovery dispatch allowance from 1077 to 1089
lines and the wire-request allowance from 1347 to 1351 for the new request
variant and dispatch arm. Allocation accounting remains in `runtime/color.rs`;
the existing public dispatch facade is preserved.
The reconciled layout gate passes with those two ledger updates.

These checks use private sockets and software rendering. They make no physical
session acceptance claim.

## Connections

- [Colormap framing](4vkywq5i-the-colormap-requests-shared-one-minimum-length-and-answered-before-framing.md)
  established request lengths but treated FreeColors as a no-op.
- [X authority contract](../../sophia-x-authority.md) now specifies component
  allocation ownership and teardown.
