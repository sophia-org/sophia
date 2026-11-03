---
id: 5euoyv1f
date: 2026-11-03
kind: milestone
status: recorded
tags: [milestone, rendering, performance]
---
# Qualify native GL CPU upload reuse on changing and retained layers

## Result or change

Mason approved integrating the native GL CPU upload reuse candidate after the
changing-strip nonregression check, retained-layer CPU comparison, and final
pixel/custody checks. This completes that bounded implementation and measurement
slice; it does not close the overall CPU-gap task or establish general desktop
performance. The GL candidate is based on `134e74df9`; its measured production
code is `e380abbcf` (preceded by `034e58722` and `928f7a30e`). Final integration
adds tests and this record, without changing the measured production code.

Trusted immutable CPU pixel revisions now identify context-local textures.
Changing buffers still upload; retained revisions reuse uploads, including
successive damage draws. The cache holds at most 32 textures / 64 MiB per EGL
context, beyond which the existing scratch path is used. It holds no resource
leases. Replacement, failed composition, and context destruction preserve the
existing custody and cleanup contracts. This is one compositor path supporting
both changing CPU raster output and retained layers, not a new rendering mode.

## Evidence and decisions

Both comparisons used iGPU HDMI-A-2 at 1920x1080/60 Hz, W2 off, 120 warmup and
2000 counted frames per arm. Bounds below are CPU accounting bounds, not
confidence intervals. Session and peer CPU are combined; external kernel workers
are excluded.

| Workload | Baseline mean, us/frame | Candidate mean, us/frame | Candidate minus baseline |
| --- | --- | --- | --- |
| Changing owned-ring strip | 1107.5–1150 | 1092.5–1132.5 | -57.5 to +25 |
| Four retained 480x512 tiles | 1332.5–1372.5 | 1147.5–1187.5 | -225 to -145 |

The strip passed the predeclared 5% nonregression rule without demonstrating a
saving. The retained workload demonstrated a midpoint saving of 185 us/frame
(13.68%); the two paired midpoints improved by 190 and 180 us. Its Session-only
difference was -210 to -170 us/frame; peer difference -15 to +25. Every counted
native interval passed the cadence gate, and all four cleanups restored the seat.

Retained evidence on crunch:

- `~/.local/state/sophia-native-gl-strip-03-result-01/`: changing-strip result,
  exact package/host identities and independent audit.
- `~/.local/state/sophia-native-gl-retained-04-result-01/`: result seal
  `fad3db775bc44a7923961244d06baa65b03cb0ed8edcfa4354bf6d930829f4a1`;
  copied independent audit seal
  `12903de60685db1ed5b3b7ebc51d3ab48738946b160052ad6f65a0e6813c8e68`.
- `~/.local/state/sophia-native-gl-merge-check-01/`: final pixel-readback,
  resource-custody, renderer-suite, lint, Session integration and layout checks.

The final readback test compares retained revisions against freshly uploaded
pixels through movement, overlap, premultiplied transparency, opacity, clipping,
layer removal/reappearance, output resize, source resize and revision replacement.
Literal colour assertions complement that comparison. Existing iGPU tests cover
32-texture/64-MiB fallback, failed upload invalidation, changing/unidentified
pixels, damage reuse, separate contexts and resource release/reopened-store
identities. The broad renderer suite also found a source-inspection test that
still expected both upload paths in gl.rs; it now checks cpu_cache.rs as well.
Only tests changed after measurement.

Earlier refused attempts remain separate. Retained-04 uses the same tmpfs-log
runner and corrected launch helper in every arm; its individual CPU figures are
not like-for-like with the disk-backed strip/retained-01 runs. No pooling,
threshold relaxation, extra ABBA, protocol change, runner merge or SDK merge is
part of this integration. The successful run does not establish the cause of the
older stalls. The historical 3x external ratio is not remeasured here.

## Remaining work

The [continuous-frame CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
and [active queue](../../../todo.md) continue to own the broader performance
work. A normal installed desktop smoke remains distinct from these automated
render-node pixel checks and measured native workload runs. Multi-output behavior
and a fresh matched external reference are not qualified by this result. Profile
the adopted baseline before choosing another optimization; this note does not
reopen W3 or inactive candidates.
