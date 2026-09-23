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
`tools/check_shell_protocol.sh`; `tools/check_policy_protocol.sh` also verifies
the frozen revision-3 WM clients against the optional group extension. Hagia and
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

### Public Policy Wire

The draft `sophia_wm_v1` wire has one checked-in KDL schema and retained Rust,
C99, documentation, and golden-corpus outputs. Normal builds do not run the
generator. The gate first checks those outputs for byte-for-byte drift, then
runs the Rust codec and an independently compiled, allocation-free C99 codec
against the same valid and malformed frames. It then drives a standalone C99
client through the authenticated session transport and Engine reducer:

```sh
tools/check_policy_protocol.sh
cargo test --offline -q -p sophia-protocol --test policy_semantics
cargo test --offline -q -p sophia-runtime --test policy_ipc
cargo test --offline -q -p sophia-runtime --test policy_socket
cargo test --offline -q -p sophia-runtime --test policy_transport
cargo test --offline -q -p sophia-engine --test policy_projection
cargo test --offline -q -p sophia-wm-demo --test policy_v1
```

The first command proves generated envelope and record layouts across Rust and
C99. The focused Rust gates prove exact supervised-peer admission, negotiation,
bounded begin/chunk/end assembly, late-epoch discard, semantic record
conversion, atomic multi-output validation, and last-layout preservation. The
Rust reference client and native Hagia client then prove their policy output
through the same reducer.

`tools/check_policy_protocol.sh` additionally runs the Rust reference,
independent C, and immutable archived revision-3 C clients through one
authenticated, eleven-cycle revision-3
behavior corpus. The retained connection observes constrained single-output
layout, two-output partitioning, output loss with surface migration, and the
same raw output returning at a new generation, then an ordered focus action, a
timed-out candidate, a stale candidate superseded by a newer scene, an invalid
candidate, and a successful recovery after each rejection. Committed replies
must pass the canonical reducer without losing an assigned surface or changing
the declared active output; rejected work must not poison later cycles. Each
client also runs the corpus across two supervised processes and fresh
connection epochs; the host pins the last committed projection across the
replacement boundary. Hagia's check below runs both exact sequences.

The authenticated black-box host covers three direct `sophia_wm_v1` peers:
Rust, C, and Hagia. The host exercises normal replacement plus timeout, stale,
and invalid replacement/recovery boundaries. `tools/check_archived_policy_client.sh`
separately verifies fixed
digests before compiling the frozen C99 codec/client snapshot and running it
against the current server. Shared restart and archived-client freeze coverage
are therefore closed. The separately authorized physical output apply/rollback
archive described below supplies the hardware evidence.

The separate, standalone Hagia checkout verifies its independently written Nim
decoder against the same retained corpus, then runs its proof client through
the authenticated Sophia transport and canonical reducer with:

```sh
cd ~/dev/hagia
SOPHIA_ROOT=~/dev/sophia-stack tools/check_sophia_policy.sh
```

The focused live recovery gate runs real Kitty under the public session path,
terminates the first supervised Hagia process, admits only its replacement,
and requires epoch advancement, startup readiness, retained layout, and clean
session/layout health:

`tools/hagia_client_lifecycle_fault_smoke.sh` applies the same replacement
requirements to explicitly armed post-negotiation and complete-snapshot client
faults. Session-operation client faults remain outside that gate until a
deterministic opaque-operation activation driver is retained.

```sh
SOPHIA_HAGIA_BIN=~/dev/hagia/hagia tools/hagia_live_session_smoke.sh
```

This is bounded offline integration evidence. The installed physical output
apply/rollback gate below supplies the separate freeze evidence.

The dynamic-output physical gate is separately armed because it takes
exclusive DRM/input ownership and asks the operator to disconnect and reconnect
one of the connected outputs:

```sh
tools/run_output_topology_gate_tty4.sh
```

Run it from `/dev/tty4` with at least two connected physical outputs. It supplies
the arm and `seat0` defaults, builds Hagia from the adjacent clean signed
checkout so its policy wire matches current Sophia, builds the clean signed
Sophia revision, and preserves timestamped evidence under `/tmp`. Environment
variables remain available for nonstandard rigs. The gate requires one
security-epoch barrier per change, complete `N - 1` loss and `N` return
publications with advancing generations, matching policy settlements, later
page-flip retirements, a surviving Kitty input proof, and clean
non-quarantined shutdown.

Revision 3's final output-authority proof is a separate two-phase TTY4 gate:

```sh
SOPHIA_FRAME_FED_OUTPUT_ARM=1 tools/run_frame_fed_output_gate_tty4.sh
```

It is reference-rig-specific and refuses anything except connected `DP-1`
2560×1440 and `DP-2` 1920×1080. Before taking DRM or input ownership it requires
clean, signed Sophia and Hagia HEADs, then builds and hashes the exact release
binaries. Local signed commits qualify without pushing or fetching; upstream
publication is separate from the identity bound by the proof. The first
normal public-Hagia session applies, first-presents, and publishes the checked-in
profile. The second applies the same startup candidate and forces reverse-card
rollback after final KMS acceptance but before candidate installation. Both
require distinct physical text confirmation and clean teardown. A verified pair
is archived under
`$XDG_STATE_HOME/sophia/promotion/frame-fed-output-runs/`; duplicate evidence,
configuration outside the signed commit, identity drift, forbidden rollback
publication, or checksum drift is refused. This gate changes real output state
and must not be run without explicit operator authorization.

The retained run is frame-fed archive `0001`. It binds Sophia
`870ba46ae231081220b982ecc3a5a95517df7a90`, Hagia
`a83c8fa022a4ceff5d8b96a01c46052bbd8ba64a`, success evidence
`7dbcc54326d48168df930edf88d81f5cf64fb64251f3b2a9b150e159a37431e5`, and
rollback evidence
`267f8b11cc3de692708ee4c634efe6a09b6eb31da992483566e3ba520114f69d`.
Independent archive verification reports `status=passed`, boundary
`after_apply`, and two phases. This closes the hardware gate for stable
interface major 1, wire revision 3.

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

Milestone 5 uses one unattended local QEMU acceptance command. It boots a
diskless, networkless Linux guest that owns virtio DRM/KMS, guest console state,
libinput keyboard and pointer devices, and the Sophia session:

```sh
tools/qemu_milestone5_acceptance.sh
```

The runner rebuilds the initramfs, runs the strict two-xterm native-session
regression, exercises Ctrl-Alt-Backspace emergency recovery, then runs classic
shared-X and confined GTK3 Zenity profiles. The GTK profiles require exact
virtio `sophia` text, a routed pointer button before Return is accepted,
CPU/SHM pixels changed by a committed resize, native presentation on both
guest outputs, normal application exit, zero protocol errors, and clean
retirement. Evidence is retained below `.evidence/qemu-milestone5/`.

`tools/live_session_milestone5_gtk_hardware_proof.sh` remains an optional
compatibility diagnostic for any machine where direct DRM/KMS, VT, and physical
input behavior needs investigation. It is not a milestone gate. Its independent
input guard, bounded process-group termination, and KD/termios restoration remain
fail-closed safeguards for direct hardware use.

The fixture-backed GTK and TTY recovery verifier check remains available through
`tools/check_live_session_milestone5_verifier.sh`.

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
tools/live_session_persistent_hardware_proof.sh
tools/live_session_milestone4_hardware_proof.sh
tools/operator_keyboard_hardware_proof.sh
tools/vrr_hardware_proof.sh
tools/build_qemu_session_initramfs.sh
tools/qemu_session_harness.sh
tools/run_sophia_input_latency_qemu.sh
SOPHIA_QEMU_SCENARIO=emergency-recovery tools/qemu_session_harness.sh
SOPHIA_QEMU_SCENARIO=gtk-classic tools/qemu_session_harness.sh
SOPHIA_QEMU_SCENARIO=gtk-confined tools/qemu_session_harness.sh
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

Before repeating the full paired proof, run
`tools/native_egl_vkcube_mixed_smoke.sh` from the same dedicated text TTY. It
uses the real native-X `vkcube` DRI3 handoff, executes the CPU-plus-DMA-BUF
native EGL export in a watchdog child, stops before KMS submit, and verifies a
single `sophia_native_egl_mixed schema=1` line with positive CPU and DMA-BUF
layers plus zero live presentation resources. The full proof preauthorizes
`sudo` and retains `kernel-before.log`, `kernel-after.log`, and environment
identity beside the software and GPU session logs.

After changing deferred admission or production transaction intake, run the
real-client ordering preflight on a host with an openable DRM render node:

```sh
cargo run --offline -q -p sophia-cli -- x-authority-vkcube-admission-smoke
```

It keeps policy-managed mapping deferred, delivers only the generic
`AdmitSurface` control, and requires continued DRI3 import plus two exact
Present Complete/Idle round trips. This is a transport/admission regression;
it does not replace visible native KMS proof.

Before involving a multi-client native desktop, run the visible single-client
isolation proof from tty3:

```sh
tools/start_sophia_vkcube_standalone_tty3.sh
```

Sophia launches default `vkcube --wsi xcb` directly. The external reference WM
uses its generic `natural` layout policy: it receives only the opaque node,
centers the node's natural allocation, and requests no policy resize. The
ordinary policy-managed deferred admission, X11 Present, renderer, and KMS
paths remain active. The launcher installs its checked-in KDL2 policy template
into the owner-only runtime directory as mode `0600`, so config safety does not
depend on repository checkout permissions. After visually confirming the
spinning cube, use Super-Shift-Q for normal logout and run:

```sh
tools/verify_sophia_standalone_vkcube.sh
```

The verifier requires exactly one presented-frame admission candidate, its
exact visual-admission completion, nonzero scanout pixels, normal logout, zero
protocol/resource debt, and clean teardown. A DMA-BUF candidate must have its
exact page-flip retirement. A software candidate must advance through at least
three authority transactions and produce positive Present Complete, Idle, and
idle-fence-trigger evidence, so a visible but frozen first frame cannot pass.
Presented-frame evidence may use imported DMA-BUF storage or an immutable CPU
snapshot materialized from a software/MIT-SHM Present request. An unresolved
X pixmap, unrelated backing snapshot, blank bordered window, process-only
success, or emergency exit cannot pass. The launcher knows that the validation
client is `vkcube`; Engine, the X authority, and the natural layout reducer
contain no application identity branches.

The staged CPU-buffer regression deliberately separates the first immutable
buffer update from the later released transaction. It must retain the
renderer-private buffer while Engine has no committed surface, then compose
visual detail and route Copy Idle-before-Complete after release. It also separates
an update-only replacement from a later patch, bounds the recent update
working set at 16 handles, and requires post-reduction committed surfaces to
retain a materialized renderer buffer:

```sh
cargo test --offline -q -p sophia-backend-live --all-features \
  --test software_present_feedback
```

Startup failure evidence reports staged and resident CPU-buffer counts,
resident bytes, missing committed buffers, and software Present submissions.
`layout_pending` identifies blocked convergence; `cpu_buffer_missing`
identifies a broken renderer residency root. Do not increase the startup
timeout to make either failure disappear.

To measure the software-Present path after correctness passes, run the bounded
benchmark from the same dedicated tty:

```sh
tools/benchmark_sophia_vkcube_tty3.sh
```

It runs an explicit 500-by-500, 900-frame, FIFO `vkcube` workload, exits with
the startup application, and reports `sophia_rendering_performance schema=2`.
The report derives FPS and p95 frame
cadence from routed displayed Present timestamps rather than process wall time. It
also joins the session's CPU replacement/patch counts, patch rectangles and
payload bytes, exact-versus-damage-scoped metric counts, native composition
target reuse, maximum CPU composition time, maximum native upload time, and
retirement count. The raw report can be regenerated without another graphical
run:

```sh
tools/report_sophia_rendering_performance.sh
```

The terminal CPU-path workload is a separate bounded standalone proof:

```sh
tools/check_bounded_xterm_geometry.sh
tools/check_sophia_terminal_performance_reporter.sh
tools/run_sophia_terminal_gate_tty3.sh
```

Before the physical command, require `sudo sv status socklog-unix nanoklogd`
to report both services running and confirm
`/var/log/socklog/kernel/current` is nonempty. The commit-pinned runner enforces
those checks, refuses a dirty worktree, and archives the session, launcher,
guard, recovery, performance report, and kernel-log delta under
`$XDG_STATE_HOME/sophia/rendering-benchmarks/<commit>/terminal-cpu/`. Run it
from a logged-in local TTY3, arm Ctrl-Alt-Backspace when prompted, and confirm
the centered xterm scrolls continuously. The default 20-second, 500-by-500
pixel intent resolves against the pinned `6x13` font rather than being passed
to xterm as character cells. It emits one line every 16 ms so visible motion
maps cleanly to the display cadence. `SOPHIA_XTERM_LINES=8` retains the previous
burst workload as an explicit stress override; it is not the visual default.
Both profiles keep the software-Present path continuously active. An inner
process-external timer bounds the producer even when terminal
backpressure blocks a write, and its incremental count preserves completed
bursts before xterm's process-level safety timeout. The independent 30-second
watchdog bounds the complete session. Let the xterm exit automatically; the
logout shortcut intentionally produces an incomplete benchmark.

The wrapper runs exactly one physical attempt under `attempt-001/`, promotes
that attempt's artifacts to the archive root, and never retries. After the
benchmark returns, it always asks the operator whether the centered xterm
scrolled continuously, even when the machine path failed. The schema-2
`terminal-gate-result` records independent `machine-status` and
`visual-status` verdicts; the overall result passes only when both pass.

A page-flip stall remains retained evidence for diagnosis. It is not an
automatic retry classification: a clean final `WouldBlock` observation does
not establish where an earlier completion was lost, and repeating unchanged
physical state cannot repair teardown or event-delivery defects. Run the gate
again only after the retained evidence is diagnosed and the relevant code or
system state has materially changed.

The trailing `sophia_terminal_performance schema=6` report retains those
resource, patch, damage, client-metadata, failure, drain, and composition-budget
checks and additionally requires exactly one
`sophia_live_cpu_visual_progress schema=3 status=complete` record with exact
microsecond gap fields. Post-readiness updates must balance exactly as presented
plus superseded with zero pending or discarded updates. The record separately
names native logical-target bindings and lifecycle supersessions. Bindings may
not exceed logical compositions, presentations may not exceed bindings, and
lifecycle supersessions may not exceed all supersessions.

An accepted update carries exact transaction, surface, handle, and generation
identity. Only ready, admitted work may enter the ledger. A target is bound only
after logical CPU or head-composition content is actually queued. Retirement
reads the presented content variant's own logical checksum rather than the
head's retained numeric checksum; mixed and retained-mixed content can neither
acquire nor inherit a logical target. Removing the same surface
lifecycle-supersedes its pending update, while an unrelated removal cannot
settle it. At least three content-changing primary retirements must occur. The
first and final source and display observations retain their one-second liveness
bounds. During steady state, the source gap budget is the greater of three
configured producer intervals or two refresh periods plus one millisecond; the
display gap and accepted-update-to-exact-retirement budgets are two refresh
periods plus one millisecond. A startup-only burst cannot pass.

The default composition budget remains 25 ms;
`SOPHIA_TERMINAL_COMPOSE_BUDGET_MSEC` accepts only a positive integer and is
reserved for a separately documented gate. The raw report can be regenerated
from the retained standalone session log:

```sh
tools/report_sophia_terminal_performance.sh
```

If the machine locks or the report fails, retain the standalone session,
launcher, input-guard, recovery, lifecycle, and protected kernel logs. The
wrapper never retries a failed physical takeover; diagnose its retained
evidence before running another candidate. This benchmark does not establish
Xserver parity. Optional X Present cadence remains a
diagnostic; the gate's screen-progress authority is the session's exact primary
composition and KMS-retirement evidence.

After greetd restores the normal Xorg or XLibre session, open a terminal in
that session and run:

```sh
tools/benchmark_xserver_graphics.sh
```

The Xserver runner compiles a bounded XCB observer, launches the identical
`vkcube` command, and measures the server's Present Complete timestamps. It
does not infer cadence from process wall time. The final comparison refuses
different workload geometry, frame count, Vulkan present mode, Vulkan provider,
or output pixel count. The default gate requires at least 90% of Xserver FPS
and permits at most `xserver_p95 / 0.90`. Override
`SOPHIA_RENDER_MIN_BASELINE_RATIO` only when a documented milestone sets a
different threshold.

The X Present completion path is an observed result, not a comparability input.
An unredirected Xserver may report `Flip`; a composited desktop may report
`Copy`. Both carry advancing FIFO UST/MSC cadence, but `Copy` can complete
before the desktop compositor's eventual scanout. The comparison therefore
labels unlike paths `comparability=cadence_only` and never promotes their p95
ratio to an end-to-end scanout- or input-latency claim. Sophia's mixed/CPU
composition record is post-KMS `Copy`. `Flip` is reserved for a future frame
that reaches direct scanout without composition.

If `glxgears` is installed, the Xserver runner also records a bounded mean-FPS
sample as `role=compatibility_probe`. On Void Linux it is supplied by
`mesa-demos`. This result establishes the reference Xserver's GLX/OpenGL
cadence and exposes gross reference-path regressions. It is not a renderer
benchmark and never supplies Sophia's Vulkan parity threshold. Set
`SOPHIA_XSERVER_GLXGEARS=false` to skip it or `true` to require the binary.

The paired Sophia-side compatibility proof is one command from the dedicated
TTY:

```sh
tools/benchmark_sophia_glxgears_tty3.sh
```

It starts `glxgears` directly in the standalone natural-layout profile, without
Kitty or a shell. The default 500-by-500, swap-interval-one workload
runs for 20 seconds and exits automatically. Before graphics takeover, a
bounded external-client preflight must reach classic visual discovery, direct
context creation, DRI3 import, and Present submission. Move the pointer over
the centered window and confirm that the three gears remain smooth, then let
the bounded client end the session automatically; Super-Shift-Q intentionally
preempts the benchmark completion. The trailing
`sophia_glxgears_performance` record reports the client's sampled FPS
separately from Sophia's routed post-KMS Copy FPS and p95 interval. It also
requires an identified GL renderer, positive DRI3/mixed-composition evidence,
Present idle-fence progress, at least one retained-image cache hit, zero
descriptor mismatch or cache-capacity rejection, no submission or retirement
failure, and clean resource drain. This remains a GLX compatibility diagnostic
rather than a substitute for the fixed Vulkan acceptance workload.

The same benchmark under a synthetic shake, for a cadence claim that does not
depend on a hand:

```sh
tools/benchmark_sophia_glxgears_shake_tty3.sh
```

It creates a virtual mouse through uinput before the session opens its seat,
so udev enumerates it beside the physical devices, then drives alternating
relative motion at `SOPHIA_GLXGEARS_SHAKE_HZ` (default 1000) reports a second
and `SOPHIA_GLXGEARS_SHAKE_AMPLITUDE` (default 8) pixels from the moment the
the client starts rendering until the bounded run ends. Keep hands off the mouse. A
trailing `sophia_glxgears_shake` record, appended to `shake.log` beside the
session log, pairs the rate the probe actually achieved with the client's
sampled FPS and Sophia's present cadence; `status=pass` requires the benchmark
to pass and the shake to have started when the client began rendering rather than on its deadline.
`/dev/uinput` must be writable (`tools/setup_sophia_uinput.sh`).

What the shaken run is for is the benchmark's own rule -- at least 55 FPS with
a p95 of at most 25 ms -- which until this harness existed had only ever been
checked against a fixture. It now holds under 1 kHz motion: 59.9 FPS at a p95
of 16.7 ms, one frame interval rather than two. The cursor record beside it
says why, and is worth reading when the rule fails. `cursor_only` counts the
blocking cursor-only commits taken while the client was drawing and should be
near zero; `rides` should be close to the frame count, because a cursor that
rides a frame going out anyway costs nothing. A high `cursor_only` with a p95
at twice the frame interval is the signature of frames losing their vblank to
the cursor.

An installed desktop reports a nonzero `cursor_only_total_msec` and that is
ordinary: it idles, so the commits that move the pointer on an idle desktop
are taken often, each bounded by one vblank. The benchmark reads zero only
because `glxgears` never stops drawing.

The session log must not contain a CPU submission between the first mixed
Present retirement and its successor, a repeated cold import of one live image
generation, nor an AMD `context is guilty` recovery. The first run that
rendered only a flash of gears violated these invariants: stale CPU fallback
blanked the composed output, and a focus repaint recreated the current
DMA-BUF's EGLImage/texture. That second import blocked in `glFinish` until AMD
recovered the guilty context. These are generic mixed-presentation lifecycle
failures, not GLX workload failures.

The raw reports and comparison can be regenerated without rerunning either
graphical session:

```sh
tools/report_sophia_rendering_performance.sh
tools/report_xserver_rendering_performance.sh
tools/compare_sophia_xserver_rendering.sh
```

Never compare a hardware Xserver run to a Sophia Lavapipe run; provider
mismatch measures the Vulkan implementation, not the compositor pipeline. The
comparison rejects that mismatch by hashing `vkcube`'s provider description.
Offline regressions are
`tools/check_sophia_glxgears_performance_reporter.sh`,
`tools/check_sophia_rendering_performance_reporter.sh`,
`tools/check_sophia_terminal_performance_reporter.sh`, and
`tools/check_xserver_rendering_performance_reporter.sh`.

## Development-Session Readiness

CP-14.3 in [the active roadmap](notes/indexes/plans.md) owns Milestone 14's
current exit: a recoverable Hagia development session using terminal, Firefox,
clipboard, layouts/tabs, two monitors, dependable input, VT recovery, and logout.
The native evidence-lifetime and suspended-deadline implementation passes its
lifecycle model, regression tests, and repository gate. The two
[physical recovery canaries](native-recovery-canary.md) remain the first operator
task. This does not establish development-session workflow acceptance.
Diagnostics improvements and the broader session checklist remain queued.

Start with a short physical Firefox/VT/deadline canary after the lifecycle
regressions pass. The normal-session acceptance check then covers startup,
terminal and Firefox launch, typing/focus, resize, both outputs, basic tabs,
VT return, and clean logout. Full tab behavior follows the
[tab acceptance contract](tabbed-layouts.md#verification-and-operator-acceptance).
The operator can perform normal work; no benchmark controller or foreign-stack
comparison is required. Reuse the existing session entry and fallback path.

For each workflow observation retain the exact source, binaries, profiles,
session identity, outcome, and relevant diagnostics. A usage failure needs a
diagnosis, focused regression where feasible, correction, and revalidation of
affected behavior. Code changes retain `cargo xtask check`; protocol changes
also retain independent client checks. Startup, rendering, input, or lifecycle
changes require the short physical acceptance check and any defect-specific
probe. Documentation-only changes need inspection, link checks, and
`git diff --check`, not an operator session.

Previous evidence keeps its original identity. Relying on it for a newer
candidate requires a recorded impact review; changed behavior requires relevant
retesting. Unrelated changes do not reset every demonstrated workflow. Real use
complements deterministic and physical tests, and cannot establish unobserved
properties. Readiness requires no fixed hour/day counter or consecutive clean
workdays.

Promotion requires observations for every declared workflow and no unresolved
blocking failures: unrecoverable sessions, lost input, application-blocking
failures, visible corruption, undrained work, or unbounded resource growth.
Review warmed resource populations and steady-state allocation growth against
the observed workload, clean teardown, and relevant refresh-relative latency
evidence; record limits rather than extrapolating unobserved durability. Longer
use is useful evidence. The optional two-hour soak and CP-14.2 matrix remain
separate and non-blocking, with their own existing verification requirements.

## Deferred Same-Hardware Comparison

CP-14.2 is deferred and incomplete. Resume it only for an explicitly selected
stable candidate or a named performance investigation. Its 36-row requirement
applies to comparison verification, not permission to use Hagia or close the
revised Milestone 14. Existing artifacts and strict verification remain intact.
The comparison is owned by typed conformance code:

```sh
cargo xtask conformance desktop-comparison install-reference XLIBRE_SOURCE PREFIX
cargo xtask conformance desktop-comparison prepare RUN
cargo xtask conformance desktop-comparison prepare-soak SOAK_RUN
cargo xtask conformance desktop-comparison gate RUN
cargo xtask conformance desktop-comparison status RUN
cargo xtask conformance desktop-comparison attest RUN SUPERVISOR_PID [CRTC]
cargo xtask conformance desktop-comparison preflight RUN
cargo xtask conformance desktop-comparison qualify RUN
cargo xtask conformance desktop-comparison capture RUN
cargo xtask conformance desktop-comparison finalize RUN
cargo xtask conformance desktop-comparison verify RUN
cargo xtask conformance desktop-comparison report RUN
```

Preparation refuses a dirty or unsigned Sophia candidate. It hashes the
repository-owned stack configurations, isolated profiles, capture adapter, and
local Firefox fixture, pins the
candidate and reference-stack identities, and records the common two-output
topology plus detected kernel, Mesa, and GPU identities. The schedule
rotates Sophia, XLibre+xmonad, and niri across three repetitions of Kitty 60 s,
the loopback-only animated Firefox fixture, 120 resize requests, and a 16-Kitty
launch burst: 36 required raw samples total. `prepare-soak` creates a separate
one-row Sophia run for an optional two-hour overnight durability check. The
soak never blocks `verify` or `report` for the interactive matrix.

`gate` owns one complete TTY3 row: it checks the clean prepared commit, builds
before display takeover, selects only the next typed stack, launches it without
an operator application, attests its supervisor, resolves DP-1's active CRTC,
captures, and tears down. `attest` publishes an owner-only local record.
Privileged preflight confirms the DRM completion tracepoint before creating an
attempt, even when tracefs is root-private. Before the first Sophia row,
`qualify` displays four candidate-derived targets and requires physical cursor
motion plus a click in each; that interaction is excluded from the measured
window. `capture` rejects a controller or workload launcher inside the measured
supervisor tree, continuously verifies the exact supervisor and required stack
components, owns an isolated workload, and samples stack, workload, and
aggregate resource populations separately. It uses a private tracefs instance
for authoritative kernel DRM completion timestamps; repeated tracepoint
deliveries of one kernel sequence are counted but do not become extra frames,
and an active cross-card CRTC-index alias is rejected as ambiguous. Kitty
control sockets live in a short owner-only runtime namespace and never load
personal Kitty configuration. Capture stages six raw inputs while the stack is
live. After stack exit and TTY recovery, `finalize` proves the attested
supervisor is gone, records clean teardown, and seals the row. Passive replay
derives the sole schema-4 sample only after the normalized visibility series proves an empty
baseline plus focused workload ownership on DP-1 with zero foreign toplevels,
and duration, resource cadence, frame monotonicity, resize population, crash,
sample-loss, and teardown checks pass. The sealed attempt has an exact file set
and internal checksums; the run ledger separately binds its result to the typed
schedule. A partial capture blocks later progress within its own comparison run
until diagnosed; it does not block the development-session work in CP-14.3.

`verify` requires the exact complete matrix. `report` retains memory,
allocation, CPU/fault, process/thread/fd, launch/settle/resize, and kernel-frame
distribution fields with `verdict=none`. Reference performance is never a
Sophia correctness threshold. The XLibre+xmonad entry is a direct reference
desktop: it never connects xmonad to Sophia and does not define a supported
Sophia policy path.

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

## Native Session Integration

The retained QEMU harness covers only Sophia-native session behavior. It builds
the session artifact from the current workspace and supports the base session,
emergency recovery, and classic/confined GTK scenarios:

```sh
SOPHIA_QEMU_SCENARIO=session tools/qemu_session_harness.sh
SOPHIA_QEMU_SCENARIO=emergency tools/qemu_session_harness.sh
SOPHIA_QEMU_SCENARIO=gtk-classic tools/qemu_session_harness.sh
SOPHIA_QEMU_SCENARIO=gtk-confined tools/qemu_session_harness.sh
```

These scenarios validate Sophia session ownership and X Authority application
compatibility. They do not host an X11 WM as Sophia policy. A WM or shell under
test must connect directly through `sophia_wm_v1` or `sophia_shell_v1`.

The primary physical policy gate is Hagia:

```sh
tools/run_current_hagia_native_gate_tty4.sh
```

The gate builds clean signed Sophia and Hagia checkouts, installs a temporary
native session, exercises policy negotiation and visible output, restores the
display manager, and archives reduced evidence. Use the dedicated text TTY
named by the launcher. The gate is bounded; long soaks are optional overnight
diagnostics and do not block ordinary development.

Focused native physical gates remain available for the mixed-output and
frame-fed output paths:

```sh
tools/run_mixed_output_gate_tty4.sh
tools/run_frame_fed_output_gate_tty4.sh
```

Their archive verifiers bind results to the current schema, exact checkout,
session identity, visual confirmation, teardown, and retained checksums.

## Installed Native Candidate

The [installed operations runbook](operations.md) defines the supported host
boundary, logs, stop/recovery behavior, fallback login, and rollback.

Build and install an immutable release separately from login:

```sh
tools/install_live_session.sh
```

A base artifact provides the diagnostic Sophia session. A Hagia-enabled artifact
adds the production-shaped native WM/shell entry. Packaging records exact
digests and Git identities, and installation verifies the artifact before an
atomic `/opt/sophia/current` switch. No package contains an X11 WM bridge, an
embedded legacy WM, or bridge-specific configuration.

Local installation does not require pushing or fetching either repository.
Hagia may be ahead of `origin/master` or have no remote-tracking branch;
packaging still verifies its source-commit signature and committed default
profile, records exact commit and binary hashes, and requires a clean Sophia
worktree. Publication is a separate step. Rebuild changed Hagia sources before
`just install-session`, since the installer can reuse an existing executable.

Run the self-contained packaging/install regression with:

```sh
tools/check_live_session_install.sh
```

It proves schema and digest validation, rejects retired bridge fields and
artifacts, checks executable native components, exercises base and Hagia
activation, verifies rollback, removes only Sophia-owned stale entries, and
preserves foreign desktop entries.

Installed native evidence can be inspected with:

```sh
sophia-status
sophia-verify-recovery
sophia-verify-login-cycle
sophia-verify-truecolor-runs 1
sophia-verify-xterm-runs 1
```

The Firefox, TrueColor, xterm, watchdog, emergency-recovery, runtime-identity,
and login-cycle recorders all identify the Hagia native session. Their verifiers
consume checksummed archives and fail closed on an unexpected revision, binary
identity, result, protocol fault, or teardown residue.

Use `sophia-stop` or the independent recovery entry to leave a failed session.
Neither route depends on the policy process continuing to answer.

## Native-only Surface Audit

Run the architecture guard whenever launch, packaging, policy, or validation
surfaces change:

```sh
tools/check_no_legacy_wm_bridge.sh
tools/check_policy_client_matrix.sh
tools/check_atomic_scanout_local.sh
```

The first gate prevents the bridge crate, bridge runtime variables, legacy
profiles, and bridge launchers from returning. The policy matrix requires the
language-neutral Rust, C, and Hagia clients. The broad local gate checks source
layout, generated protocol artifacts, launch safety, package behavior, shell
syntax, model inputs, and the self-contained verifier regressions.

The direct XLibre+xmonad desktop-comparison profile is the sole active xmonad
exception. It is an external baseline measured beside Sophia and never receives
a Sophia policy socket.

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
separate evidence.

## Atomic Scanout Evidence

The production-shaped scanout preflight and evidence verifiers require atomic
capability, exact request scope, steady-state page-flip delivery, explicit
resource retirement, and the reduced evidence schema. Native DRM object
identities are rejected from retained public records.

Check their deterministic fixtures without taking hardware ownership:

```sh
tools/check_atomic_scanout_verifiers.sh
```

## Keyboard Independence on Hardware

Two physical keyboards on one seat, for t094. The backend mints an identity
per device and announces arrivals and departures on the packet stream; the
session releases whatever a departed device still held that the kernel did
not. Headless controls pin each of those pieces; hardware shows them on two
real keyboards, one of them unplugged and replugged.

**The ordinary-session path, which is the acceptance path.** Install the
candidate as the live session (`just install-session`), log in, type on the
keyboard you will unplug, unplug it, type on the other, plug it back in, type
on it, and log out; typing on the other keyboard before the unplug instead
is also accepted. Keep the session short: the recorder rotates early segments
of a long session away. Then:

```sh
tools/verify_keyboard_independence_session.sh            # newest finished session
tools/verify_keyboard_independence_session.sh ~/.local/state/sophia/sessions/<id>
```

It reads the session's manifest (release commit and binary digest), outcome
and lifecycle (a clean, installed, non-emergency exit), health (no storage
errors) and the retained event records, and requires two hardware keyboards
typed on, a removal of one of them, keys routed by the seat between that
removal and the return, the return announced under an identity never seen
before and typed on, and no class-identity fallbacks. The kernel releases a
USB keyboard's keys itself when it goes, so `released=0` on the removal is
the ordinary reading; a nonzero count must carry the session's flush record.
`tools/check_keyboard_independence_session_verifier.sh` pins the verifier
against a fixture session and sixteen mutations and runs under
`cargo xtask check`. The summary it prints is the evidence for the milestone
note. A uinput keyboard is admitted and announced with `virtual=true`; the
verifier requires hardware on both sides, so a rehearsal with a virtual
keyboard exercises the path but never satisfies the claim.

**The attended gate, optional.** `tools/keyboard_independence_physical_gate.sh`
through `tools/run_keyboard_independence_gate_tty4.sh` (any text console but
the display manager's) adds what a passive record cannot witness: the
emergency chord split across the two keyboards must not arm the input guard,
on the seat and pinned to one keyboard, and the proof phrase typed after the
unplug must be unshifted. It runs two guard phases and a session with a guide
in Kitty, verified by `tools/verify_keyboard_independence_physical.sh` and
archived by `tools/archive_keyboard_independence_physical_run.sh`;
`tools/keyboard_independence_walkthrough.py` checks its preconditions, names
the keyboards and explains each phase. It refuses while a key remapper such as
keyd runs, since a remapper presents every keyboard as one virtual device.

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
