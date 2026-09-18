---
id: d7k4q2vm
date: 2026-09-18
kind: investigation
status: repaired-pending-native-acceptance
tags: [investigation, native-shell, dock, x11, lifecycle]
---
# Dock smoke exposes color lookup and seat retirement order

## Retained observation

The operator saw a text-like dock and terminals which immediately closed in
`.artifacts/lom-panel-native/20260918T153807Z`, based on Sophia `fdc60b9a`
and Provlita `01609c8b`. Keep that capture immutable. Four exact persistent
catalog launches reached Session process custody, all on logical output 1;
each xterm then reported BadRequest for core opcode 92 (LookupColor).
Neither process creation nor an action ACK proves a usable terminal.

Later, a requested VT handoff reported drained native ownership, then failed
at libseat disable with one remaining lease. This is separate from the X11
refusals. The old Session path skipped retirement polling while a VT request
was outstanding; its release path asked the broker to disable before polling
the retained owner. That owner retains the card lease until shutdown and
disposition complete. The capture does not identify every individual holder
of the remaining lease; the ordering gap is source-confirmed.

## Repairs

LookupColor now uses the existing colormap authority, bounded named-color
table and TrueColor conversion. It returns the exact/visual RGB reply without
allocating a color. Named-color decoding treats STRING8 as Latin-1 rather than
UTF-8. Missing names and inaccessible colormaps remain protocol errors, not
guessed colors. The independent t057 inventory records remaining external-case
coverage debt; the new Rust socket checks are not relabelled independent XTS.

Seat release now polls the exact retiring owner before asking the broker.
Outstanding leases are a typed Pending outcome. The owner loop keeps visiting
retirement during requested VT handoff and uses a single bounded release
deadline. It does not discard leases, acknowledge early, run ordinary KMS after
revocation, or admit a replacement on an old completion. Unresolved ownership
still fails with custody retained in the existing terminal error carrier.

Provlita replaces text symbols with original terminal/globe/folder vector
artwork recorded through Xilem Canvas into its existing Vello GPU scene.
Square opaque tiles, labels and layout-derived targets share the retained tree.
Only bounded CanvasSizeChanged notifications enter the toolkit layout path;
application activation still requires an exact presented protocol action.
No icon-theme filesystem access or CPU production fallback was added.

Catalog child exits now carry the original transaction and grant plus success.
The smoke verifier requires matching successful exits and a zero X refusal
tally, in addition to independent component lifetimes and presentations.
The operator exits each terminal by typing `exit`; VT handoff is a separate
attended lifecycle check rather than an ambiguous interruption of the smoke.

## Evidence and limitations

Logs are under `.artifacts/dock-smoke/`; disposable mutant sources and patches
are in `.artifacts/dock-repair/` and Provlita's `.artifacts/dock-repair/`.
The private xterm test launches the actual catalog executable/argv in a
device-hidden namespace, with a controlled login shell and an explicit named
ANSI color. Restoring the absent opcode reproduces exit 83 and opcode-92
BadRequest; this is an application reproduction without Session/WM/KMS.
The earlier default-color invocation passed even with the opcode removed, so
it is startup evidence only and not the discriminating reproduction.

Seat controls exercise the production retirement/broker boundaries with
simulated worker/disposition effects. Bypassing retirement or allowing disable
with outstanding leases fails the controls. They do not induce a real VT event.
Removing icon drawing fails the first-frame raster control; CPU rasterization
is test evidence only. Mutations were confined to disposable snapshots.

An initial diagnostics command omitted native-session and hit the pre-existing
ungated indicator re-export; its failure remains recorded separately. A
post-mutation UI run reused a stale package build despite restored source;
the explicit package-clean rerun is the restoration evidence. Full canonical
validation must name the exact signed successor. Native/GPU acceptance remains
pending; no new hardware run, installation or publication is implied.

Related: [native component plan](../plans/ptil1ejw-modular-native-shell-components-and-independent-launcher-critical-path.md),
[dock smoke instructions](../../../tools/probes/dock/README.md).
