---
id: g930kzbe
date: 2026-09-08
kind: investigation
status: confirmed
tags: [investigation, rendering, x11]
---
# Default X visual excluded RGBA pixmap configurations

## Question

Why does accelerated Brave still show white video after the implicit-modifier
repair, despite working imported-pixmap pixel tests?

## Live evidence

The user installed `7dd74c5c5183`, opened a new live session, and reproduced
white YouTube video through Super+B. The GPU process used renderD128 without
`--use-gl=disabled`. A subsequent user-run launch preserved that device adapter
and added a bounded GLX call tracer. The log recorded 16,391 pixmap initialization
failures at Chromium's `native_pixmap_egl_x11_binding.cc:205`, with zero line-98
DRI3 refusals, GBM import failures or GPU restarts at capture.

The positive call sequence identifies the selected display configuration:

- An empty `glXChooseFBConfig` query returned six configurations. The first
  selected configuration had native visual `0x22`, alpha zero and stencil zero.
- The configuration-generation query again returned six. ANGLE read the color
  attributes of the `0x22` configurations, but read only the visual ID of all
  four `0x23` configurations before skipping them.
- Only two geometry records appeared, both for the depth-24 browser window;
  the tracer's geometry cap was not reached. No pixmap-creation call appeared.

The decisive evidence is the selected visual and alpha, rather than absence of
calls alone. The tracer does not instrument ANGLE's internal `eglChooseConfig`
and has finite per-process counters. The matching ANGLE source explains the
observed positive attribute sequence.

Redacted diagnostic counts, GLX stage records and test logs are retained in
`.artifacts/t068-default-visual-20260908/`. Browser URLs and raw user logs are
not copied into the archive.

## Finding and resolution

ANGLE revision `7df613367a1d4ca9aea9ece344d4580d32d132a9`, used by Chromium
152.0.7977.83, selects the first matching native visual when the display names
`EGL_X11_VISUAL_ID_ANGLE`. `DisplayGLX::generateConfigs` then requires both that
visual and the selected context's color-channel sizes. Sophia's default visual
had only alpha-zero configurations; every RGBA8 configuration belonged to the
other visual. The video importer requires alpha eight and therefore cannot
select a compatible EGL pixmap configuration on that display.

The previous probes asked for an RGBA configuration without following the
default-visual selection. Their successful imports did not exercise this filter.

XLibre checkout `56be9f4320ef121dc5d4bc40a6365d995512d3bc` supplies the server
reference. `Xext/glx/glxscreens.c` matches native visuals by RGB masks and class,
prefers alpha-bearing GL configurations, and excludes alpha from ordinary X
visual depth. `glxcmds.c` accepts pixmaps independently of that native visual's
depth. The [GLX 1.4 specification](https://registry.khronos.org/OpenGL/specs/gl/glx1.4.pdf)
also distinguishes color-buffer bits from TrueColor visual depth in section
3.3.3. Its legacy visual-based pixmap constructor retains an exact native-depth
requirement in section 3.4.2.

The repair stays in the X frontend. The default X visual remains depth 24 while
its GL buffers provide RGBA8. Catalog rows carry native visual depth separately
from GL color bits; both GLX query versions read the same catalog. Modern
pixmap creation accepts native RGB storage or the configuration's full RGBA
storage, and geometry reads the actual live or retained backing depth. The
legacy visual-based constructor still enforces native depth. Unsupported
depths, namespace checks and resource lifetime remain enforced.

No browser recognition, Engine rule, allocation copy or per-frame workaround
is introduced. The correction changes configuration description and validation.

## Validation and remaining work

The new private GPU regression follows first-default-visual selection, then
binds a legacy-imported depth-32 ARGB buffer using that visual's GL configuration.
It failed before the repair at the alpha-eight precondition. It passes after
the repair, reading exact initial and updated pixels with alpha `0x7b` and
`0x3d`. The ordinary GLX first-frame probe now also requires the default visual
and depth 24; its existing full-frame opaque-alpha assertion remains in place.

Thirteen external configuration tests pass, including actual backing geometry
across FreePixmap and XID reuse, RGB/RGBA binding promises, strict legacy visual
depth, and refusal of unsupported core depths. The full
`SOPHIA_FIRST_FRAME_REQUIRE_AUX=1 cargo xtask check` passed 2,811 tests across
249 successful summaries, including all eight pixmap hardware cases and opaque
GLX/EGL first-frame pixels. Its sole Clippy finding was a blank line after a doc
comment; that was removed, and the affected crate's complete Clippy check plus
workspace formatting passed afterward. Logs and candidate identity are retained
in the evidence manifest.

## Physical acceptance on 2026-09-09

niltempus opened a new session on `b43d23d0bb15`, launched Brave through Super+B,
and confirmed that the playing video was "normal". This accepts the installed
white-video repair. The session identity is
`00000001788947503816-89494440-9ce4-43e1-9884-2b7d75ea14a9`; its release and
binary checksum match the packaged candidate in the evidence manifest.

Brave's GPU process remained PID 21982 across the two observation snapshots,
held renderD128 descriptors, and had no `--use-gl=disabled` flag. It still used
the explicit CLI device override. Its stderr went to `/dev/null`, so these
observations establish neither an error-free run nor the absence of earlier
GPU restarts. The wider `t068` accelerated-video gate and `t069` generic
device-negotiation requirement remain open.

## Connections

The [implicit-modifier investigation](n5i1x7iv-implicit-dma-buf-exports-used-the-wrong-drm-modifier-sentinel.md)
records the independently repaired black-pixel export defect. The [Brave
investigation](uqnx2t2b-brave-gpu-restarts-after-va-buffers-fail-gbm-import.md)
owns physical video acceptance, and the [pixmap export contract](../../pixmap-texture-exports.md)
owns these visual and storage semantics. The [universal-device plan](../plans/6tewvlbh-universal-device-negotiation-across-sophia-clients.md)
keeps this frontend compatibility correction separate from client device choice.
