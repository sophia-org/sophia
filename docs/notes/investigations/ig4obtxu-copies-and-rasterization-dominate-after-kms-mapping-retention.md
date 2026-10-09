---
id: ig4obtxu
date: 2026-10-05
kind: investigation
status: investigating
tags: [investigation, rendering, x11]
---
# Copies and rasterization dominate after KMS mapping retention

## Native cache result refused on composition counts (2026-10-07)

Fixture04 completed the excluded C smoke and all eight **BC CB CB BC** arms.
Native compatibility, fixed geometry, library identity, application cleanup
and declared timing guards passed. The frozen verdict is **NOT_ACCEPTED**:
candidate completed compositions were four fewer in pairs 2 and 4.

| Pair | Baseline desktop CPU | Candidate desktop CPU | Raw saving | B/C whole-Session compositions |
| --- | ---: | ---: | ---: | ---: |
| 1 | 1.53 s | 1.41 s | 7.84% | 1948 / 1948 |
| 2 | 1.63 s | 1.33 s | 18.40% | 1952 / 1948 |
| 3 | 1.62 s | 1.41 s | 12.96% | 1944 / 1949 |
| 4 | 1.53 s | 1.35 s | 11.76% | 1948 / 1944 |

Median raw paired saving was **12.36%**, with lower owner CPU in every pair.
Each arm measured 1,800 frames at 60.03 fps and captured/promoted/evicted 1,920
snapshots including warmup. No pending supersessions occurred. Candidate
maximum submit-to-flip was 16 ms in every pair, baseline 17/18/16/16 ms; depth
was two throughout. Report-only in-flight owner ticks were B 0/0/0/0 and
C 2/2/0/0. These are service visits, not elapsed latency. No causal explanation
of that tick difference is established. The raw improvement does not waive
the failed work rule; the cache remains unmerged and uninstalled.

The extra baseline work is not explained by startup/teardown spread alone.
In pair 2, two additional two-head queues follow client retirements 216 and
407 during animation; pair 4 has the same pattern after 208 and 409. DP-1
repeats the preceding mixed Present's logical checksum and DP-2 has the empty
checksum. Queue admission is not a worker-completion ledger, and the checksum
is not independent pixel proof. Also, `scene_generation=642` on a mixed
Present is its transaction ID, whereas `216` on the retained source set is
the committed surface generation. Their numeric order does not prove older
pixels. Claude's startup-only and older-generation readings were withdrawn.

Source narrows the next control to ordinary cadence repaint admission. A
non-Present authority batch is cadence-eligible even with displayed GPU
content; GPU preservation can report `composed=false`, which arms the pacer.
Once a Present retires, the ordinary repaint draws retained sources through
`OrdinaryScene`, without the retained queue's checksum suppression. This can
produce the observed `head_composition` records. In both refused pairs the
baseline has two more cadence repaints, but no per-repaint reason identifies
the exact triggering batches. It remains a hypothesis, not a diagnosis.

Next add a CPU/runtime control for a nonvisual batch over a displayed DMA-BUF
surface, with controls that preserve genuine CPU, chrome and layout damage.
Then distinguish required client work from optional recompositions inside the
CPU bookends. Do not add a tolerance from observed spread or repeat the same
series hoping its totals match. Any changed acceptance rule needs a fresh
declaration; this verdict stays unchanged.

Evidence: `t289-native-kms-property-cache-04/qualification-20261007T224213.594340Z/RESULT.json`,
`RUN.SHA256SUMS` (1,442 original files), `DISPOSITION.txt`, `QUEUE-SPREAD.json`
and `SOURCE-TRACE-01.txt/json`. The 13 traced source files are identical in B
and C. No new GPU test or measurement was used for the source follow-up.

### Subsequent hardware change and login failure

After the series, the operator moved the 1080p monitor from discrete DP-2 to
integrated HDMI-A-2. Sysfs associates discrete DP-1/renderD128 with PCI
`0000:03:00.0`, and integrated HDMI-A-2/renderD129 with `0000:16:00.0`.
Fixture04 describes the preceding two-head, one-card setup; it cannot be
reused unchanged. Render-node-only correctness tests can select the iGPU,
with the discrete nodes hidden. Concurrent full Session/KMS tests require
explicit card and seat/VT isolation, and performance comparisons still need
quiet host CPU and memory activity.

The next installed login returned to greetd on release `825d91460`, before the
cache candidate. Both heads reported ready, then startup failed 109 ms later;
TTY recovery was clean. No cause text was saved: the installed wrapper sends
uncaptured stderr to `/dev/null` in daily diagnostic mode. Evidence is copied
with hashes in `igpu-login-exit-20261007`.

The first bounded local-TTY retry stopped before takeover because the input
guard was not armed. The second armed, reached Session and saved
`Error: UnknownConnector("DP-2")`; recovery was clean. The installed profile
still names disconnected DP-2. The startup projection enumerates lit heads,
so that connector is absent rather than present with `connected=false`.
This refusal does not require a second GPU. The retry uses the installed
proof route with a watchdog and raw capture, not the ordinary supervised
login route; it explains that retry, while the original daily cause remains
unrecorded. Neither run establishes a cross-GPU renderer defect.

The operator chose the iGPU monitor for development. Niltempus `83c34a45e`
therefore names only DP-1, assigns workspaces 1 through 6 to its policy key,
and sets output `inherit-sophia #false`. Naming HDMI-A-2 as disabled would
still require it in the startup snapshot, so it stays unnamed. The desktop
build passed with the existing lockfile and unchanged Sophia/Hagia/kleis
binaries. The operator installed `niltempus-f18fc2ed5aa55e0f6132` and reported
being back in the live session. At 23:25 UTC the ordinary supervised login
was running the new release, DP-1 was enabled/On and connected HDMI-A-2 was
disabled/Off. Diagnostics were recording with zero storage errors and no
fatal record in the observed snapshot. This accepts startup on this profile,
not a complete-session, lock or hotplug gate. This excludes the iGPU output
at profile reconciliation, not at device discovery: card/seat isolation
remains separate work. The read-only receipt is
`profile-dp1-release/LIVE-LOGIN.txt/json`.

Evidence: `ATTEMPT-01-DISPOSITION.txt`, `ATTEMPT-02-DISPOSITION.txt` and
`profile-dp1-release/` under `igpu-login-exit-20261007`. The daily record's
missing refusal reason is tracked as [t309](../plans/queue-05-3-make-failures-diagnosable.md#t309).

### t310

The approved [runtime policy and GPU admission plan](../plans/cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md)
extends the accepted startup fallback below. niltempus chose safe fallback for
unsupported settings; strict profiles retain their refusal contract.

#### Admitted discovery and startup boundary (2026-10-09)

Signed `5fabe9f61` introduces stable GPU admission and a revalidated fresh
render-node opener. The next boundary candidate adds the pure adaptive resolver,
GPU-qualified connector selectors, identity-changing reload refusals and passive
backend discovery. Session resolves that inventory before constructing selected
heads. Disabled connectors cannot consume their CRTCs or planes. Native requests
retain the exact advertised full mode timing. Startup waits without launching
policy or applications when adaptive resolution has no usable output, while
servicing seat release and host-admin logout. The same Session resolver is the
handoff to t306's retained-image continuity hook.

The isolated backend/config/Session suite passed: 124 test-result summaries,
2,499 reported passes, zero failures and 51 ignored (summaries include nested
child-process tests). After the startup activation code was split by ownership
and GPU-qualified lookup corrected, Session lib tests passed with 896 passed and
26 ignored; native output topology passed 12/12, including two GPUs sharing both
a connector name and connector number. Three-crate all-target/all-feature clippy
passes with warnings denied. Source layout passes with no new debt, including the
test-module layout correction Claude found in the opener commit.

Evidence is `~/.local/state/sophia/development-evidence/t310-runtime-20261009/`:
`config-01`, `admission-01`, `admission-tests-01`, `focused-02`, `discovery-01`
and `replacement-01`. Failed intermediate checks remain recorded. The exact
boundary commit and source hashes are recorded in `replacement-01`.

This qualifies the boundary for integration, not a live release. Runtime hotplug,
seat reacquisition and recovery still need the continuity hook; committed
geometry/key publication, bounded conservative hardware retry, durable output
policy diagnostics, the combined gate and attended acceptance remain open.
The installed release and master are unchanged.

#### Port change after reboot (2026-10-09)

niltempus moved the main monitor between ports on the discrete AMD card before
rebooting and requested a durable startup repair. Both normal logins failed
on installed Sophia `19403a511`, release `niltempus-087445319affcb9bfc53`, with
`output_profile_unknown_connector` and the retained cause `unknown connector
"DP-1"`. Both reached native head readiness. Their diagnostics report zero
suppressed, discarded or lost records. Sysfs shows discrete PCI `0000:03:00.0`
with DP-1 disconnected and DP-2 connected; the integrated card at
`0000:16:00.0` retains HDMI-A-2. The udev rule assigns that integrated card and
render node to seat1; the discrete card remains on default seat0. The installed
profile still requires DP-1. This establishes a configuration availability
refusal, without requiring a rendering-failure hypothesis.

Evidence is `~/.local/state/sophia/development-evidence/t310-startup-port-20261009/`.
Its `incident/` retains both failed sessions, the installed profile, udev
records and connector snapshot with `SHA256SUMS`. Session IDs end in
`19724b5a-ac06-4bea-9b7e-7154eb41f9e5` and
`02bedb1b-176a-4d8a-8a89-4194567bd4d4`.

The repair is on `fix/t310-startup-output-fallback` in
`~/dev/sophia-output-fallback`, based exactly on installed `19403a511`.
Signed implementation `0a95d308d` adds explicit adaptive availability while
strict remains the default. Missing ordinary preferences are skipped; if no
output remains enabled, the least connected unnamed connector receives a
preferred-mode desktop, automatic scale, normal transform, VRR off and focus.
All named connectors and mirror members are excluded from the fallback pool.
Mirrors and unsupported settings on present named outputs remain strict.

The explicit fallback policy key transfers workspace affinity to that port
for the session and removes every previous claim to the same key. Reloads
that change configured keys, availability or fallback key are declined before
staging a replacement WM. Review found that the previous publication error
propagated out of the owner loop and ended the session; the new early check
returns `Declined` and preserves its generation, profile, bindings and WM.
Topology reload also refuses a change to the realized startup binding.

Signed follow-up `838d5b16acad9a6afd119ce25b8622c068b32646` moves the five
prepared shell accessors unchanged into their own module, preserving the
public facade. `full-gate-01.log` passed tests and clippy but refused the
profile parser at 1,002 lines. The split resolves that failure without
relaxing the debt ledger. `full-gate-02.log` exits zero on the final signed
source, including workspace tests, both SDK checks, clippy, layout, verifier
controls and six retained direct-scanout archives. The source matches
`candidate-02-source.SHA256SUMS` after the run. Nineteen new controls cover
configuration, startup affinity and safe reload settlement.

The gate ran with no DRM/input devices, no network and a private target.
Earlier focused runs exposed fixture files made group-writable by the host's
umask 0002. `isolated-private.py` sets umask 0077 before launching the checks;
the original wrapper and failed logs are retained. No unrelated fixture
permission patch remains. Device-dependent pixel checks explicitly report
that they are not proved in this environment.

The matched integration commit is signed
`73498befe6c656707c34f83ecfdce6b4ca3c30c3` on
`fix/t310-daily-output-fallback` in `~/dev/niltempus-output-fallback`. Its
profile enables adaptive fallback key 1 and explicitly disables HDMI-A-2,
including when absent. Its lock binds Sophia to the exact local signed commit;
preserve the source worktree until that pin is replaced by a published source.
The prior t306/t307 packages and unqualified master changes are not included.

Nix built `niltempus-19efce64b00ae803d566` at
`/nix/store/6lhvb1x9a531gbavv93qpxf6iykm1spa-niltempus-desktop-niltempus-19efce64b00ae803d566`.
The packaged profile passes `status=accepted policy=validated`; all packaged
checksums pass. Sophia SHA-256 is
`96f7f48f5008b5f9a5a980746330814c17b5eabf10f10205c504fc7a590cc16f`.
All client and factotum binaries match the installed baseline byte for byte.
`release-build-01.log`, `release-binaries.json` and `RESULT.json` retain the
build and verification identities.

Installation was first attempted through the reviewed `tools/desktop install`
command but sudo required a password and the tool terminal had no graphical
authentication agent. No release copy or current/previous switch occurred.
The initial `RESULT.json` retains that pre-install disposition. On DP-2,
fallback uses the EDID preferred refresh rate with VRR off; the saved 120 Hz
and VRR preference is still specific to DP-1.

Automatic hotplug reconstruction still does not rerun profile reconciliation.
Runtime exclusions, workspace migration, all-head suspension and retained-image
recovery remain separate obligations with t306. This startup repair does not
close t310 or change the prior shutdown checkpoint's unqualified evidence.

#### Startup accepted on DP-2 (2026-10-09)

niltempus subsequently reported being back in the live session. Read-only
checks confirm `current` is `niltempus-19efce64b00ae803d566`, with the prior
`niltempus-087445319affcb9bfc53` retained as `previous`. The normal Session
process, PID 10449, runs the new installed path on seat0 with Hagia over
9P2000.L. Session `00000001791549817928-0337103c-3efd-4c02-b52a-977f4b9caed3`
records commit `838d5b16a` and the exact verified binary hash above. Sysfs
shows DP-2 connected/enabled, DP-1 disconnected/disabled and HDMI-A-2
connected/disabled. This accepts startup fallback for the reported port move.

`LIVE-LOGIN.json` and `LIVE-LOGIN.SHA256SUMS` bind this observation to a
checksummed preserved session under `~/.local/state/sophia/session-investigations/`,
suffix `-0dbccf62-3e7b-4646-b88c-d4092c3f09dd`. The running-session snapshot
reports zero discarded records and storage errors, but 20,317 records were
suppressed by per-kind volume limits. It is incomplete event evidence, not a
whole-session verdict. No live topology, reload or lock intervention was made.
Runtime loss/return and card admission remain open under t310 and t306.

The operator expects the desktop to handle changing monitor combinations at
login and during a session. The DP-1 profile is an immediate configuration
repair, not that capability's acceptance. Define saved output settings as
preferences for available connectors: an absent preferred monitor should
not prevent login on another eligible output. Explicit exclusions still
apply, including the operator's iGPU development monitor. Do not silently
reclaim an excluded device when selecting a fallback.

Specify startup fallback and runtime reconciliation together, including
workspace and window reachability on surviving outputs, reconnect affinity,
and suspension with retained state when no eligible output remains. Keep
required-output semantics explicit for proof fixtures that depend on exact
geometry. Resolve how policy keys and unnamed connectors are assigned before
changing the current strict reconciliation rule.

Acceptance needs startup and loss/return controls across different monitor
sets, no outputs, and an explicitly excluded GPU. Include static DMA-BUF
clients so losing a head cannot discard their only retained source. The
runtime continuity repair is t306; neither these profile edits nor a passing
startup check close it. Full concurrent development sessions also still need
separate card and seat/VT ownership.

Card admission must be specified separately from output policy. Excluding a
connector from the layout does not prevent Session from opening its DRM card
or render node. The iGPU passthrough preparation exposed this distinction:
the live Session retained card1 and renderD129 while HDMI-A-2 was excluded.
Today admission follows udev's seat assignment; a missing `ID_SEAT` means
seat0 only for an initialized record.

Decide whether the profile exposes a card exclusion or allowlist whose
meaning is "do not open this card". Use stable PCI/udev identity, such as
`pci-0000:16:00.0`, rather than `cardN`. An absent excluded card must not
prevent login. Profile selection may only narrow the cards assigned to the
session's seat; it must not grant access to another seat. Specify the same
boundary for render-node discovery and shell GPU selection so neither can
reopen an excluded card. Controls must cover changed card numbering, absent
exclusions, foreign-seat cards and a shell render-node request that conflicts
with admission. This remains t310 design work, not a promotion or a device
ownership change. The separate udev seat rule used for iGPU preparation does
not establish profile-level card selection.

## Native cache comparison reached the workload (2026-10-07)

Fixture03's explicit local-session lookup worked. The candidate smoke and
first baseline/candidate runs completed at the exact DP-1 geometry with
radeonsi, clean application groups and restored TTY modes. The comparison
stopped at its frozen requirement that `pending_frame_supersessions` be zero:
the paired candidate recorded one. The original result remains **FAILED**;
there was no replacement run and no cache merge or installation.

| Run | Desktop CPU over 1,800 measured frames | Completed compositions, whole Session | Observed pending replacements |
| --- | ---: | ---: | ---: |
| Candidate smoke, excluded | 1.41 s | 1,953 | 0 |
| First baseline | 1.53 s | 1,949 | 0 |
| First candidate, refused by validator | 1.32 s | 1,949 | 1 |

Each measured interval was about 30 seconds at 60.03 fps. Baseline and
candidate both captured, promoted and evicted 1,920 snapshots including
warmup, with six imports and no renderer failure, slot deferral, exporter
replacement or direct-scanout work. The paired candidate had 1,941 cache hits,
two discoveries, zero discovery failures and zero invalidations. These are
adapter counts, not a count of kernel ioctls saved. The raw CPU difference is
13.7%; one refused pair does **not** establish a repeatable performance gain.

The replacement counter measures pending exporter frames overwritten before
worker submission, plus latest-wins deferrals and direct fallbacks. Those
last two paths are excluded by this recipe. A pending replacement is not
itself a failed client Present. The owner samples a maximum of the summed
counters per tick, so the terminal value can miss later overwrites: it is
an observed lower bound, not an exact offered-frame ledger. Engine damage
history is based on committed states; a superseded candidate does not advance
the next candidate's damage baseline.

Fresh fixture04 therefore reports replacements by arm and pair while retaining
all client/snapshot checks and requiring candidate completed compositions at
least equal to baseline in every pair. CPU is never normalized by this count.
All pairs must improve, with a median saving of at least 5%. It also explicitly
requires zero page-flip phase and overlap rejections, and adds pairwise bounds:
candidate maximum submit-to-flip no more than baseline plus one reported
millisecond, and no increase in maximum in-flight depth. Deferral-decision
counts and maximum in-flight owner ticks remain descriptive; they are not
elapsed time. These whole-Session maxima do not prove latency percentiles
over the measured interval. A late candidate startup flip can refuse a pair;
a late baseline flip can loosen that pair's limit.

Seventeen validator/driver controls pass, including rejection of fewer
completed compositions despite supersessions and cheaper CPU with worse
timing. Three mutations are detected; the first runner's assertion-only
detector stopped on the expected old-rule exception and its log is kept.
Both argument/profile checks pass. Arm binaries, settings, wrapper and
workload are unchanged. Fixture04 has no native result yet and requires a
fresh quiet window and local tty3 launch.

Evidence: `t289-native-kms-property-cache-03/DISPOSITION.txt`, the original
`qualification-20261007T112207.476147Z/RESULT.json` and
`FAILED-RUN.SHA256SUMS`; successor `t289-native-kms-property-cache-04`, frozen
manifest `45826909111a3dcdf67c8c41a00c891607bf3a40bdefc82b59d91c495f112e78`.
Claude's independent source check agrees that observed supersessions can be
reported with these work guards. His concrete fixture review found no blocker.
Its requested descriptive tick field and timing-limit wording are in the final
freeze `07ef8edc0c2f68f44a6e5c85d4036c7d4786c086d12d7c7a5f645cb23b7b5965`;
the reviewed predecessor is kept in `reviewed-freeze-01`. All 17 controls pass
on the final source.

Separate cleanup follow-up: the wrapper kills its watchdog subshell but leaves
the current `sleep 270` child until expiry. Three such idle orphans were
observed after these runs and expired naturally; no Sophia, benchmark or perf
process remained. Fix watchdog-child cleanup separately and exercise normal
completion and forced shutdown. The application-group receipt does not cover
all wrapper descendants.

## Native comparison refused session lookup (2026-10-06)

The corrected record parser in `t289-native-kms-property-cache-02` passed review,
but its candidate smoke at `20261007T014040Z` failed before native graphics
startup: `libseat open failed: No data available`. No benchmark frames or
comparison pairs ran. TTY modes and the application group recovered cleanly.
The failed result and logs remain unchanged, bound by `FAILED-RUN.SHA256SUMS`;
`DISPOSITION.json` records the interpretation. The candidate bytes are identical
to fixture01's successful native smoke. This is no performance result.

Read-only queries reconstructed a concrete login-selection failure. On this
elogind host, tty3's shell belongs to session 32 with cgroup `0::/32`. Host
libelogind identifies it correctly. The Nix libsystemd used by pinned libseat
returns `-ENODATA` for the same PID, then its user-display fallback returns
session 69, an SSH session with no seat; querying that seat also returns
`-ENODATA`. Both libraries accept explicit session 32 as active on seat0.
The pinned seatd 0.9.3 source, `libseat/backend/logind.c:607-667`, first honors
`XDG_SESSION_ID`, otherwise uses PID lookup and then the user-display fallback.
Our minimal benchmark environment omitted the explicit ID. The failed process
did not record which ID it chose, so these are a post-run reconstruction and
source explanation, not a recovered trace of its lookup.

Successor fixture03 resolves the launcher's own PID through the pinned host
libelogind and refuses unless UID, local/user/tty class, seat0, active state,
TTY, VT and exact cgroup agree. Two observations must retain the process and
session identity; an inconsistent inherited `XDG_SESSION_ID` refuses. It records
the session file and passes only that validated ID into both arms. This happens
before the wrapper can change TTY state. It never chooses the user-display
fallback, changes seats or opens a device during preflight. Normal launches
that already carry the correct PAM session ID use libseat's explicit-ID path;
the Nix/elogind mismatch matters when that variable is omitted.

Seven new seat controls and all twelve existing comparison controls pass. The
real host query accepts the tty3 shell and rejects the remote tool process;
both argument/profile checks pass. Claude reviewed fixture03 ACCEPT, verified
the same controls and host queries, and supplied a fresh quiet ACK. Frozen
manifest SHA-256 is
`129181bd4fa653d6c20bb03f9de1e952550e7ec39b34470f7b8befa0cca126a2`.
The stale copied `REVIEW-REQUEST.txt` remains frozen; `REVIEW-REQUEST-03.txt`
explicitly corrects it. The candidate/baseline binaries, wrapper, workload,
acceptance rule and comparison order are unchanged. Native execution remains
operator-initiated on the active local TTY: one C smoke then BC CB CB BC, stop
first failure, no replacements. No cache merge or install is implied.

## Question

After retaining Mesa's software framebuffer mappings, what accounts for the
remaining desktop CPU cost? Does the evidence justify SIMD or assembly work,
or is there still unnecessary work to remove?

**Moving the owned raster command into its journal reduced median CPU by 6.3%**
in the fixed software workload, accepted on October 6. The
[measured result below](#owned-journal-move-accepted-2026-10-06) records the four
pairs and their limits. The [native DMA-BUF profile](#native-dma-buf-attribution-2026-10-06)
now puts the owner loop first: 0.86 of 1.49 desktop CPU seconds over 30 seconds.
Repeated KMS property discovery is the next concrete candidate. The separate
owned-upload candidate still needs measurement on the software recipe.

## Evidence

One Sophia profile followed by one unprofiled control completed on October 5
local time. Both used the unchanged wmbench `47fdb6b` bundle, Sophia
`825d91460`, and the measured Mesa mapping-retention candidate enabled in both
arms. The host was logged out at greetd, with other development work paused.
Each guest had four CPUs, 3072 MiB and a virtio 2D GPU running llvmpipe. The
1160×680 SHM client ran 120 warmup and 300 measured frames on a 1280×800 output.
No host GPU was passed through.

| Run | Desktop CPU per 300 frames | Elapsed |
| --- | ---: | ---: |
| Profile | 1.47 s | 39.0 s |
| Unprofiled control | 1.42 s | 38.9 s |

Both guest and host wrappers exited zero, both passed compatibility and the
48-frame Mesa pixel/cleanup helper, and loaded binary identities matched.
The control agrees with the earlier ON range of 1.41–1.44 seconds. One pair
does not establish repeatability or a stable profiler overhead. The workload
name says “render 60 fps”; achieved throughput was about 7.7 fps in this guest.

The profile recorded **zero minor and major faults** over the measured desktop
interval, with 1.38 seconds user CPU and 0.09 seconds system CPU. There were
713 CPU-clock samples at a 2 ms period, none lost: 1.426 seconds of sample
weight against 1.47 seconds of process accounting.

| Thread group | Samples | Share |
| --- | ---: | ---: |
| llvmpipe raster workers | 307 | 43.1% |
| X11 client worker | 188 | 26.4% |
| Session owner | 159 | 22.3% |
| Renderer group worker | 59 | 8.3% |

These groups partition the samples. Symbol costs below overlap them:

- `memmove`: **233 samples, 32.7%**, comprising 170 on the X11 worker,
  43 on the owner and 20 on the renderer worker.
- Owner `memset`: 23 samples, 3.2%.
- Mesa `util_fill_rect`: 49 samples, 6.9%.
- Unresolved Mesa JIT code: 209 samples, 29.3%.
- Kernel code: 48 samples, 6.7%.

Whole-session work matches between the two runs: three targets, 429 target
reuses, 432 worker requests and completions, 421 exact-nearest draws, zero
snapshot captures/imports and zero COW splits. Those counters include startup
and warmup. They must not be divided by the measured 300 frames as though they
cover only that interval.

## Finding and resolution

### First candidate: move the command already owned by the journal

At the measured source, in
[raster_variants.rs](../../../crates/sophia-x-authority/src/software/raster_variants.rs):

- `XOwnedImagePixels` owns a `Vec<u8>` and derives `Clone` (lines 134–138).
- `from_put_image` retains the accepted pixels with `to_vec()` (line 231).
- `SurfaceRasterStore::record` takes its command **by value** (line 609),
  then pushes `command.clone()` in both accumulation and replay paths
  (lines 660 and 676).

The proposed small change is to finish coverage calculation or variant replay
through `&command`, then move the command into the journal after its last use.
This should eliminate one payload allocation and copy for a retained PutImage
without changing the payload type or upload interfaces. Every validation,
budget, poison, coverage and baseline-reset decision must stay intact.

The profile supports prioritizing copies, but does not isolate this clone's
share. Only seven X11 copy callchains recovered a caller: three reach
`from_put_image`, two reach SHM dispatch and two reach `packed_patch_region`.
One owner copy chain reaches `compose_layer_clipped`. The measured Sophia
binary contains a `memcpy` call inside `from_put_image` at ELF address
`0xdf0732`, confirming that copy survived optimization. No percentage saving
is assigned to the proposed journal move.

### Follow-up: share immutable upload ownership where it is safe

If the small slice is useful, inspect the next ownership boundary. SHM dispatch
already holds an owned normalized upload, but passes a slice through drawing;
`from_put_image` then allocates another retained buffer. Separately,
`SurfaceRasterStore::satisfy` clones the journal when staging an atomic change
to retained density variants (line 744).

An immutable owned payload shared across those retained readers could remove
further copies. The initial client-memory snapshot remains necessary. Shared
bytes must never alias mutable client or canonical drawable storage. Preserve
format conversion, crop/stride checks, byte order, graphics-context semantics,
journal budgets and atomic requirement satisfaction. This is a separate,
broader candidate; do not combine it with the first measurement.

The earlier whole-image crop candidate remains inconclusive and unmerged.
The accepted shared CPU-patch forwarding already avoids queue-to-queue copies;
patch packing and applying rows into the destination are distinct remaining
operations. Zero COW splits here gives no reason to weaken snapshot isolation.

### SIMD is already used in the largest named copy routine

All sampled copies use glibc's `__memmove_avx512_unaligned_erms`. Disassembly
at sampled instruction offsets shows ZMM vector loads and stores
(`vmovdqu64`/`vmovdqa64`). Replacing this with handwritten SIMD is not the
first candidate. There are no hardware bandwidth, cache-miss or IPC counters
in this capture, so these instructions do not prove memory-bandwidth saturation.

llvmpipe is the largest thread group, but its generated routines lack symbols
in this binary. Mesa's `lp_profile` symbol/assembly dump is compiled behind
`PROFILE`; its identifying dump-path and environment strings are absent from
the measured Gallium ELF. A later renderer investigation needs a separately
qualified diagnostic build with JIT symbols before choosing a specific loop.

`util_fill_rect` uses `rep stos` and library fills in the measured binary.
Its 6.9% share is an upper cost bound, not a forecast for vectorization.
The last periodic record attributes 419 full repaints to the coverage decision;
the client covers 77.0% of the output. This is not evidence of an incorrect
damage decision. The measured clear-coverage candidate remains rejected.

## Original validation plan

The October 5 profile proposed this slice under t289:

1. Implement only the owned-command move. Test allocation identity in both
   journal paths, exact variant replay, full opaque baseline resets, partial
   coverage, budget/unsupported refusals and independence from later writes.
2. Run the focused tests and required full gate on the frozen candidate.
3. Compare against this accepted Sophia baseline with the **same Mesa ON in
   both arms**, unchanged client bundle and frame count, and four balanced
   pairs (BC CB CB BC). Keep one attribution profile per arm. Coordinate the
   quiet window; stop on a failed run and keep it.
4. Apply the existing requirement that every pair improves, with no pixel,
   lifecycle or work-count regression. If the change is within noise, retain
   that result and move to the next measured cost. Source simplicity alone
   does not establish a performance gain.

The profile itself changed no implementation. Reviewed production crates
on master `a7e4aedf9` are identical to measured `825d91460`.

Limits: 468 samples have no callchain and 54 have only one frame; the remaining
191 have multiple frames. Per-callsite copy percentages are unavailable.
This is the **SHM/software guest path**, with no DMA-BUF captures. It does not
measure Kitty's hardware path, whole-machine energy or laptop battery life.
XLibre was not rerun, so no new cross-server ratio follows.

Evidence: `~/.local/state/sophia/development-evidence/t289-remaining-hotpath-01/`:
`READY.json`, both guest trees, `ANALYSIS.json`, `SOURCE-RECEIPT.json`, raw
`perf.data`, self/callchain reports and disassembly. `python3 analyze.py`
rechecks identities and recomputes the attribution. `RESULT.txt` SHA256:
`584dbc7b9b05e444e24aeac9d59ff0004d7c40c6f45a6e2bd7a3d9c857998232`.

## Owned journal move accepted (2026-10-06)

Signed production commit `97a9e4ce6` moves the command after its last coverage
or variant-replay borrow in both journal paths. Allocation-identity controls
fail on the old clone and pass with the move; they also check replay pixels,
generations, prior snapshot independence, partial coverage and full-baseline
reset. Focused tests, strict clippy, formatting, layout and the full isolated
`cargo xtask check` pass on the clean candidate. Peer source review accepted it.

The comparison used the recipe above, unchanged `wmbench 47fdb6b` and the same
Mesa mapping-retention candidate ON in both arms. Baseline Sophia `825d91460`
has the same production crates as the candidate's parent, `650e5c62b`.
Only Sophia changed in the two VM settings; companion binaries and the loaded
Mesa hashes match. The host remained logged out at greetd with competing work
paused. All ten fresh guests passed their workload, identity and Mesa
pixel/cleanup checks. Four unprofiled pairs ran in BC, CB, CB, BC order:

| Pair | Baseline CPU | Candidate CPU | Baseline elapsed | Candidate elapsed |
| --- | ---: | ---: | ---: | ---: |
| 1 | 1.44 s | 1.33 s | 39.0 s | 39.3 s |
| 2 | 1.42 s | 1.30 s | 39.2 s | 39.2 s |
| 3 | 1.42 s | 1.33 s | 39.2 s | 39.2 s |
| 4 | 1.42 s | 1.34 s | 39.2 s | 39.2 s |

Median desktop CPU per 300 measured frames fell **1.42 → 1.33 seconds (6.3%)**,
or 4.73 → 4.43 ms per frame. Every pair improved, with separated ranges and
exactly 432 worker compositions in every run. Both elapsed medians are 39.2
seconds; this establishes no throughput or latency improvement.

Whole-session work also matches: 420 CPU updates, comprising one replacement
and 419 patches, 1,325,184,000 payload bytes, three targets and 429 reuses,
421 exact-nearest draws and no captures, imports or COW splits. Every update
was bound and accounted for. Two runs per arm presented all 420 updates; the
other two presented 419 and released one at teardown. That terminal split is
balanced across arms and did not reduce rendering work. There were no pending
worker supersessions, slot deferrals, worker failures/hard stalls, exporter
replacements, topology events or direct-scanout work.

The separate attribution pair measured 1.44 → 1.40 seconds CPU and zero minor
or major faults. CPU-clock profiles contain 695 → 685 samples, none lost.
X11-worker samples fell 182 → 160 and its `memmove` samples 168 → 139.
Total `memmove` samples fell 232 → 219, while owner and renderer copy samples
varied upward. This supports removing an X11 copy, but one sampled pair does
not assign an exact CPU share to the journal callsite.

### Failed measurement attempts retained

`comparison-01` and `comparison-02` remain **FAILED** and contribute no CPU
observations to acceptance. Their guests passed; the added validator made two
overly specific assumptions. The first required 429 target reuses even when
one pending frame was replaced before reaching a worker (431 requests and
428 reuses). The second required 419 presented plus one lifecycle-superseded
update, and refused a baseline that presented all 420.

The successor checks source-backed accounting and records both terminal counts.
It excludes direct scanout, worker deferrals, exporter churn and topology
changes before checking requests plus pending replacements. It conservatively
requires 432 offered frames and at most three terminal lifecycle outcomes for
this recipe; those are qualification bounds, not generic source invariants.
Thirty-nine controls pass. Offline archive checking accepted eleven matching
logs and explicitly excluded one with 433 offered frames. The final fresh
series used these frozen checks, stopped on any failure and allowed no
replacement runs. It met the stricter criterion that every CPU pair improve
with candidate worker compositions at or above baseline.

Evidence: `~/.local/state/sophia/development-evidence/t289-raster-journal-move-01/`:
`RESULT.txt`, `comparison-03/RESULT.json`, `12-attribution.json`,
`07-preflight4.json`, `05-gate.json`, RED/GREEN controls, peer review and both
failed-series dispositions. `attribution3.py` recomputes the sampled attribution.

This result is limited to the SHM/software guest path, with Mesa retention ON.
There is no new XLibre comparison, hardware DMA-BUF, battery or live-session
claim. No install was performed. t289 remains open; the next candidate is the
immutable upload-sharing boundary described above, measured separately.

## Native qualification started (2026-10-06)

The operator asked to move as much measurement as possible onto crunch's real
hardware. Both DP-1 (2560×1440) and DP-2 (1920×1080) are connected on the discrete
GPU; the desktop is logged out at greetd. The earlier software guest percentages
do not establish the cost distribution on this hardware.

One bounded render-node invocation passed all six ignored `snapshot_reuse`
tests in 1.67 seconds on renderD128 (PCI 0000:03:00.0, AMD Navi31). The sandbox
exposed only that render node. This checks snapshot independence, imports,
reuse and resource reclamation on the physical GPU; it measures no Session CPU,
KMS presentation, latency or power. Evidence:
`~/.local/state/sophia/development-evidence/t289-native-render-node-01/`
(`RUN.json`, `test.log`).

A native wmbench compatibility fixture is prepared in
`t289-native-wmbench-01`, using the published `97a9e4ce6` binary and unchanged
wmbench. Its private profile keeps both physical heads active, with the benchmark
on DP-2. It checks the actual client geometry after 120 warmup frames, before the
upstream measurement gate opens for 300 frames. The intended client is 1800×960;
the output rates are 60 Hz and 120 Hz. These differ from the software guest
recipe, so native CPU results will form a new baseline.

The exact profile and Session arguments pass parser-only checks. Native launch
uses the existing TTY wrapper with a real seat, independent input recovery and
a 270-second watchdog; Session is bounded to 240 seconds. Configuration and
runtime state are private to the attempt. A surviving benchmark application
group is cleaned up and fails qualification. Nothing is installed, no service
is stopped, and a failed attempt is retained rather than silently repeated.

The first native Session ran from the operator's tty3 and failed after applying
the output layout, before wmbench began. Selecting DP-2 as primary made the CPU
scene size disagree with the first composition descriptor. The
[startup mismatch investigation](bxeem6rg-changing-the-primary-monitor-mismatched-the-cpu-scene-descriptor.md)
owns that repair and its CPU regression. Console recovery passed, and the failed
run is preserved. It provides no native CPU measurement.

After qualification, profile the same hardware workload and use its largest
costs to select the next source change. Keep real DMA-BUF and software upload
results separate, based on the renderer and transport actually observed. The
remote control process remains outside the local seat, so a fresh native launch
still needs a process started from an active local TTY.

## Owned upload candidate prepared (2026-10-06)

While the native retry awaits a local seated launch, branch
`performance/t289-owned-upload` prepares the next copy reduction on top of the
startup repair. This candidate is **unmeasured and not accepted for promotion**.

Core PutImage decoding and SHM extraction already produce private, normalized
pixels. The drawing path now carries that ownership through the canonical
writer into `from_put_image`, instead of borrowing it and allocating another
copy for replay. Cross-drawable copies likewise already own their extracted
pixels. Public borrowed callers still receive a private snapshot. Client SHM
reads, format conversion, crop packing and canonical drawable writes remain.

Only a Vec whose length and capacity exactly equal the retained prefix moves
into the journal. Borrowed data, trailing bytes or excess reserved capacity
take the bounded copy path, preserving the existing byte charge. An
`Arc<Vec<u8>>` shares immutable journal pixels during atomic density staging;
each command still incurs its full logical charge. Variant backing keeps its
existing copy-on-write behavior. Refusals and baseline resets are unchanged.

The planned 1800×960 native window has a 6,912,000-byte tight upload, larger
than the journal's 4 MiB payload cap. The current path copies that supported
upload before the journal reports `JournalCapacity`. A regression proves the
candidate can move it and retain that same refusal without copying its pixels.
This is source and allocation-identity evidence, not a claim about the native
client's observed transport or CPU savings.

Controls cover allocation identity, immutable command clones, borrowed input
independence, prefix/capacity bounds, staging and failed-demand atomicity, budget
charges and last-owner release. A wire-level SHM control rewrites and detaches
the client segment before requesting a density variant, then changes the
drawable and checks the earlier snapshot in both byte orders. Existing core,
SHM, GC and replay suites remain part of validation.

Evidence: `~/.local/state/sophia/development-evidence/t289-owned-upload-01/`.
Keep this slice separate from the inconclusive whole-image crop change.

Signed candidate `b02b47c45` has read-only peer acceptance and a full isolated
gate at exit 0, with 7,179 printed Rust passes and zero failures. The focused
authority, core wire, SHM and replay suites pass. Three mutants fail their named
assertions: copying the owned payload, deep-copying it during density staging,
and retaining excess Vec capacity. The gate excludes device tests and provides
no native performance result.

The release build passed from the same clean commit. Candidate Sophia is
`539d507768abae4864b4444f2c717fd9a0ea67ce6165ae5b8c7bbb21c6b55a3c`;
the corrected baseline is
`61b8ec422dbed189281a87f19bdf78004be1fcdd365e098b8c0f278263b05e11`.
Both are retained in the Nix store. Factotum and PAM match byte for byte.
`PAIR-IDENTITIES.json` binds the builds to the frozen native fixture;
`candidate-settings.json` changes only Sophia's path, revision and digest.
Candidate profile and argument parsing passed without launching a Session.
The baseline fixture itself is unchanged.

Next, qualify that baseline on the real seat, then compare the candidate with
the same geometry, client, transport and accounting scope. Record native work
counts before choosing the comparison checks; the guest's observed composition
count is not a hardware invariant. Use balanced unprofiled pairs for CPU time
and separate profiles for attribution. Preserve failed attempts, and refuse a
performance claim if the candidate did less work. This candidate remains
unpublished pending those measurements; no install or native run was performed
while preparing it.

To reduce console handoffs, `t289-native-owned-upload-01` now prepares one
command for baseline then candidate compatibility. It copies the reviewed
native launcher unchanged into two frozen arms; settings differ only in the
Sophia path, revision and digest. A failed baseline prevents the candidate
launch, and any failure is retained without replacement. The existing Session
bound, watchdog and cleanup remain responsible for each launch. Six CPU-only
driver controls and both actual profile/argument checks pass. Read-only peer
review accepts the pair driver; it now prints each arm and the recovery key
before launch, then the arm verdict. At that freeze, neither arm had run on the native seat.
Its verdict covers compatibility and cleanup, not native scanout pixel
equivalence or a performance improvement. After qualification, inspect the
native work records before freezing the performance comparison checks.

## Connections

The [framebuffer mapping investigation](djo84ohx-repeated-kms-software-mappings-account-for-the-wmbench-fault-storm.md)
establishes why this profile uses the opt-in Mesa candidate. Its 30.6% software
CPU saving and remaining device/suspend limits still stand. The
[CPU plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md#remaining-hot-path-after-mapping-retention-2026-10-05)
owns this next experiment, and [todo t289](../../../todo.md) remains the queue.

## Native qualification stopped at GLX loading (2026-10-06)

The operator ran the frozen pair. Baseline B applied the two-output policy and
continued composing, so the repaired CPU-scene mismatch did not recur on this
path. The benchmark then failed with `no visual` from `glXChooseVisual`, before
creating a window or reaching its measurement checkpoint. Candidate C stayed
held. Application cleanup and TTY restoration passed. Keep the original failed
run: `t289-native-owned-upload-01/B/smoke-20261006T224240Z`. There is still no
native performance result or complete compatibility pass.

The failure is a benchmark library-loading gap. A renderD128 EGL inventory on
the pinned radeonsi stack included depth 24/stencil 0, contrary to the initial
missing-config hypothesis. A private Sophia XAuthority socket selected visual
34 using the pinned GLX library. The decisive control used the exact wmbench
executable, with an observer that exits before `XCreateWindow` and records its
loaded libraries:

- Original environment: `no visual`, exit 1, no Mesa GLX vendor loaded. The
  loader searched the absent NixOS `/run/opengl-driver/lib` and other Nix paths,
  without reaching the pinned Mesa directory. It did not load host Mesa.
- Same executable and socket, with the three pinned glvnd/Mesa/GBM directories
  in its library search path: visual 34 selected through DRI3/radeonsi, with
  the pinned Mesa GLX vendor and gallium library mapped. Exit 0.

Both selection invocations finished below 0.12 seconds. No Session, KMS,
window, GL context or draw was involved. The executable's RPATH alone did not
supply the vendor library to this dynamic lookup. No Sophia GLX catalog or
rendering change is warranted by these results. The failed setup and observer
attempts remain labeled invalid in `t289-native-glx-visual-01/RESULT.txt`.

Successor fixture `t289-native-owned-upload-02` applies that environment only
to the benchmark child, identically for B and C. Before releasing the warmup
barrier, it requires one renderer process in the benchmark's child tree,
checks its identity across the maps read, and records the exact pinned GLX,
Mesa vendor and gallium paths and hashes. Missing, foreign, extra or deleted
libraries refuse measurement. Original wrapper, TTY recovery, cleanup,
geometry, binaries and bounds remain unchanged. Eleven driver/graphics
controls, five existing native controls and both profile/argument checks pass.
A native retry from the active local TTY is still needed; the owned-upload
candidate remains unpublished and unmeasured.

## Native warmup reached DMA-BUF; placement refused (2026-10-06)

The operator ran `t289-native-owned-upload-02`. Baseline B reached the Radeon
RX 7900 GRE renderer and completed 120 DMA-BUF warmup Presents. The previous
GLX selection failure did not recur. Hagia placed the window at **2440×1320 at
1980,60 on DP-1**, instead of the frozen DP-2 geometry. The client geometry guard
refused before `measure.go`; candidate C never ran. This remains a failed
qualification, with no measured CPU result. Evidence: the fixture's
`RESULT.txt`, `RESULT.SHA256SUMS` and `B/smoke-20261006T230554Z`.

The Session completed and cleaned up normally: wrapper exit 0, no surviving
application group, no pending native cleanup, and TTY restoration without
emergency recovery. The client's `client-error.json` names the geometry error;
the outer verdict instead reported the missing terminal `client.json`. The
loaded-library attestation was after the geometry guard, so it was not reached
in this attempt. Do not infer that receipt from the earlier selection probe.

The source explains the placement: `update_public_work_areas_at` preserves the
WM's active output when it remains live. Making DP-2 the profile primary did
not replace that active DP-1. Hagia follows `snapshot.activeOutput`; monocle
with gaps 59 and the 1px border yields the observed DP-1 size. Record this
focus-at-startup/active-output mismatch separately from performance. A repair
must distinguish initial focus selection from later topology updates that
should preserve focus; this fixture does not change WM production behavior.

The transport also settles the optimization scope: the run records 120
snapshot captures, promotions and evictions, with **zero CPU updates and zero
CPU payload bytes**. The owned-upload candidate is not exercised by this native
workload. Keep its copy-saving measurement on the software upload recipe; do
not use a native B/C difference to claim that saving.

Successor `t289-native-dmabuf-01` is a single corrected published baseline,
`cb2cc176b`, for the real DMA-BUF path. It keeps both heads and their positions,
makes DP-1 primary, and uses Hagia policy `gaps 0` plus left/right struts 379 and
top/bottom struts 239. A CPU-only control loads this exact profile through Hagia
source `155daab6c`, applies its policy candidate, and runs the real monocle
projection: outer **1802×962 at 2299,239**, predicting content **1800×960 at
2300,240** after Sophia's 1px inset. The actual native geometry must still match.
This agrees with wmbench's centered 1920×1080 stage and 60px margins on DP-1.

The successor checks that exact content rectangle, DP-1's output/head identity,
and the primary RandR mapping. It checks post-configuration RandR rates of
120 Hz on DP-1 and 60 Hz on DP-2; bootstrap `native_head` rates are not proof of
the applied configuration. The workload targets 60 fps on the 120 Hz head.
This is a new native recipe, not comparable directly with the QEMU workload.
The graphics map receipt is taken before the geometry guard, and the outer
verdict now preserves a client failure's original message.

Twelve CPU fixture controls, the Hagia projection control and actual
profile/argument checks pass. Peer review accepts the placement/launcher
changes; wrapper, bounds and group/TTY recovery are byte-identical to the prior
fixture. No native launch was performed while preparing the successor. The
remote tool has no active local TTY; the prepared launch remains an operator
console command. A completed native baseline and profile are still needed to
choose the next DMA-BUF optimization. The owned-upload candidate remains held.

## First native DMA-BUF baseline passed (2026-10-06)

`t289-native-dmabuf-01/smoke-20261006T232842Z` passes the frozen native
qualification on baseline `cb2cc176b`. The actual window is **1800×960 at
2300,240 on DP-1**. RandR reports DP-1 primary at 120 Hz and DP-2 active at
60 Hz before and after the workload. The client reports RX 7900 GRE/radeonsi,
and its pinned GLX, GLdispatch, Mesa vendor and gallium mappings are attested.

The unprofiled report gives:

| Interval | Desktop CPU | Elapsed | Approximate share of one core |
| --- | ---: | ---: | ---: |
| Idle | 0.02 s | 10.0 s | 0.2% |
| 300 render frames | 0.28 s | 5.0 s | 5.6% |

The client reports 60.20 fps. CPU accounting sums user and kernel time for the
Sophia owner, its protected Sophia helper and Hagia, with unchanged process
identities. It excludes benchmark client CPU and GPU execution. The render
figure is approximately 0.93 CPU ms per client frame, not presentation latency.
Accounting is at 100 Hz; the idle figure is only two aggregate ticks. The
frozen fixture retains full Session records in a private raw log; keep this
logging mode fixed during attribution. This is one native observation, with rounded elapsed time, not a repeatability claim,
an optimization acceptance or a native comparison with XLibre.

Whole-Session work records include setup, 120 warmup frames, the 300 measured
frames and teardown. They show 420 snapshot captures, promotions and evictions;
zero CPU uploads or payload bytes; six composition targets and 442 target
reuses; six import-cache imports and 416 hits. All 448 worker requests complete,
with no worker failures, soft/hard stalls, generation/recovery replacements,
frame-slot deferrals or direct scanout. Present records contain 419 copy
completions, one skip and 420 idle/fence events. Keep that terminal split; do not
normalize it into 420 presented frames. These counters are not timed-interval
deltas and do not prove scanout pixel equivalence.

Both Session and the benchmark exit cleanly, with no orphan group members or
pending native cleanup. TTY modes and termios are restored without emergency
recovery. No process from the run remains; END was sent to the peer. Evidence
is bound in `RESULT.json`, `RESULT.txt` and `RESULT.SHA256SUMS`. The benchmark's
power field remains a sensor observation; its scope has not been validated for
whole-machine or battery claims.

The next fixture, `t289-native-dmabuf-profile-01`, holds binaries, geometry,
head rates, client and graphics libraries fixed. It runs an unprofiled control
then a profile, each with 1,800 measured frames (about 30 seconds), to collect
enough CPU samples. Both arms take process/thread counter bookends; only the
profile arm samples. Host `perf_event_paranoid=2` remains unchanged: the event
is `cpu-clock:u`, so samples cover user space, while the counter deltas retain
kernel CPU time separately. The sampling period is 2 ms of CPU time, with DWARF
stacks. The pinned Sophia binary retains its symbol table and unwind sections.

The profiler is attached disabled to the exact desktop PIDs after warmup,
acknowledges readiness, enables before the render gate, and disables after
`MEASURE-END`. It stays in the Session application's process group, with nested
cleanup if profiling fails. Each Session keeps its 240-second bound and
270-second wrapper watchdog; benchmark deadline is shortened to 150 seconds.
The driver stops at the first failure and never replaces a run. U and P must
differ only in the profiling flag; this is no source-change comparison.

Eighteen native/graphics/attribution controls and seven driver controls pass;
both actual profile and Session argument checks pass. A short CPU-only
functional control attached user-only perf to its creating process, collected
samples and stopped through the same acknowledgement channel. Peer read-only
review accepts the attribution ordering and cleanup. At fixture freeze no
native profile had run; `READY.txt` owns that launch. The completed result
follows. The owned-upload candidate stays separate because the native workload
performs no CPU uploads.

## Native DMA-BUF attribution (2026-10-06)

Both arms of `t289-native-dmabuf-profile-01` pass on `cb2cc176b`:
`U/smoke-20261006T234618Z` and `P/smoke-20261006T234711Z`. Each renders 1,800
measured frames at **60.03 fps**, with the same geometry, two heads, client,
libraries and Sophia binary. This is baseline attribution; no candidate code
differs between the arms.

| CPU time over approximately 30 seconds | Unprofiled U | Profiled P |
| --- | ---: | ---: |
| Desktop total | 1.49 s | 1.71 s |
| User space | 1.02 s | 1.13 s |
| Kernel | 0.47 s | 0.58 s |
| Owner thread | 0.86 s | 0.96 s |
| Two renderer workers | 0.37 s | 0.39 s |
| Mesa submission thread | 0.09 s | 0.09 s |

U consumes approximately **5% of one core**, or 0.83 CPU ms per client frame.
The owner accounts for 58% of desktop CPU; renderer workers account for 25%.
The 100 Hz process counters include exited threads, while thread bookends can
only attribute surviving threads: their sums are 1.39 s in U and 1.51 s in P.
Do not assign the missing difference to a particular worker. The helper and
Hagia accrue no whole CPU tick in either measured interval.

P records 2,380 physical pointer events over its whole Session; U records zero.
The explicit first-motion and output-transition records precede the measurement
gate, but later motion within an output is not individually logged. Its ending
before measurement is therefore unproved. **The 0.22-second difference does
not isolate profiler overhead.** Both runs remain valid compatibility and
capture observations; neither is an optimization acceptance.

Both whole-Session reports have 1,920 snapshot captures, promotions and
evictions, six targets, six import-cache imports and 1,916 import hits. CPU
updates and payload bytes remain zero. U completes 1,949 worker compositions;
P completes 1,948. No worker failure, stall, replacement, slot deferral or
direct scanout occurs. The terminal Present split is 1,919 copies plus one
skip, with 1,920 idle/fence events. These totals include setup, warmup and
teardown. Both native cleanup and app-group cleanup finish empty; TTY recovery
passes and END was sent to the peer.

### What the samples establish

The user-space recording contains **521 samples, zero lost**, all within the
Sophia process in the explicit desktop scope. The owner has 326 samples (63%);
the two renderer workers have 142 (27%). Every sampled instruction pointer
resolves, but **298 caller stacks do not unwind**. Preserve their leaf symbols
without inferring callers. Inclusive stack counts overlap.

Owner samples include allocator and collection work, 17 `rustix::ioctl`
leaves, three `drm_ffi::mode::get_property` leaves and a malloc caller chain
through `PropertyValueSet::as_hashmap`. The ioctl leaves do not identify their
requests. Kernel CPU, 32% of U's total, is outside this recording. All-thread
`memmove` has 19 samples (3.6%); this does not justify custom SIMD. Renderer
samples measure CPU submission to Mesa, not GPU execution time.

Keep two limits with later comparisons: this fixture retains full Session
records, so formatting cost need not match ordinary-login diagnostics; and
perf has no recorded clock identity, so its timestamps must not be aligned
directly with Python's monotonic bookends. The enable/disable acknowledgements
and benchmark gates bound the recording. Future timestamp correlation needs
an explicit supported clock choice. No native XLibre or power conclusion
follows from this pair.

### Next candidate: retain primary-plane property handles

The source at the measured revision confirms avoidable repeated metadata work:

1. `persistent_native_scanout/singleton_tick.rs:62` supplies the real group's
   `session.card()` to submission.
2. `drm/native_scanout/prepare.rs:266` discovers connector, CRTC and plane
   property handles after every successful buffer export, including page flips.
3. `native_atomic/properties/lookup.rs:39,48,57` uses
   `get_properties().as_hashmap()`. The DRM crate queries every property's
   metadata and allocates names and hash entries. Conversion then makes another
   vector before discovery selects the fixed handle bundle.

The next slice should retain only a **successfully discovered handle bundle
for the current device and head selection**. Cursor handles already use a
per-head cache initialized empty at owner construction. Audit selection changes
without reconstruction, including mirror and preview paths, before choosing
the key and invalidation points. Identical numeric IDs on different cards must
never share a bundle. Keep storage bounded by the heads; do not cache changing
property values, alter atomic requests or change framebuffer/fence ownership.
Failed discovery must remain retryable.

Required controls are cached/uncached atomic-request equivalence, discovery
only once for unchanged selections, rediscovery on owner/device/selection
change, failure followed by success, and optional-property behavior. Cover VRR,
out-fence, IN_FORMATS and cursor-bearing requests. Then run the isolated gate,
native correctness smoke and balanced unprofiled trials with matched geometry,
work and input activity. The profile motivates this candidate but does not
establish its saving. Temporary per-frame collections remain a later target;
xshmfence loader churn and SIMD stay lower priority.

`RESULT.json`, `RESULT.txt` and `RESULT.SHA256SUMS` bind the raw runs, offline
analysis and exact source hashes. The source audit matches the measured
revision byte for byte. `SAMPLE-SUMMARY.json` distinguishes sampled leaves
from available callers. No new device run, production edit or install was
performed during this analysis. t289 remains open.

### Property-handle candidate ready for native measurement (2026-10-06)

Signed candidate `82ec37c0b` retains one successful handle bundle per physical
head, keyed by native owner, device group, connector, CRTC and plane. Ordinary
singleton and mirror preparation use it; startup and topology preparation keep
raw discovery. The common topology installation method clears it for both the
candidate and rollback, including unchanged object IDs. Device operations,
atomic values, cursor updates, PRIME imports and out-fence handling are forwarded
unchanged. One record at owner release reports cache hits, discoveries, failures
and invalidations; these are adapter counts, not measured kernel ioctl counts.

The clean isolated gate exits zero: 7,181 reported test passes, zero failures,
100 ignored, with clippy and layout passing. Nine focused cache/installation
controls pass; six mutants fail their named assertions. The installation control
calls the shared production head method, not a complete hardware transaction.
Evidence is `t289-kms-property-cache-01`: `SOURCE.json`, `MUTANTS.json`,
`09-gate.json`, `GATE-SUMMARY.json` and `10-nix-build.json`. Earlier compile
failures, the empty-filter test run and the corrected layout refusal are kept.

The Nix binary is `/nix/store/qdah1mk0dsy3dphmrainsq65jcjs5945-sophia-0.1.0/bin/sophia`,
SHA-256 `7ac7a1d0ff1faccbd64264d2e43e45dfe0e884afb3f5dc247aa5c22d5537ff73`.
Its baseline remains the measured `cb2cc176b` binary; baseline production source,
Cargo files, flake and toolchain equal candidate base `fdb5976d9`.

Frozen fixture `t289-native-kms-property-cache-01` keeps the reviewed native
launcher, attribution, graphics checks, profile and wrapper byte-identical to
`native-dmabuf-profile-01/U`. Ten CPU controls and both arms' argument/profile
checks pass. Native source/fixture review and the quiet-window launch remain
pending. Run one candidate smoke, then four unprofiled pairs in order
**BC CB CB BC**, with 1,800 measured frames each. The smoke is excluded from the
comparison. Keep the 240-second Session and 270-second wrapper bounds, and reserve
360 seconds before each launch within the 25-minute series budget. Stop on the
first failure or refused workload; keep it with no replacement.

Acceptance requires every pair to lower desktop CPU, a median paired reduction
of at least 5%, and candidate compositions at least baseline in every pair.
Reject input activity and changed transport, capture, completion or cleanup
work; retain the actual terminal counts. These are whole-Session checks beside
gate-bounded CPU accounting. No native run, saving, publication or install is
claimed for this candidate yet. The owned-upload candidate stays separate.

### First cache smoke passed; comparison parser refused (2026-10-06)

Claude accepted the source and fixture before the native window. Fixture 01's
candidate smoke then passed native compatibility: 1,800 measured frames, clean
TTY recovery and no surviving application processes. Desktop CPU was 1.49
seconds; this is a smoke observation, not a matched saving. The backend emitted
`owner=1 heads=2 hits=1941 discoveries=2 failures=0 invalidations=0` at release.

The comparison driver nevertheless stopped with `candidate cache evidence
missing`: its parser expected the record at column zero, while backend tracing
added a timestamp, colour codes and module prefix. Keep
`qualification-20261007T002912.390995Z/RESULT.json` **FAILED**, with no comparison
trials. The native cache is observed; no performance acceptance follows.

Successor `t289-native-kms-property-cache-02` changes the parser and its controls
only. For the cache record it removes ANSI SGR and the exact known INFO logger
prefix, then applies the existing schema, owner and counter checks. The real
log, plain and colour-free forms agree; wrong logger, quoted record, WARN,
duplicates, bad schema and invalid counters refuse. Twelve controls pass; the
old parser fails the new real-log regression. Both argument/profile checks pass.
`SMOKE01-OFFLINE.json` records the successor interpretation separately, and
`SMOKE01.SHA256SUMS` binds the source evidence. Source, binary, launcher, settings,
workload, bounds and acceptance rules are unchanged. Review and a new quiet ACK
precede the fresh comparison; the first result is never replaced.
