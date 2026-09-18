# Private graphics probes

## Lom protected GPU/content proof

`tools/lom_gpu_content_hardware_proof.sh` launches the exact source-built Lom
`--serve` client through Sophia's production metadata-shell protection policy.
The private domain receives one render node under its real kernel basename and
a generated read-only sysfs discovery view for that same device, with no card
node, host sysfs tree, input device, display socket or network. Lom validates
the enumerated Vulkan adapter's DRM render major/minor before rendering a 256x24 Vello
panel and sends its real complete content candidate. The host verifies the
nonempty immutable pixels, returns the real `RendererFailed` outcome, and
requires resource and backing cleanup. This is a hardware render and protocol
proof; it deliberately records `native_presentation=false` and does not acquire
DRM master.

```sh
SOPHIA_LOM_GPU_PROOF_ARM=1 tools/lom_gpu_content_hardware_proof.sh
```

The separate `tools/run_current_lom_panel_gate_tty4.sh` is the native acceptance
candidate. Run it only from tty4 after ending the graphical session, with
`SOPHIA_LOM_NATIVE_GATE_ARM=1`. It builds from clean signed Sophia and Lom tips,
first runs the protected GPU/content proof while Sophia does not own the display,
then runs the 90-second normal-exit workload (110-second failure watchdog). A failed prerequisite therefore
stops before graphics takeover and retains the client's boundary error. The native
stage restores the TTY through the existing session harness and retains exact
identities and structured session diagnostics. A machine-independent
gate or isolated proof cannot establish that the bar was visible; the attended
run still requires the operator to confirm its placement and appearance.

Wait ten clock ticks after both bars appear, then make 20 state-changing workspace
clicks per output within 60 seconds (40 total). No clicks during warmup or after
the 40; wait for automatic exit. The checked-in candidate budget is ACK p95/max
50/100 ms and exact native retirement p95/max 150/300 ms. These are predeclared
workload acceptance values, not claims of historical approval or driver guarantees.
The launcher copies/hashes budgets and profiles before GPU use, checks them again
after proof and session, and refuses any reused evidence directory. It requires
normal exit 0, existing strict health/recovery proof, complete causal outcomes,
bounded in-run inventory and exact zero final content accounting. See
[workload evidence](lom_workload/README.md) for scope and interpretation.

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

## GTK redraw

`gtk_redraw.c` exercises GTK3 drawing through Sophia's X frontend: a dialog,
a menu, dialog hide/show, and an explicit redraw. It uses synthetic text and
captures only its own windows. It needs a C compiler, `pkg-config`, and GTK3
and Xlib development files. Build Sophia with `native-session` and supply a WM
that implements `sophia_wm_v1`:

```sh
cargo build --offline -p sophia-cli --bin sophia --features native-session
python3 tools/run_gtk_redraw_probe.py --wm /path/to/wm
```

The runner creates a private directory under `/tmp` and prints its path. It
records binary identities, configuration, the session log, five PNG files, and
`result.json`. Use `--sophia` to select another candidate and `--output-parent`
to retain evidence elsewhere. It starts a bounded headless session with no
physical input. It does not alter the installed desktop or launch a shell.

A pass requires a successful client exit, clean session health and cleanup,
and background and text in both halves of each dialog and every menu row.
After the dialog hides, X11 focus must refer to a viewable window or to the
special None/PointerRoot targets. Retaining focus on its unmapped window fails
the probe even when its stored pixels are correct.
The headless composed scene must also report nonempty frames. This rejects the
admission regression where frontend `GetImage` returned complete windows but
the scene remained empty. The legacy `cpu_max_nonzero_pixel_bytes` field uses
exact counts only for initial proof frames, then bounded composition evidence;
the probe treats it solely as a nonempty-scene witness. It cannot distinguish
correct window content from surviving decoration. Exact compositor pixels are
checked separately by the admission/visibility regression.
The controlled white background and dark text make these checks independent
of the user's theme; they are content checks, not exact font-image comparisons.
The probe reads pixels only at five observation points. It installs no drawing
interposer. The socket regression `gtk_clip_copy_stream` separately compares
canonical pixels and published buffer updates for batched, fragmented, and
paced writes without intervening readbacks.

These checks establish frontend drawing, remap behavior, focus eligibility after
unmap, and nonempty headless composition. They do not prove
physical composition, replacement-focus policy, or pointer grabs. The synthetic menu has
no physical trigger event; GTK may report that warning. Installed-session
acceptance still requires opening Thunar menus and submenus, dismissing them,
and switching windows without missing pixels or lingering overlays.

## Qt popup probe

`qt_popup.cpp` opens an owned QMenu, selects an action, reopens it, opens and
selects a nested menu, then dismisses another opening with Escape. A separately labelled raw-X phase
uses the same XCB connection to create an owned popup, map it and immediately
grab without drawing first. Only GrabSuccess permits its marker drawing. This
checks the client pattern that cannot wait for presentation before receiving
the grab reply. Its events
are delivered locally to its own Qt widgets. It never injects physical input.
The executable forwards Qt's core and XI2 grab calls unchanged and records their
actual replies; it sends no extra X request between mapping and grabbing.
This observation is local to the probe process, with no desktop-wide interposer.

```sh
python3 tools/run_qt_popup_probe.py --sophia /path/to/sophia --wm /path/to/wm
python3 -m unittest discover -s tools/tests -p qt_popup_probe_test.py
```

Building needs a C++17 compiler, `pkg-config`, Qt6 Widgets, Xlib, XCB and XCB
XInput development files. The runner creates a private headless session with no
input devices, records executable/source hashes and configuration, and retains
four own-window PPM captures under the printed evidence directory. It does not
use the current display or restart the installed session.

A pass requires observed successful grabs on the actual first, reopened and
final menu windows, with mapping preceding each grab. Missing observations fail
the check. It also requires mapped menus with the expected transient-owner
chain, light backgrounds and dark text, successful selections and dismissal,
and successful raw grab followed by exact marker pixels. It also requires
a successful client exit, and clean session health and cleanup.

Own-window `GetImage` checks frontend pixels. The compositor's nonempty-scene
witness establishes only that composition occurred; neither establishes exact
composed menu pixels. The Engine scene regressions cover exact composition
separately. These local Qt events also do not prove physical click delivery or
recovery from a real application grab. Installed-session acceptance remains
necessary. Use `--authority-trace` to retain synthetic X request diagnostics; this changes
scheduling and is recorded in the identity file. The ordering race can depend
on scheduling: preserve repeated baseline runs and do not interpret one successful baseline as a disproof.

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

### Independent native launcher

The [three-component dock smoke](dock/README.md) adds `lom-test dock` for independent
Lom, Bemenu and Provlita processes. Its profile generator and host transcript
reader are Rust `xtask dock` commands; visual/native acceptance remains attended.

The [native launcher smoke](native_launcher/README.md) adds `lom-test launcher`
without changing plain `lom-test`. It retains separate component identities,
per-role GPU policy and host presentation/accounting evidence. Its offline
controls substitute builds/VT/native effects; no automated physical run is part
of those controls. See the linked readiness qualification before an attended run.
