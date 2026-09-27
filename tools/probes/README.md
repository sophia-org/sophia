# Private graphics probes

## Protected shell GPU/content proof

`sophia shell-gpu-content-proof` accepts an explicit client, geometry, outcome
sequence and pixel policy. It exercises the protected render-device grant and
content protocol without acquiring DRM master. The `contract` pixel policy
checks transport validation; `full-surface-raster` additionally requires the
declared raster pattern. Neither proves GPU execution by itself. Client adapter
and rendering evidence belongs to the external client verifier, and the proof
always reports `native_presentation=false`.

Named-client GPU proofs, workload budgets, native launcher and dock acceptance
recipes live in the niltempus repository. Its gates pin Sophia's public proof
interface and retain the workload and verifier controls. Physical acceptance
remains separate from the deterministic checks.

## Private GLX pixmap probe

`glx_pixmap.c` creates offscreen resources and compares synthetic pixels read
through a direct GL texture. It checks the initial CPU contents, a later partial
write and retention after `FreePixmap`, for depth-24 and depth-32 buffers with
2D and rectangle textures. Its non-power-of-two width exercises the texture
shape needed by ordinary video frames. It follows the required
synchronize/bind/read/release sequence. It opens no visible window and sends no
input.

The optional `--texture-1d` diagnostic exercises a separate Mesa client limit:
its direct GLX target decoder does not recognize 1D despite the driver's target
mask including it. The local Mesa 26.1.8 client reports target zero on that path.
The default pixel gate therefore covers 2D and rectangle sampling; it does not
claim 1D client support.

The integrated test starts a private X frontend with the same pixmap provider
as a native session. Select a render node explicitly:

```sh
SOPHIA_PIXMAP_TEST_DEVICE=/dev/dri/renderD128 \
  cargo test --offline -p sophia-session --all-features --lib \
  direct_glx_client_reads_live_and_retained_pixmap_exports \
  -- --ignored --nocapture --test-threads=1
```

The lower-level renderer tests verify repeated imports, partial updates,
revision replay and allocation lifetime through a private EGL/GL consumer:

```sh
SOPHIA_PIXMAP_TEST_DEVICE=/dev/dri/renderD128 \
  cargo test --offline -p sophia-renderer-live --all-features \
  --test shared_pixmap -- --ignored --nocapture --test-threads=1
```

Both commands require DRM render-node access. The GLX test also requires a C
compiler and Xlib/GL development files. They do not install a build, replace a
session or prove browser hardware-video playback. See the
[export contract](../../docs/pixmap-texture-exports.md) for those boundaries.

## Desktop probes

The GTK redraw and Quickshell popup probes live in the niltempus repository.
They test selected desktop clients against a pinned Sophia build.

## Explicit DRI3 layout probe

`dri3_layout.c` allocates real XR24 or AR24 buffers through the server's DRI3
device, using an explicitly selected modifier. It records the actual GBM layout,
per-plane descriptor sizes, queried screen/window preferences and received
Present Complete/Idle events. It generates muted opaque pixels through GBM's
mapping API; it does not capture the desktop or inject input.
`--suboptimal` opts each Present into reallocation advice. The default leaves
that bit clear. The probe records advice but does not reallocate in response.

```sh
cc -std=c11 -O2 -Wall -Wextra -Werror tools/probes/dri3_layout.c \
  -o /tmp/sophia-dri3-layout $(pkg-config --cflags --libs xcb xcb-dri3 xcb-present gbm)
/tmp/sophia-dri3-layout --geometry 0,0,16,16 --format XR24 --list-only
/tmp/sophia-dri3-layout --geometry 64,96,96,64 --format XR24 \
  --modifier 0 --frames 4 --timeout-ms 3000
```

Both commands use the selected session's `DISPLAY` and `XAUTHORITY`. List-only
creates an unmapped window for the queries; with `--modifier` it also verifies a
real allocation without importing or presenting it. The second command briefly
opens an owned window. Geometry is explicit and must remain unchanged. A physical
scanout comparison needs the selected output's exact geometry and an otherwise
eligible scene; the small example checks only the client's feedback/lifetime path.

The probe refuses implicit sentinels, substituted allocation metadata and layouts
GBM cannot map for writing. It never relabels a buffer or maps an opaque plane
directly. Two buffer slots bound residency; a slot is reused only after both
Complete and Idle. The final scanout buffer may remain owned until connection
teardown. A process deadline covers X setup and event waits, but does not promise
cancellation of an uninterruptible kernel operation. `result=pass` establishes
the probe's completed protocol flow, not captured pixel correctness or task
acceptance.

For an exact retired comparison, the candidate **owner** must start with
`SOPHIA_ENABLE_DIRECT_SCANOUT=1`; ordinary sessions leave this physical gate off.
Also enable `SOPHIA_LIVE_VISUAL_PROGRESS=1` before starting that owner so routed
completion clocks are recorded. Setting either variable on the probe cannot
change the running owner. Confirm the owner's identity and admission mode before
opening a full-output test window. Direct eligibility still requires exact output
geometry, opaque pixels and no composed chrome/cursor or other disqualifying
layers. These are controlled acceptance conditions, not ordinary app launch flags.
Covering another surface does not remove it from the current composition plan.
Confirm an isolated output before interpreting Copy as a layout refusal. The
installed CLI must also forward the backend's `sophia_scanout_evidence` tracing
target into daily capture; console output alone is discarded by normal sessions.
Retain
candidate/session identity, capture health and one bounded run interval, then use:

```sh
python3 -B tools/verify_layout_comparison.py \
  --session-log /path/to/bounded-session.log --probe-log /path/to/probe.log
```

The reader joins the layout comparison to retirement by source-image identity, then
to current preference comparison by transaction and native generation. It checks
the unique routed completion clock against the probe's received completion,
submission and actual allocation. Repeated test attempts, shared completion
clocks, implicit layouts and missing stages are inconclusive and fail the check.
Schema 3 distinguishes original atomic-test rejection from framebuffer-creation
rejection; both require the alternative's actual passing test and exact retirement.
Schema 1 remains the older atomic-pair record. A framebuffer refusal alone cannot
pass this reader. The default expects ordinary Copy. `--expect-mode suboptimal`
instead requires the exact submission's recorded opt-in, the server's retained
SuboptimalCopy mode, and the client's matching completion. It does not accept a
hint without the full proof chain. This checks one transaction; verifying that
the run contains at most one hint per surface/preference generation remains a
separate acceptance check.
`--transaction` selects an existing comparison in a longer bounded run. Old logs
without these identities cannot prove the join. Session identity and capture
health remain prerequisites checked by the caller; the reader does not declare
t069 or t070 complete.

`cargo xtask check` runs the bounded reader/CLI regressions and a private frontend
test that checks both exact formats without mapping, importing or presenting a
window. That test uses a render node and does not acquire DRM master.
