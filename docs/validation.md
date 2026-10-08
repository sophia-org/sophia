# Validation

**Role:** reproducible validation catalog.

The active product is native X11 with namespace admission, portals, external
WM policy, and Engine-owned CPU/DMA-BUF presentation. Retired compatibility
frontends are preserved under `research/` and are not validation gates.

Sophia's default validation path must not require native renderer libraries,
kernel devices, a display server, or network access. The default suite protects
the data model, protocol authorities, runtime reducers, renderer admission
records, and deterministic backend seams.
Default physical input validation uses `QueuedInputPoller`. Native libinput
coverage is feature-gated and opt-in; ordinary workspace validation must prove
physical input intake with deterministic queued packets and must not open
`/dev/input` devices.

Run before committing ordinary changes:

```sh
cargo fmt --check
tools/audit_source_layout.sh
cargo test --workspace --offline
```

The broader development gate, `cargo xtask check`, runs workspace tests with
`--all-features`, including the `sophia-session` library's `native-session`
controls and socket-directory tests. It therefore needs the native development
libraries at build time. Tests use an isolated configuration directory and
the gate clears the inherited destructive scanout-smoke opt-in. Explicitly
ignored hardware and component-acceptance tests remain separate; enabling
the feature is not permission to access live input or display devices.

The workspace test child receives `/dev/null` as standard input. The protected
GPU proof fixture also declares its own standard descriptors, so a socket
inherited from a terminal or agent launcher cannot change the test's result.
The production proof still refuses inherited sockets.

### Offscreen renderer tests on a selected GPU

Use `tools/run_render_node_test.py` for an already-built test that accepts
`SOPHIA_TEST_RENDER_NODE`. It selects one GPU by PCI identity and gives the
test only that render node. Primary DRM cards, physical input, VTs and live
session sockets are absent; home, runtime and temporary directories are private.
Both agents can run correctness tests on different GPUs. Performance results
still need a quiet host, and display/KMS tests need separate ownership.

```sh
python3 -B tools/run_render_node_test.py \
  --pci 0000:16:00.0 --sha256 FROZEN_TEST_SHA256 \
  --output /absolute/new/receipt --timeout 60 \
  -- /absolute/frozen/test --ignored --nocapture --test-threads=1
```

The output directory must be new. It keeps the executable, its hash, command,
device and namespace admission, stdout/stderr and exit/timeout result. Runs on
one GPU serialize through a nonblocking lock. There is no unconfined fallback.
That lock coordinates these runners; it does not exclude the live desktop or
other programs that already use the device.
A zero exit only means the admitted executable finished; use a test that
asserts it exercised hardware and checked pixels, rather than silently skipping.
The runner deliberately strips ambient graphics/loader flags, so select a
frozen executable with the runtime libraries it needs available in `/usr` or
the Nix store. It does not build or install anything.

The executable and runner are frozen, but shared libraries and the host
`bwrap` are not. Record their scope and the confinement tool's identity when
qualifying a recipe. The minimal `/etc` contains only the loader cache; tests
that need other host configuration must account for its absence. Negative child
exit values in the receipt denote POSIX signals. The admission file is in the
test's writable artifacts directory, so it is evidence from a reviewed test,
not an immutable audit against a test that rewrites its own results.

The runner is trusted host tooling, not a defense against a hostile process
of the same host UID. Private namespaces and descriptor cleanup confine the
test and its children; the two agents also keep separate source/build owners.
See the [GPU development plan](notes/plans/b0yjm547-separate-gpu-development-from-the-live-desktop.md).

### Shader Sources

The renderer's GLSL lives in its own files under
`crates/sophia-renderer-native-egl/src/gl/shaders/`, embedded at compile time by
`include_str!`, so nothing is read at runtime and there is no asset to deploy.

The reason they are separate files is that a shader error is otherwise not
discoverable until a GPU refuses it, and that refusal is not fatal by design: the
pipeline records `status=unavailable`, falls back to the direct program, and the
session runs on with its filtering silently uncorrected. That is right at runtime
and a poor place to find a typo. A GLSL front end finds it first:

```sh
tools/check_shaders.sh          # or SOPHIA_GLSLANG=/path/to/glslangValidator
```

It refuses to run without a validator rather than passing, and refuses a run that
matched no shader sources rather than reporting success over nothing. It is a
front-end check only: it says the source is valid GLSL, not that a driver's
limits were respected or that a uniform was bound.

### Bounded Formal Transition Model

Milestone 12 adds unattended TLA+ gates for visual candidate preparation,
submission, output-scoped retirement, terminal settlement, resource release,
X11 admission recovery, and full-geometry feedback. They are not Milestone 11
installed-session requirements and add no physical operator steps.

The model and its action-to-Rust boundary map live under `validation/tla`.
Sophia pins the command-line TLA+ Tools v1.7.4 jar by SHA-256. Once that
artifact has been obtained, the check is entirely offline and leaves its TLC
state in a temporary directory:

```sh
SOPHIA_TLA2TOOLS_JAR=/absolute/path/to/tla2tools.jar tools/check_tla.sh
```

The bounded configurations explore retirement and supersession ordering,
exact PresentedBuffer selection through proactive or timeout recovery,
ownership of a software Present by one native frame, move/resize geometry
feedback, exact cached workspace assignment, pixel-silent first-admission
retry, public policy negotiation and transfer assembly, and atomic
multi-output projection.
`TabDescriptorPresentation` checks tab candidate freshness and capture lifetime
across layout changes and shell loss. Its stale-candidate and lost-capture
negative controls must violate `CoherentPresentation` and `ExactActivation`,
respectively. Independent tab wire and protected client checks run through
`tools/check_shell_protocol.sh`; `tools/check_policy_protocol.sh` checks the
WM file export and independent C SDK peer. Hagia and
Narthex run their own `SOPHIA_ROOT=/path/to/sophia nimble test` gates.
These offline checks are separate from the [tabbed-layout operator gate](tabbed-layouts.md#verification-and-operator-acceptance).

`tools/check_control_protocol.sh` checks the experimental
[control v1 wire](sophia-control-v1.md), independent Python and Rust clients,
real Unix endpoint, config opt-in, sequencing, dispatch recheck, cancellation,
deadlines, and bounded queue pressure. Add `--live-owner` to require the
bubblewrap namespace denial proof and the live session owner fixture: policy
success waits for Engine settlement; restart waits for the intended
replacement's first commit. These tests use temporary sockets and supervised
test processes, not a graphical session. Installed input/render fairness is a
short optional operator smoke. Scripted reload remains deferred.

The Rust WM and shell socket codecs are retired. Protocol tests require their
former message numbers (WM 32–55; shell 96–122 and 160–202) to fail as unknown
kinds. WM row values retain their golden corpus through `policy_record_corpus`;
complete file envelopes, arrays and controls retain their own malformed-input
tests. The generator still checks file rows, output/control contracts and the
C SDK's pinned compatibility artifacts, but cannot regenerate the retired Rust
WM frame codec. C SDK artifacts retire with the coordinated SDK update.

`ShellWorkAreaCoordination` checks that a future ready shell reservation,
derived work area, and exact WM projection promote as one coherent generation;
normal shell or WM failure preserves the prior presented bundle. It is a target
pre-schema model and is not evidence of a production shell runtime.
`OutputTopologyLifecycle` checks the current native owner's rescan boundary:
replaceable hotplug hints, one routed-input epoch advance, old-scanout
retirement, complete multi-consumer publication, current policy settlement,
and presentation-before-input. No-output and bounded rebuild failure remain
recoverably quarantined.
The frame-ownership model permits an unrelated frame to submit and retire first
and proves that only the exact bound frame can emit feedback.
`GeometryFeedback` separates full rectangles from pixel readiness and proves
no-op silence plus convergence after late-target/FIFO rollback.
`PolicyConnection` requires the full client, connection-epoch, and transaction
identity for admitted work.
`PolicyProjection` requires proposals to answer an outstanding server-issued
request for the current scene generation. `PixelSilentAdmission` preserves the
owner and one bounded retry before withdrawal. They remain suitable for
routine validation. A TLC counterexample that changes implementation behavior must
become a deterministic Rust regression before the model or implementation is
corrected. The models are not refinement proofs and must not be weakened to
accept a known Rust shortcut.

Specula is an optional development audit, not part of the build or installed
session. Its commit pin, narrow runner, retained findings, and artifact policy
live under `validation/specula`.

### Complementary Architecture Models

The bounded Alloy and SMT-LIB2 gate complements TLA+ without translating or
duplicating the temporal models. Alloy checks finite authority,
protection-domain composition, action-capability, policy-operation binding,
and target topologies. Z3 checks target geometry/disclosure arithmetic,
policy presentation geometry, and consumes
schema-generated `sophia_wm_v1` widths and maxima for wire-bound proofs.
Every protected query is paired with a retained negative control that must
produce a counterexample or satisfiable witness.

The model inventory, scopes, correspondence, proof limits, official Alloy
archive hash, and optional Z3 5.x differential are documented under
`validation/architecture`. The stable unattended gate requires Alloy 6.2.0 and
Z3 4.16.0 and performs no network access:

```sh
SOPHIA_ALLOY_ARCHIVE=/absolute/path/to/alloy-6.2.0-linux-amd64.tar.gz \
  tools/check_architecture_models.sh
```

These models are bounded decision evidence, not Rust refinement proofs. The
target models remain pre-schema; their symbolic count, precision, and rate
budgets are not wire constants. Spin/Promela, dependency-policy automation,
and fuzzing remain candidates until they have retained models or corpora,
expected outcomes, and reproducible gates.

### Public WM file contract

The WM role uses 9P2000.L. Complete file records replace the retired WM IPC
begin/chunk/end framing. Run the independent C SDK peer against the production
file export and the generic owner controls with:

```sh
tools/check_policy_protocol.sh
```

This gate runs the WM file envelope, arrays, controls and admission tests;
capability selection; the profile reducer and bounded I/O; peer endpoint
admission; and the Engine projection reducer. Session's worker tests include
an independently compiled C SDK client that exchanges a profile, configuration,
a multi-read snapshot, a projection and a session operation through the real
export. Additional protected C peer tests exercise stale/timeout recovery and
control-driven replacement through Session's supervisor and settlement owner.
The protected recovery test supplies layout failure; it does not prove a
physical resize deadline or presentation. No graphical session is accessed.

The old IPC conformance host, archived revision-3 client runner and their
framing-specific transport tests are retired. Their prior results remain
historical evidence, not claims about this gate. The generic policy reducers
and file tests retain epoch, capability, complete-record, profile, settlement
and recovery checks. The assertion mapping is recorded in the
[IPC retirement investigation](notes/investigations/1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md).

`tools/check_policy_client_matrix.sh` calls the same repository gate. It no
longer builds a sibling WM checkout. Client-specific policy, restart and
physical desktop acceptance runs belong in niltempus and the client
repositories, with their own exact source and artifact bindings.

For Sophia X Authority compatibility changes, also run the focused wire suite
and the real-client smoke that exercises the touched path. The
[X11 compatibility matrix](x11-compatibility-matrix.md) identifies each
probe's precise proven surface and next gate; do not treat this list as a full
X server conformance suite:

```sh
cargo test --offline -q -p sophia-protocol
cargo test --offline -q -p sophia-portal
cargo test --offline -q -p sophia-x-authority --test x11_wire
cargo test --offline -q -p sophia-x-authority --lib font::
cargo test --offline -q -p sophia-x-authority --test x11_wire x_server_frontend_routes_selection_notify_to_the_requestor_client -- --exact
cargo test --offline -q -p sophia-x-authority --test x11_wire cross_namespace_executor_installs_property_and_notifies_requestor -- --exact
cargo test --offline -q -p sophia-portal --test socket
cargo run --offline -q -p sophia-cli -- x-authority-xclock-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xeyes-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xwininfo-root-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xprop-root-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xsetroot-name-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xlogo-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xmessage-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xrandr-query-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xcalc-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xterm-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xterm-render-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xterm-input-smoke
cargo run --offline -q -p sophia-cli -- x-authority-xterm-two-client-smoke
cargo run --offline -q -p sophia-cli -- x-authority-kitty-input-smoke
dbus-run-session -- cargo run --offline -q -p sophia-cli -- x-authority-zenity-smoke
```

The Kitty input smoke is a strict promotion gate. It launches unconfigured
Kitty, waits for two DRI3/Present submissions, verifies the client-visible XKB
mapping, focuses the mapped surface, routes `ll` plus Return, and requires both
the exact shell result and a later Present. A failure is actionable evidence;
do not replace it with a wire-write-only assertion.

Whole-desktop QEMU acceptance and attended GTK/TTY recipes live in niltempus.
Sophia retains their generic evidence readers and protocol regression checks.

The real-client smokes are regression smokes, not full X server conformance
tests. Their reduced output must keep `first_error=none`, report the
proof-window outcome explicitly, and include request/opcode counters so future
client-driven regressions show which compatibility surface changed. The
external probe harness fails if it observes any client-visible X protocol
error, even after a drawing client has already produced authority transactions.
External probe binaries are resolved from `PATH`; set
`SOPHIA_XAUTHORITY_<LABEL>` to override a probe binary path for a local host.
`x-authority-xterm-smoke` is a setup/lifecycle regression, not a rendered
transaction proof; its reduced output is expected to report zero committed
runtime transactions. `x-authority-xterm-render-smoke` is the separate drawing
transaction and materialized CPU-pixel proof. The guarded session tools are the
separate Engine/KMS evidence.
`x-authority-zenity-smoke` is a GTK software-rendering regression. Prefer
running it under `dbus-run-session --` on TTY hosts so GTK reaches its DBus
startup path. It requires a committed surface, a copied nonzero `MIT-SHM`
buffer, and `first_error=none`. Pixel requirements are declarative probe policy;
the frontend does not branch on client names.
Parse-error details include a bounded request head so extension decode failures
show the concrete minor opcode that drove the next compatibility slice.

For live composition changes that connect X Authority transaction intake to
backend-live rendered scanout reporting, run the commands below. These validate
the backend-owned production runtime as well as the Engine, renderer, and
backend boundaries:

```sh
cargo test --offline -q -p sophia-backend-live --features libdrm-events live_session_composition
cargo run --offline -q -p sophia-cli --features native-session -- live-session-composition-smoke
cargo run --offline -q -p sophia-cli --features native-session -- session run --proof --terminal=xterm
cargo run --offline -q -p sophia-cli --features native-session -- session run --display=:177 --max-runtime-ms=6000 --inject-text=sophia
# Operator TTY proof: add --input-devices=/dev/input/by-path/...-event-kbd,
# type into xterm, and require physical_keys_routed>0 plus changed pixels.
tools/live_session_content_hardware_proof.sh
tools/vrr_hardware_proof.sh
tools/build_qemu_session_initramfs.sh
tools/qemu_session_harness.sh
tools/run_sophia_input_latency_qemu.sh
SOPHIA_QEMU_SCENARIO=emergency-recovery tools/qemu_session_harness.sh
SOPHIA_QEMU_SCENARIO=gtk-classic tools/qemu_session_harness.sh
SOPHIA_QEMU_SCENARIO=gtk-confined tools/qemu_session_harness.sh
SOPHIA_QEMU_SCENARIO=session-lock tools/qemu_session_harness.sh
SOPHIA_QEMU_SCENARIO=xtest-selection tools/qemu_session_harness.sh
tools/audit_no_xlibre_runtime.sh
tools/audit_xcentric_runtime.sh
```

The Milestone 4 proof must pass both the software verifier and the strict
schema-14 GPU verifier. A DMA-BUF-only mixed-export diagnostic does not satisfy
the gate: the retained GPU run must include the CPU background layer, positive
acquire waiting, rejection recovery, Flip/Idle and idle-fence activity, and
zero live resources. On an AMDGPU command-stream rejection, capture
`sudo dmesg -T` immediately before another graphical session obscures the
kernel validator record.

After changing deferred admission or production transaction intake, run the
real-client ordering preflight on a host with an openable DRM render node:

```sh
cargo run --offline -q -p sophia-cli -- x-authority-vkcube-admission-smoke
```

It keeps policy-managed mapping deferred, delivers only the generic
`AdmitSurface` control, and requires continued DRI3 import plus two exact
Present Complete/Idle round trips. This is a transport/admission regression;
it does not replace visible native KMS proof.

## Desktop acceptance and benchmarks

The niltempus repository owns installation, desktop workflow acceptance,
standalone client recipes, hardware comparison and benchmark orchestration.
Those checks bind their selected WM, shell and applications to a Sophia revision.
Sophia retains the generic renderer, protocol and archive readers used by them.
Deterministic checks do not establish physical display or input acceptance.
Previous evidence keeps its original source and binary identities.

## Evidence reader compatibility

`tools/check_live_record_schema_readers.sh` checks current emitters against
reviewed readers during `cargo xtask check`. Message identity includes the record
name and status: WM readiness uses schema 4, while other WM records retain their
own schemas. Session completion uses schema 18 when startup proof was requested
and schema 19 when it was not; 16 and 17 were the same pair before the
high-water fields were added, and 17 is not reused. A live session admits XTEST only under `--admit-xtest`,
which is refused beside any input-proof flag, and reports what it injected in
`sophia_live_session_xtest schema=1`. A normal completion is not startup-proof evidence.
The guard checks literal selectors and the registered parsed schema conditions;
verifier fixtures retain authority over required fields and lifecycle assertions.
Run its mutation checks with `--self-test`. New reader purposes or emitter forms
must be reviewed explicitly rather than silently excluded from the guard.

The two-xterm and paired Milestone 3 hardware launchers are retired. They exit
before touching devices or services. Their evidence verifiers and the three-class
archive entry point retain the historical schema ranges and acceptance rules;
they do not certify a current live session. Use the native integration paths below
for current sessions. The Milestone 4 GPU diagnostic remains available: it reads
historical schema 14 and current proof schema 16, accounting for Copy and Flip
separately while retaining its mixed-export and controlled-rejection requirements.

## Installed desktop validation

Installed-session packaging, activation, rollback and physical evidence gates
live in niltempus. They must validate their chosen desktop independently of
Sophia's deterministic repository gate.

## Native-only Surface Audit

Run the architecture guard whenever launch, packaging, policy, or validation
surfaces change:

```sh
tools/check_no_legacy_wm_bridge.sh
tools/check_policy_client_matrix.sh
tools/check_atomic_scanout_local.sh
```

The first gate prevents the bridge crate, bridge runtime variables, legacy
profiles, and bridge launchers from returning. The policy matrix covers generic language-neutral peers. The broad local gate checks source
layout, generated protocol artifacts, launch safety, package behavior, shell
syntax, model inputs, and the self-contained verifier regressions.

Desktop comparisons live in niltempus and never give a foreign WM a Sophia policy socket.

## Client buffer negotiation and pixels

`cargo xtask check` runs the GLX and EGL first-frame proof when a writable DRM
render node is available. It reports the hardware gate as unavailable on a
host without one; a skipped test is not pixel evidence.

```sh
tools/check_client_first_frame.sh
SOPHIA_PIXMAP_TEST_DEVICE=/dev/dri/renderD128 SOPHIA_FIRST_FRAME_REQUIRE_AUX=1 \
    tools/check_client_first_frame.sh
```

The test starts a private X frontend with the production render-device and
pixmap providers and measured modifier capabilities. Separate Mesa GLX and EGL
clients draw known colors. Each accepted DRI3 import must reach a correlated
Present, native renderer capture and exact pixel readback; the retained pixels
must also survive client exit and frontend teardown. The optional auxiliary
requirement pins the compressed-plane regression on hardware that produces
such buffers. Ordinary runs report whether that layout was exercised.

This needs GL/EGL/Xlib development files and uses only a render node. It does
not acquire DRM master, drive KMS, or connect to the live desktop. Normal-login
and physical scanout acceptance remain separate gates.

## XTEST selection between real clients

```sh
cargo xtask check xtest-selection
cargo xtask check xtest-selection --self-test
```

A headless production session (`--no-input --admit-xtest`, a private display
in `:90`–`:99`, isolated configuration) runs
`crates/sophia-session/examples/xtest_selection_driver.rs` as its client. The
driver starts two real xterms, aims XTEST motion at a marker row in the first
and reads the pointer back before any button, drags across it, then
middle-clicks the second. The gate passes only on the driver's exact pass
line, `bounded_complete`, `sophia_live_selection owner_changes>=1
conversions>=2` (xterm A taking PRIMARY, then the driver's read-back and xterm
B's paste asking for it), and a completed `sophia_live_session_xtest` with
`admitted=true refused=0 injected_buttons>=4`. Logs are kept under
`.artifacts/xtest-selection/`.

`--self-test` must fail three mutations: a drag on a blank row, a session
without `--admit-xtest`, and no middle-click. This covers the headless, no-WM
path only. A physical drag, a window manager's session and scanout are
separate evidence. The driver's `--overshoot` argument releases past xterm's
right edge, and `cargo xtask check xtest-selection --overshoot` runs the pass
with it. It was red when filed (the frontend delivered an implicitly grabbed
release by position, to xterm's shell window rather than its text widget),
read green on master a50e393f before t158's frontend seam landed, and t158's
own red and green are its wire tests; the variant stands as the two-xterm
check of the window that took the press.

The same drag and paste under Hagia, with an operator's own configuration
copied into the isolated directory, is a normal session with the driver as
its only startup application; the exact command is in the t124 investigation
note.

The QEMU guest runs it on a scanned-out head with physical input devices
present and nothing typed:

```sh
tools/build_qemu_session_initramfs.sh   # the image carries the driver and DejaVu Sans Mono
SOPHIA_QEMU_SCENARIO=xtest-selection tools/qemu_session_harness.sh
SOPHIA_QEMU_XTEST_ROW=5 SOPHIA_QEMU_SCENARIO=xtest-selection tools/qemu_session_harness.sh  # must fail
```

`tools/verify_qemu_xtest_selection_evidence.sh` reads the same counters from
the guest's serial log, plus the session's application record with the
driver's stdout matched and one bounded completion. The guest runs xterm in
the C locale, so the driver asks for UTF8_STRING and falls back to STRING as
a pasting client does. Rebuild the image after any change to the session or the driver;
the harness runs whatever image is there.

## Session lock in QEMU

The `session-lock` scenario runs the whole lock on physical input, the only
input a lock takes. The guest session is the GTK proof (zenity, classic
profile) with the factotum agent, its PAM helper and real PAM: the guest init
writes a root-owned `sophia-lock` stack (`examples/pam.d/sophia-lock`) and a
test account whose password is `sophialock`. `--inject-session-lock`, a
proof-only option, locks once the physical input proof is armed, because no
in-tree WM can fire a `session:lock` binding; everything after the trigger is
the shipped path. Arming first matters: before it, the proof routes no keys
at all, so a leak could not show. The host types a wrong password, waits for
real PAM to reject it, types the right one, and after the unlock completes
the GTK proof:

```sh
tools/build_qemu_session_initramfs.sh   # carries sophia-factotum, its PAM helper and pam_unix
SOPHIA_QEMU_SCENARIO=session-lock tools/qemu_session_harness.sh
```

zenity's stdout must be exactly the text typed after the unlock, so a key
that leaked past the lock fails the run. `tools/verify_qemu_session_lock_evidence.sh`
checks the record in order (the armed input proof, locking, locked, a
rejected first attempt, an accepted second, unlocking, unlocked) and that
neither password appears anywhere in the evidence. The binding itself and the
attended checks (VT switch, hotplug, agent loss) are t297's.

## X11 conformance profiles and XTS5

```sh
cargo xtask check x11-profile --profile=all \
    --output=$PWD/.artifacts/x11-profile-$(git rev-parse --short=8 HEAD)-all \
    --target-dir=$PWD/.artifacts/x11-profile-target --timeout=1800
```

Both paths must be absolute and under the repository's `.artifacts`: the gate
refuses a relative one ("output and target must be disk-backed children") and a
directory that already exists.

The two independent conformance profiles, `xtest` and `native-input`, run
from a snapshot of the committed source against real private sockets and
must both read PASS. XTS5, the X.Org X Test Suite, runs through the same
gate against `x11_conformance_host` when a built checkout and a purpose
manifest are named; without them the report says `XTS5 BLOCKED`, which is a
statement about the run, never a pass:

```sh
cargo xtask check x11-profile --profile=all \
    --output=$PWD/.artifacts/x11-profile-$(git rev-parse --short=8 HEAD)-xts \
    --target-dir=$PWD/.artifacts/x11-profile-target --timeout=1800 \
    --xts-root=$HOME/src/xts \
    --xts-expected=tools/probes/x11_conformance/xts_expected_selected_core.json \
    --xts-scenario=selected-core --xts-timeout=900
```

`~/src/xts` is a checkout of `gitlab.freedesktop.org/xorg/test/xts`, built
with `./autogen.sh && make`, with `tools/probes/x11_conformance/xts_check.sh`
copied over its `check.sh`. Scenarios and their manifests are enumerated
from the built suite by `xts_select.py`, never typed, following the
preprocessor's inclusions (`>>INCLUDE` and the GC components an
`>>ASSERTION gc` names) so a manifest counts what TET runs:
`xts_expected_selected_core.json` is nine cases around windows, properties,
atoms, selections and focus; `xts_expected_xproto.json` is every core
request's wire test, 122 cases and all 389 purposes, the `TOO_LONG`
purposes among them since t165 let a flooding client be served and t174
framed the BIG-REQUESTS encoding they use; and `xts_expected_arcs.json`
is XDrawArc, XDrawArcs, XFillArc and XFillArcs, 304 purposes across every
GC component, which the mi arc ports (t176, t178, t179) and IncludeInferiors
(t181) pass wherever Xorg's own mi does -- its eight declared WARNINGs are
pixel checks Xvnc fails identically. The other Xlib drawing scenarios
follow it, each run on the host and on Xvnc through the same adapter:
`xts_expected_lines.json` (XDrawLine, XDrawLines, XDrawSegments),
`xts_expected_points.json` (XDrawPoint, XDrawPoints),
`xts_expected_fills.json` (XFillPolygon, XFillRectangle, XFillRectangles),
`xts_expected_rectangles.json` (XDrawRectangle, XDrawRectangles) and
`xts_expected_images.json` (XPutImage, XGetImage, XGetSubImage). On all
five the host passes exactly the purposes Xvnc passes, except two
XGetImage and XGetSubImage purposes that read a window's border: Sophia
draws no window borders, so they are declared by that decision. A text
scenario (the XDrawString and XDrawText cases) does not run yet: its
purposes need the suite's own test fonts on a font path, and every one is
UNINITIATED on Xvnc as on the host. `xts_expected_colors.json` (the Xlib7
colour and colormap cases and Xlib10's install and list cases) and
`xts_expected_gc.json` (the Xlib8 GC cases) followed the same way. Their
declarations are the suite's omissions, the colour classes a TrueColor-only
screen cannot offer, purposes Xvnc fails identically, and t210 and t212.
Some cases are left out: XAllocNamedColor and XLookupColor stop before
their last purposes on Xvnc as on the host, XInstallColormap's fourth purpose is unstable until t210 gives it the ColormapNotify it waits for, and XChangeGC, XCreateGC,
XGetGCValues and XSetFont wait on t201's font path.
`xts_expected_windows.json` is the Xlib4 and Xlib5 window cases (304
purposes, run without XTEST): the host passes 241 where Xvnc passes 277
on the same cases, and the declarations are the suite's omissions,
backing store (not offered), one screen, the no-borders decision for
border pixels, purposes Xvnc answers the same, and t214 to t218 and t227
for the rest; XChangeWindowAttributes, XCreateWindow, XDefineCursor and
XUndefineCursor wait on t201's font path. The event section (`Xlib11`, every
event type, 195 purposes) is `xts_expected_events.json`: it runs with
`--xts-admit-xtest=yes` (the adapter's `--admit-xtest`), which starts the
host with XTEST admitted so the suite's extended purposes inject their
input; the host passes 123 where Xvnc passes 121, and every declared row
that is the authority's names its task (t199, t211, t220). A
manifest is the suite's account of itself: every purpose is
listed, and one the suite or the authority cannot pass today is declared
with its disposition and a reason (`xts_declare.py`, from a real journal and
the reviewed `xts_reasons_*.json`), never removed. The gate reads PASS only
when every manifested purpose starts and meets its declaration, a declared
purpose that starts passing fails it as a stale manifest, and the verdict
line carries the count: `XTS5 PASS (339 passed, 50 declared)` is 50
purposes of debt, each naming its row.

x11bench, an independent Xlib/XRender/Xft drawing suite, runs through the
same gate as a pixel oracle for the fixture host's CPU raster:

```sh
cargo xtask check x11-profile --profile=xtest \
    --output=$PWD/.artifacts/x11-profile-$(git rev-parse --short=8 HEAD)-x11bench \
    --target-dir=$PWD/.artifacts/x11-profile-target --timeout=1800 \
    --x11bench-bin=$HOME/src/x11bench/build/x11bench \
    --x11bench-expected=$PWD/tools/probes/x11_conformance/x11bench_expected.json
```

`~/src/x11bench` is a checkout of `github.com/KarpelesLab/x11bench`, built
with `cmake -B build && make -C build`; its committed references are not
used. The gate re-enters itself inside bubblewrap with a private `/tmp`, no
network or System V IPC and a cleared environment, starts the host and
TigerVNC's Xvnc there at the host's own screen size in pixels and
millimetres (read from each setup reply and required to match, because Xft
derives its DPI from the millimetres), generates the references on Xvnc,
requires Xvnc to pass every test against them, and then runs the host
against the same references. Without the binary, bubblewrap or Xvnc the
report says `x11bench BLOCKED`. `x11bench_expected.json` names every test
the suite lists; one the host does not pass is declared `FAIL` with a
reason, a declaration that starts passing fails the run as stale, and the
verdict line carries the count, as XTS5's does.

## xterm as a pointer oracle

```sh
cargo xtask check xterm-pointer-oracle
cargo xtask check xterm-pointer-oracle --self-test
```

The same headless session and isolation as the selection gate, with
`crates/sophia-session/examples/xterm_pointer_oracle.rs` as the client. It
starts a real xterm whose command turns on SGR button-event mouse tracking
and copies the pty's input to a file, injects XTEST motion, press, drag and
release at chosen cells, and reads xterm's own reports back: `CSI < 0;col;row
M` for the press, `32;col;row M` for each drag cell, `0;col;row m` for the
release, `1` and `2` for buttons 2 and 3. What is in the file is what the
frontend delivered to the text widget, on which window, at which cell, with
which button state. Button-event tracking is the mode that matters: xterm
then relies on its `<Btn1Motion>` translation for drag motion, as an
ordinary xterm does when selecting, so a frontend that delivers motion only
to PointerMotion selectors (t162) fails it on `no_drag_report`; any-event
tracking (`--any-event`) makes xterm select all motion itself and cannot see
that defect. `--self-test` must fail a session without `--admit-xtest` and an
xterm with tracking off. The driver's `--overshoot` releases past the widget's
right edge and is red until t158 lands.

## Atomic Scanout Evidence

The production-shaped scanout preflight and evidence verifiers require atomic
capability, exact request scope, steady-state page-flip delivery, explicit
resource retirement, and the reduced evidence schema. Native DRM object
identities are rejected from retained public records.

Check their deterministic fixtures without taking hardware ownership:

```sh
tools/check_atomic_scanout_verifiers.sh
```

## Keyboard independence on hardware

Sophia's headless tests cover per-device identity, arrival/departure and release
of held state. The attended two-keyboard and ordinary-session acceptance recipes
live in niltempus. Only physical observations establish unplug/replug behavior;
virtual input and deterministic tests are not hardware acceptance.

## Retiring `DEFAULT_DISPLAY`

The `DEFAULT_DISPLAY` EGL smoke is temporary, but it is not removable merely
because the GBM-backed path exists. It can be retired only after the opt-in real
render-node validation is repeatably green and the reduced public boundary is
unchanged.

Current decision: keep `DEFAULT_DISPLAY` for now as a host compatibility smoke.
The real GBM/EGL path has passed repeated local validation on the current
machine, but one host is not enough evidence to remove a broad compatibility
check. `DEFAULT_DISPLAY` remains non-production-shaped; it must not be used as
the compositor platform boundary.

Before removing it, record evidence that:

- `SOPHIA_RUN_REAL_GBM_SMOKE=1` passes after a clean build;
- the same command passes in repeated local runs on the target development
  machine;
- the GBM-backed draw smoke reaches `ClearColorReady`;
- the offscreen presentation smoke reaches `Ready`;
- the reduced frame-target allocation smoke reaches `Ready`;
- `LiveRealGbmSmokeEvidence` records `Passed` without exposing native identity;
- driver crashes remain isolated to child-process validation failures;
- no public report exposes render-node paths, file descriptors, GBM/EGL objects,
  native errors, pixels, KMS framebuffer IDs, connector IDs, CRTC IDs, or plane
  IDs.

If any condition fails, keep `DEFAULT_DISPLAY` as a host compatibility smoke and
continue treating GBM-backed EGL as the production-shaped path under
development.

Minimum host/device matrix before retirement:

- one Intel integrated GPU machine;
- one AMD integrated or discrete GPU machine;
- one machine where `/dev/dri/renderD*` exists but GBM/EGL degrades cleanly;
- one headless or restricted environment where the real smoke is skipped or
  unavailable without failing default validation;
- repeated clean-build runs on the primary development machine.

Each matrix entry must record only reduced evidence: command, pass/fail status,
draw status, presentation status, and whether a child-process crash was
contained. Do not record render-node paths, fd numbers, GBM/EGL handles, driver
error strings, pixels, or KMS object identity.
