---
id: i9ljdlt4
date: 2026-10-30
kind: investigation
status: investigating
tags: [investigation, performance, rendering, cpu]
---
# How can Lom render Vello CPU directly into an owned-ring slot across its worker boundary?

## Decision and corrected starting point

CPU direct-fill adoption precedes further GPU implementation on the
[critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md).
This is a source-grounded design, not an implemented API or performance result.
Existing holds, W2, releases and sealed benchmark evidence stay untouched.

Mason's clarification is two native pixel movers: CPU shared-memory pixels and
GPU buffers. The application chooses, with user preference where supported;
Sophia still enforces permissions and negotiated capabilities. Direct fill is
the intended default **within the selected pure-CPU path**, after qualification.
It does not force CPU use or create silent fallback. Slice upload remains useful
for already-owned pixels, not a third renderer policy. Internal CPU/GPU hybrid
rendering also fits these two output pipelines; see the
[application guide](../../native-rendering-choice.md).

At signed Lom `32e0bab31594f70c3d945547d7e81e2806b6fae8`, native `--serve`
requires `GpuGrant::from_environment` (`src/cli/serve.rs:18–24`). Its `lom-gpu`
worker calls `dma_content` or `readback_content` (`src/render/worker.rs`).
`RenderImage::Cpu` currently means **GPU-readback bytes**, not Vello CPU
rasterization. `src/protocol/pixels.rs` says this explicitly. Vello CPU is used
by `tests/ui.rs`, but not by this production worker. Earlier shorthand that
production already rendered Sparse Strips into an owned CPU buffer was wrong.
Adoption needs both an explicit CPU renderer and worker-safe slot custody.

## October 30 SDK source gate

The Rust custody layer is implemented locally on `cpu/owned-fill` in
`~/dev/sdk-rs-cpu-owned-fill`: signed implementation
`5d89565266c3a066c32c2eebac6fad40dc1d0a66`, parent exactly
`0706ea2c78cd16ba59ed209620354f30a2c78906`; signed documentation head
`4d81630c5125c88cb74c1b45c1a0fe890a12a64a`. The tree is clean and unpushed.
Public operations are `image_acquire_ring_fill`, `image_commit_ring_fill` and
`image_abort_ring_fill`, with an owned `OwnedRingSlotFill`. Failed commits retain
the loan, successful operations leave it inert, and abandonment is terminal.
One lazily allocated producer identity is reused across fills; consumed-payload
checks prevent an old handle claiming a later fill. No per-frame token allocation,
new thread, wire record or unsafe block was added.

All 948 workspace tests pass (174 result lines, including doc tests). Fifteen new
cases cover producer custody and scripted shell-client integration, including
actual mapping identity, wire-visible payload, owner progress, foreign/stale
handles, exact release, failures and late commit after disconnect. Four guard
mutations are killed and restored; the restored producer/client tests pass.
Workspace/all-target clippy has no warnings. Changed Rust files pass formatting;
whole-workspace fmt still refuses one unchanged baseline hunk in
`sophia-image-producer/src/role.rs`, hash-checked against the base. No DMA edit was
included to conceal that exception.

Receipts: `~/.local/state/cpu-owned-fill-sdk-01/`, 25 manifest entries verified;
`RECEIPT.json` SHA256 `bb8d771d7b3f886b4a0f7ce718655e53c2c01d84e838d595411eb8b7d2423e5c`,
manifest SHA256 `30ff435ff65698052f1499c09a8c665728a2afbf7805b98c13b9fef3a6b7d73c`.
This is device-free Rust source evidence, not Lom integration or native acceptance.

**C behavioral parity is proved under documented caller ownership.** Signed
`9adb3c5b83a52baf2a8728cb10fcf821d1d6729b`, tree `cd509040`, based on
`3ccc7d04c28309ebed6fd53f399af7c7a5682d80`, needs no new C operation or ABI.
The existing pixels/offer split supports a worker while owner dispatch and
disconnect leave its mapping intact. C callers enforce one fill, exclusion of
other pixel/offer calls, exact geometry and identity, synchronized completion,
and no result surviving close/init. Image close must wait for the worker to stop.
Rust enforces its loan rules; C does not automatically detect misuse or abandonment.

All 74 C test binaries and the Nim binding smoke test pass. Five new scenarios
use a real pthread parked mid-fill and independently transferred ring FD to prove
owner progress, abort/refusal without ID spending, backpressure/retry, exact
release reuse, and mapping lifetime across session/channel failure. Three
negative controls are killed and restored; M1's SIGSEGV site is inferred, without
a stack trace. The final checks bind tree and diff before/after. Receipts:
`~/.local/state/cpu-owned-fill-c-01/`, 33 manifest entries;
`RECEIPT.json` SHA256 `ac856c095c80861bfe941970db5c406166457228372e982c9b96233d9148a323`,
manifest SHA256 `5554687862534df6be7cabe11d32bd7f1fc45dcb2fc35286bac89d9602a67b7c`.
Rust's documentation follow-up is signed `99b05e345be34678128733bf0166ded85b06239a`;
its implementation and original receipts are unchanged.

## October 30 Lom consumer source gate

Signed local Lom `ceb89664ab07abae57bf6c56fe772820d37a8786`, tree
`674f10ca9ad90d9ab4afa69fb8dba99347aa1927`, parent `32e0bab3`, adds explicit
`--serve --renderer cpu`; GPU remains the default. CPU requires owned-ring and
does not read the GPU grant. Its existing worker renders the retained scene into
an owned slot loan and applies the same pixel conversion as the copy reference.
The owner retains its acquisition snapshot through completion and commit,
rechecks it at both stages, and commits that exact metadata. A stale completion
aborts before staging and re-dirties the panel. Errors after staging are terminal:
abort an uncommitted loan, then drop/restart the service. A successful Offer is
never rolled back by a later timeout. Accepted alone does not permit reuse.

The final device-free gate passes 18 Python tests and 106 Rust tests (one existing
ignored subprocess entry), source layout, formatting, dependency audit and
warning-free clippy. Seven snapshot/abort mutations were killed and restored.
Byte equivalence combines an actual owned-loan worker comparison against the
same CPU renderer's copy reference with the scripted peer's slot-byte check;
it does not establish CPU/GPU pixel equality. A real Accepted-only hold proves
that no second job or Offer starts before exact SlotReleased. SDK hard-commit
Refused/Closed/Argument is not induced end-to-end; only the components are covered.

Receipts: `~/.local/state/lom-cpu-direct-fill-01/`. Candidate and committed diff
SHA256 `22abd46edcf093a957818ff434dce6bd0fc38a92d6edefdbb957c13953fbe0b0`;
`RECEIPT.json` SHA256 `da0399053fd56b4707d7798ef77db7dc8df6e4cce7521bd8853376201be2d5d6`;
manifest SHA256 `a9009ef09bfa4aa5579a27cd5a53ae24df2a292ca3a9f49e3cd5ca5d1708e6f2`.
The SDK `file://` pin is a recorded local development exception. There is no
verified Sophia SDK pin update, release, native qualification, default adoption
or performance claim. C consumer enforcement remains caller-specific. Next is
native CPU qualification against the same-renderer copy reference; GPU direct
rendering stays later.

## Native launch prerequisite and qualification order

Sophia's `base_launch_spec` at `d9116ff0` starts the configured executable with
exactly `--serve`; it canonicalizes and read-only binds `SOPHIA_SHELL_CONFIG`,
without parsing Lom's panel keys. Consequently the CLI-only CPU selector in
`ceb89664` was not selectable through an ordinary native component recipe.
Signed local Lom `9b1cf041203b03b69b675d2d771f80b15ab9cae2` fixes that prerequisite:
`panel { renderer "cpu"; }` selects CPU, absence still selects GPU, and explicit
`--renderer` overrides the config. CPU plus the resolved DMA source refuses before
connection or renderer startup. Sophia, the SDK and the renderer/custody path are
unchanged. No environment override or wrapper executable is introduced.

The default full gate passes 110 Rust tests (one existing ignored subprocess
entry), 18 Python tests, formatting, dependency audit and warning-free clippy.
Five controls catch ignored config, ignored CLI override, omitted CPU/DMA refusal,
ignored CPU parsing and ignored CPU startup routing. Receipts are in
`~/.local/state/lom-cpu-native-config-01/`: signed receipt SHA256
`a8aca6a45d8d1e65a541f349bcaa08ada69cbc183a83c96244e047c497c25277` and manifest
`82db193f9117512a21e4c70f6107c7a50ea6d7ef8fd9beb5f7e9b6ebe7dc2199`.
The isolated default-feature release is
`~/.cache/cargo-slots/lom@lom-cpu-native-config/1/release/lom`, SHA256
`123746c6fd52670bdd8ded6dcd3237ca867424b8d6a0fbd0305eee2cf3b1b13b`.
`RELEASE-READY.json` (`3640db6fcae9ffd6f1eaaca0693d97e505229192944846fe1ce937da8031e255`)
records permission normalization to 0700 without changing the binary bytes.
The local SDK pin remains a development exception.

Qualification proceeds in this order:

1. An ordinary native CPU clock run, using the sealed `shell-client-work-02`
   recipe as a read-only reference, in a fresh package. Preserve its one-output
   topology, fixed-time collection, identity binding and recovery checks. Deny
   GPU access to Lom and require its exact CPU renderer record; keep any retained
   dock's GPU admission separate. This is not a wholly GPU-free desktop. Check
   matching client/host presentation identities, ongoing rendered clock updates,
   resource settlement and cleanup. There is no output pixel readback or CPU
   improvement verdict in this gate.
2. After ordinary startup works, add a separate default-off diagnostic reference
   entry point. Use one binary for both arms, selected by an explicit protected
   config value; the ordinary entry point must refuse the diagnostic reference.
   Reuse Vello CPU, retained scene construction, conversion and the existing slice
   upload. Do not use the GPU-readback path as the baseline, a hidden environment
   switch, a new host-side workload driver or a continuous-redraw hook.
3. Qualify that reference and predeclare its comparison before running it. The
   seconds clock is a real approximately 1 Hz workload, not a 120 Hz producer.
   Existing fixed-time snapshots only bracket presentation counts; report CPU
   per time and the count bracket, never divide by an assumed frame count.
   Same-scene byte proof, native pixel evidence and elapsed render-to-ready timing
   remain distinct gates. Keep inconclusive outcomes and do not repeat for a win.

No native result, default adoption or performance claim follows from the launch
prerequisite or release build.

### Sealed ordinary CPU clock package

`~/.local/state/t331/lom-cpu-native-01` is sealed for one attended `clock-cpu`
attempt. It retains host `560b0aa` / binary `25f2ed76`, the GPU dock and the
60 Hz topology from `shell-client-work-02`; Lom uses release `123746c6` with
`renderer "cpu"` and a denied GPU grant. The run lasts 100 seconds, with a
10-second warmup and a 60-second fixed accounting window. Seconds-clock updates
are required; pointer/keyboard interaction is excluded. CPU accounting is
descriptive, with no comparison or per-frame denominator.

The package requires exact CPU renderer/transport records, distinct bar/dock
negotiations, and a bijection between Lom and host presentation records.
Candidate generations and presentation epochs are separate domains: epochs may
skip values. This shared untrusted-log join proves consistency, not pixels.
Existing identity, resource-settlement, output, VT/PAM and cleanup checks remain.
Exclusive attempt/result markers prevent retry after an attempt is spent.

Offline validation passes 43 tests; 28 tested inputs stayed unchanged. Root's
validation receipt binds all seven checks and 43 static input hashes, including
test logs and preparation code. Its SHA256 is
`2ce4f43da77ce1742d92798ead72cfdfc5357fd010890b54bad3b433bf257f35`.
The 46-entry manifest SHA256 is
`560eed909da2b042df99902966aec23cf8d5ea888c3973d9f666a59c3c208f75`.
Post-seal check-only passed without launching a session or opening devices.
The single attempt is now spent. Do not relaunch this package. Its failed
postflight and the separate saved-data qualification are recorded below.

### Saved-data qualification after a verifier correction

The native collection completed with clean shutdown, but the package postflight
failed on an incorrect dock-field requirement. Provlita `01035e62` emits
`connection_epoch` in `provlita_candidate`, not `content_grant_epoch`. The test
fixture had invented that field. This was a package verification defect, not a
Provlita failure; the review did not catch it before the attempt.

`~/.local/state/lom-cpu-native-audit-01` contains the separate correction. It
requires Provlita's actual seven-field schema and joins its output, candidate,
presentation epoch and connection to the host's presented record, where grant 2
is required. It refuses missing/duplicate joins, mismatches and foreign
connections. Other native gates are unchanged; outputs go only to the audit
directory. The original package remains failed and immutable, with all 74 files
and their recorded metadata unchanged. All 248 package pins still match.

The original verifier fails on the saved log and the repaired verifier passes.
Sixteen adapted tests and four saved-data tests pass, including nine dock and
seven host refusal controls. The full saved-data audit exits zero: 100 rendered
Lom candidates join to 100 host presentations, with the GPU grant denied; one
dock candidate joins through host grant 2. All 71 process-binding checks are
stable, native worker requests/completions are 109/109, and output, resource,
VT/PAM, recovery and cleanup gates pass. The accounting window is 60.004633 s;
one-second polling brackets its rendered count at 60–62. There is no exact
per-frame denominator or performance comparison.

The audit receipt SHA256 is
`c7ed9eb0ace2ede96f4201c23888025fe783824b1e69fa4cf870a38eca5367a5`;
its 21-entry manifest SHA256 is
`e46e91b6cff25e81c8bb20ca015e3ee66cef2985a755a220fc6580b81a76854e`.
The corrected reducer is `be4ebef6`; the reviewed diff is `dc47f147`.
This qualifies the ordinary native CPU seconds-clock path on the pinned local
artifacts. No native rerun was needed. Pixel readback, the same-renderer copy
reference, default adoption and performance remain separate gates; the reference
is next, with GPU direct rendering still later.

### Same-renderer diagnostic reference source gate

Signed Lom `8358cb3fa5ae121fd04fa89084b8a8f5b776528c`, tree
`4f8771e21bd14ff046d3687b0bfe4348bd8d9285`, adds the default-off
`diagnostic-cpu-reference` feature and a separate `lom-cpu-reference` binary.
It accepts only the host's fixed `--serve`. The protected panel selects
`renderer "cpu"` (direct fill) or `renderer "cpu-copy-reference"` (owned image
plus the existing slice upload). Both arms use the same binary, retained scene
worker, Vello CPU renderer and pixel conversion. No service or scheduler source
changes were needed. Ordinary Lom's CLI, startup record and default stay the same.

The diagnostic entry refuses GPU; ordinary serve refuses the reference mode;
both CPU selections refuse DMA. These checks run after config/argument resolution
and before connecting. The ordinary feature-on binary can still run `check-config`
for either panel; it is a diagnostic config checker, not a default-feature build.
Both diagnostic startup records include `diagnostic=cpu-reference`.

The final default and feature gates passed 113 and 120 Rust tests respectively,
with zero failures and two existing subprocess entries ignored across the builds.
Eighteen tooling tests, formatting, dependency audit and both Clippy passes also
passed without warnings. The tests compare renderer output, real worker output,
and peer-read SDK ring-slot bytes, and prove prohibited entries never connect.
This is device-free equality evidence, not a native displayed-pixel claim.
Candidate 1's feature-off test warnings and candidate 2's superseded passing gate
are retained; candidate 3 uses a fresh exclusive socket-test directory.

Evidence is `~/.local/state/lom-cpu-reference-01/`: receipt SHA256
`388a8518356a4af8e7090ef51a671a78070d799560d92e60f7a28da311749538`,
manifest SHA256
`4dd8581fadeec4720bc2a0a1b72944d64dca9003fb616a51468168c2e38610cf`.
The signed branch is published as `sophia-org/lom:cpu/copy-reference`.
Rust SDK `99b05e34` and C parity `9adb3c5b` are published on `cpu/owned-fill`
and `cpu/direct-fill-caller` in their respective official SDK repositories.
This new Lom branch uses the official HTTPS Rust SDK pin at the same revision;
the earlier native build retains its recorded local-file provenance unchanged.

The separate release build uses `cargo-slot lom cargo build --locked --release
--features diagnostic-cpu-reference --bin lom --bin lom-cpu-reference`, with
two jobs. Source stayed signed and clean before/after; all three Lom fingerprints
list exactly that feature. The runtime is
`~/.cache/cargo-slots/lom@lom-cpu-copy-reference/1/release/lom-cpu-reference`,
SHA256 `2facb26d59fcb26308e8dbaccb26355add490611bdb142d61c175839f6d9f5a8`,
device/inode `2067/64518768`. The adjacent feature-on `lom` checker is SHA256
`134291da81f9495da36f123fa1885d6ff9bc70e57b0bf034c8e6fb35b5be121a`,
device/inode `2067/64518767`. Both are consumed in place at mode 0700, uid 1000;
neither binary was copied. Offline checker runs accepted both exact panel
spellings, while the frozen default binary refused the copy-reference spelling.
These checks opened no live connection. `RELEASE-READY.json` SHA256 is
`8e84b9a5e0be30798167d04402cbe505fb6d6f6fe0e7ad27a1651f3c6fb3fca6`;
`RELEASE-SHA256SUMS` is
`5cf51e02aba5ae99bd2755c8f15f7560165cc7e4c43d5bb963d8649e7345015f`.
Keep this source worktree and its Cargo pool frozen through package consumption.

The following two-arm native qualification used: direct then copy, each independently
checking startup, denied GPU admission, custody, rendered-clock progress and
cleanup. Keep the seconds-clock workload and fixed-time descriptive CPU window.
Do not spend an ABBA on this gate or derive differences, ratios, per-frame cost,
latency or a benefit claim from its two unpaired wall-clock scenes. Performance
requires a later separately declared comparison; GPU direct rendering stays later.

### Sealed diagnostic native qualification

`~/.local/state/t331/lom-cpu-reference-native-01` is sealed and now complete.
Its single attempt is spent; do not relaunch it.
Order is `clock-cpu-direct`, then `clock-cpu-copy`, each with the inherited
100-second seconds-clock recipe and a fixed 60-second CPU accounting window.
Both use runtime `2facb26d`; checker `134291da` is used only offline. The host,
GPU dock, output profile and other inherited artifacts are unchanged. The panel
GPU grant is denied. No ABBA or performance comparison is authorized by this
package, and different wall-clock scenes forbid cross-arm checksum equality.

Forty-six package tests pass, including an in-memory adaptation of the real
failed-01 log to each exact diagnostic startup line. All 74 original files and
their metadata remain unchanged. The correct seven-field Provlita schema and
host-side grant join are preserved. Root also exercised the new binding block
on real saved identities and eight refusal mutations; that is a schema check,
not native evidence for the diagnostic binary.

Root validation covers all 61 static inputs and nine successful check logs;
44 tested inputs were unchanged, and all 268 preliminary pins matched. The seal
covers 65 files. Post-seal check-only passed both SSH/idle-VT and argument/config
preflights, with `devices_opened=false`; no run directory, attempt or result
marker existed at that point. Manifest SHA256:
`4ad78dd3d72e42d24514250cb54b16f9f98eb7e719193b47ac8bac17b47a29b3`.
Root validation SHA256:
`e5d88f651932ebace3950877cf7880394b07cec39e218e9632cb68c15333d75d`.
Root receipt in `~/.local/state/lom-cpu-reference-native-root-01/` is
`ac41f1e37ef626834f95f7f4c494d85e2e2e755023e87b220b29540a412c736c`;
its manifest is
`cce04401450762b550fe8a36fdb5779f0b2449b4ee06ab84c5b2e7a9b6f2bdc3`.

### Completed native reference qualification

Mason ran the package once. Both arms completed and passed their sealed
postflights: each produced 100 rendered Lom presentations and one dock
presentation, with 71 stable process-binding checks. The exact diagnostic
startup line selects each arm; both used the same pinned runtime. The bar had
GPU access denied. Client/host joins, resource settlement, protocol, output,
recovery and PAM/TTY cleanup passed with no residual processes.

Root's separate saved-data audit in
`~/.local/state/lom-cpu-reference-native-audit-01/` rechecked all 65 sealed inputs
and 268 pins, raw native/recovery records, every saved process binding and CPU
sample identity, and environment evidence. Reanalysis of both saved CPU windows
reproduced their JSON results exactly. Every package file and its recorded
metadata remained unchanged (124 files). `AUDIT.json` and the external receipt
bind the verifier, sampler and complete input inventory; no native rerun or
build ran. Claude's independent read-only audit also passed. Receipt SHA256:
`9b1623d974079f1aa80e6a8dca8cf6cee5a260c9e8f18677df338e6af8cd995c`;
its nine-entry manifest SHA256:
`1b6e811623b28b63e201ff7248e5d449a13be41fcd74731486fb1b2b60e28e7c`.

The windows were 60.002495 s (direct) and 60.006523 s (copy), with zero late
sampler slots. Total descriptive CPU intervals were respectively [9.67, 11.67]
and [9.83, 11.83] ms/s; Lom's were [0.50, 1.17] and [0.67, 1.33] ms/s. These
independent clock scenes support no subtraction, ratio, per-frame, latency or
benefit inference. Both logs contain `hagia: WM SDK dispatch failed: -1
remote=116` after frontend join during shutdown, followed by quiescent components
and clean cleanup; the audit does not claim error-free application text.

This closes the diagnostic native startup/custody/clock-progress/cleanup gate.
Displayed-pixel correctness, default adoption and a predeclared same-scene
performance comparison remain open. Preserve the package and release artifacts;
GPU direct-render work still follows the CPU adoption work.

### Static opaque displayed-strip gate: native pass

Signed niltempus `f7510a96361f41d18b95d8318269230954dd54da` on
`bench/lom-cpu-pixels` adds the untimed controller and comparator. It reuses
the private capture delivery verifier, with no Lom or Sophia source change.
The gate passes 5 pixel tests, 10 controller tests and 14 existing capture
tests; four source mutations are caught and restored. The immutable source
receipt is `~/.local/state/lom-cpu-pixel-controller-01/RECEIPT.json`, SHA256
`0d3d7ff0fd248b0e8bc4ba6d4378a1d6cc7c0f79b0fbb244113af7d78bf4cf5d`.

A separate diagnostic host builds exact signed Sophia `a56ace891`, whose
lineage is `a56ace891` → `d9116ff0` → `560b0aae`, with
`native-session,experimental-shell-capture,diagnostic-capture-delivery`.
The release stays in the `sophia@sophia-lom-cpu-pixel-host/1` slot, SHA256
`e43936c30cd8e67156e708f74851dd3c71df82d7bf05dcf00953666fdf18f0db`.
Its release-ready receipt is `~/.local/state/lom-cpu-pixel-host-01/RELEASE-READY.json`,
SHA256 `9038afa0279dd640df66c0cee880fa4934d2bb009e138e87fca4c9ba3f048a2c`.
Session and its admitted capture observer must use this same executable.

The pending fresh package uses the frozen diagnostic Lom runtime for two
static workspace-only arms, with one denied-GPU bar and no dock, clock or
input. The portal supplies a private full-output top-down RGBA8 PAM. Check
every pixel in its top 24 rows: alpha 255, a red/blue-distinguishing pixel,
then swap R/B and compare FNV64 against the unique Lom candidate. No flip,
PNG normalization or excluded columns are allowed. Capture SHA256 binds saved
pixels; it does not make the producer's FNV64 a cryptographic proof.

The controller brackets capture with unchanged live candidate, host and
indicator records. Every sampling row must match the 1:1 exact-nearest bar
contract. Final postflight must separately validate shutdown and reject later
scene changes. Direct/copy checksum equality is a separate criterion requiring
identical configs except the renderer and identical admitted model inputs.
The first package, `~/.local/state/t331/lom-cpu-pixels-native-01`, is spent
and failed before Session launch. Its wrapper requested a 240000-ms environment
receiver, but the pinned runner accepts at most 180000 ms and refused before
binding its socket. No Session, Lom, controller or pixel capture started;
the copy arm was not reached. The tty4 PAM session cleaned up with no residual
processes and restored the terminal. All 69 sealed inputs still verify, and
all 79 files and recorded metadata are unchanged by the read-only audit.
Failure receipt: `~/.local/state/lom-cpu-pixels-native-failure-01/RECEIPT.json`,
SHA256 `1478c419e9956dfc3e1400fb1aa13582595383acecbdd733443c585e36838f83`.
This is a package argument error, not a Lom or pixel result; never rerun 01.

Fresh `~/.local/state/t331/lom-cpu-pixels-native-02` was sealed before its attempt.
Only package paths and timing bounds change: receiver 180 seconds, host 120,
watchdog 150, collector 165, VT 200 and outer SSH 240. Readiness 30 plus the
outer controller limit 80 stays below the host deadline. The actual pinned
receiver is exercised offline: 240000 refuses without a socket; 180000 binds
a private temporary socket and is then stopped. No display is opened.

All 28 package tests and both device-free argument checks pass. The boundary
fixture executes the signed controller and feeds its actual result and saved
snapshots to postflight. Root verifies 55 tested inputs, 275 pins and all 71
manifest entries. Manifest SHA256 is
`75dd23b523eb69fb5337c8975f22da14c53b06555e98c13969bf8d43fe097840`.
Post-seal check-only passes with no runs or attempt/result marker. Root receipt:
`~/.local/state/lom-cpu-pixels-native-root-02/RECEIPT.json`, SHA256
`da8dd7e0ba21e75e866ff995cc0e5074fc2bd4a99737e9d425b2b1c06aa0cb60`.

The wrapper waits for complete log lines and permits peer disappearance only
after the exact runtime-deadline quiescence transition. Final postflight uses
the actual a56 resource schema, binds the controller's before/after snapshots
to the final log prefix, and validates the observer delivery and cleanup.
The 02 attempt reached Session and delivered `capture-3.{json,pam,stderr}`,
then the controller refused `unexpected capture attempt`: both controller and
package postflight confused Session's global launch generation with capture
ordinal 1. WM and Lom registrations had consumed generations 1 and 2. The
three-attempt budget does not limit the generation value. The wrapper exited
0 after the normal 120-second deadline; PAM cleanup completed, residual PIDs
were empty and tty4 was restored. The copy arm never started.

A separate saved-data audit verifies all 71 manifest entries and preserves
all 103 file/socket entries and metadata. It checks the delivered generation-3
receipt, empty stderr, PAM SHA and all 46,080 opaque strip pixels: produced and
captured FNV64 both `a7bbb8c840a7303c`. Independently reconstructed log prefixes
agree, but the controller saved no actual before/after/result files. Thus 02
remains failed, with a saved pixel consistency observation, not a controller
or bracket pass. Audit: `~/.local/state/lom-cpu-pixels-native-failure-02/RECEIPT.json`,
SHA256 `75803793b8f6f59ec5d12a97d5786da09803bc584bb3c7be27eb9ecd95189275`.
The existing native-log and recovery verifiers also pass on the saved text;
this does not run or replace full postflight. Hagia remote=116 remains a
shutdown observation followed by quiescent/clean, not error-free text.

Signed and pushed niltempus `1d3fe6e95b547064183e1b2b519cb52afd1165ae`,
tree `b980787264975ea59f33d2aa1610b975da358830`, fixes the controller in a new
`niltempus-lom-cpu-pixels-generation` worktree. It pins the first observed
single generation and retains it through the final exact triple check. Tests
cover generations 3 and 41, real delayed inotify delivery, changed/disappeared
or extra groups, mismatched receipts and refusal/partial files. The final gate
passes 5 pixel, 18 controller and 14 shared GPU-controller tests; three mutants
are killed and restored. Receipt: `~/.local/state/lom-cpu-pixel-controller-02/RECEIPT.json`,
SHA256 `4052f39d650045ae240d90acdf8683f3d0f9ca0e345f8a2765d3de29516932e8`.
Fresh `~/.local/state/t331/lom-cpu-pixels-native-03` was sealed before its attempt.
It applies the same dynamic join in postflight; its boundary fixture executes
the signed controller for generations 3 and 41. All 31 package tests and both
device-free argument checks pass. Root verifies 55 tested inputs, 275 pins,
67 validation inputs and 71 manifest entries. All arm/SSH configs equal 02
apart from the package path; binaries, scene, budgets and pixel criteria stay
the same. The controller compatibility receipt `PACKAGE-READY.json` projects
per-file hashes from the exact gated Git tree and labels that derivation;
it does not claim additional tests or independent timed snapshots.

03 manifest SHA256:
`61e1e08b6e79fa03bc7b85bfedc60920cc6f24d0531f2c7d166b06f16388c72e`.
Root receipt: `~/.local/state/lom-cpu-pixels-native-root-03/RECEIPT.json`, SHA256
`1dcc6d6ffd62630731a8ab43156686257ea894d26cd5a4cc571ffe5796b5b43d`.
Post-seal check-only passes with no attempt/result marker and an empty runs
directory. The first seal invocation refused the absent empty runs directory;
creating that directory mode0700 satisfied the precondition without changing
any validated file, and the successful seal is separately recorded.
03 completed both arms and the conditional comparison. Both real controllers
saved their before/after/result files; the before log is a strict prefix of the
after log, which is a strict prefix of the final raw log. The signed static
snapshot agrees at both brackets and before teardown. Each arm delivers only
`capture-3.{json,pam,stderr}`, with zero-byte stderr, receipt/PAM SHA agreement,
the pinned host executable identity and launch-custody binding. Shell candidate
1 / presentation epoch 4 remains distinct from native capture generation 7.

For both arms all 46,080 top-strip pixels are opaque, include a red/blue control,
and match the produced FNV64 `a7bbb8c840a7303c` after RGBA-to-BGRA conversion.
The captured BGRA strips are byte-identical, SHA256
`20ce6ece2d199bf93435d35e3751e95538730af465f0d3fd5f9b5ee44a06b404`.
This cross-arm result is conditioned on identical panel/theme configs except
the renderer, normalized desktop equality and identical admitted indicators
and model inputs. It is a strip claim, even though the full PAM hashes also
happen to match.

Each arm has one client/host candidate, three valid sampling rows, 118 stable
binding checks, zero resource failure/leak invariants, wrapper/controller exit
0 and clean Session/PAM/TTY cleanup. The observer exited successfully; no run
PID/starttime remains live. Hagia remote=116 is recorded at shutdown followed
by quiescent/clean, not described as error-free text. The environment receiver's
SIGTERM is the intended collection shutdown.

Root's external audit checks all 71 manifest entries, all 275 current pins,
actual controller brackets, native/capture/recovery validators, environment,
process identities and an independent FNV/byte comparison. All 143 file/socket
entries and metadata are unchanged. Independent pK and Muse reviews concur.
Audit: `~/.local/state/lom-cpu-pixels-native-audit-03/RECEIPT.json`, SHA256
`0ee9a4972c3737f981c2d17684b6fac24476d883d0e654090738c2b28c77239e`;
checksums `849bbb04ac255b05e9bc00117278bc2ba34fbe44f50a42b1689aefadf23236a5`.
This closes the two-arm opaque static scale-1 displayed-strip gate. Capture
hashes bind saved bytes; FNV does not become a cryptographic producer proof,
and shared-log joins establish consistency. Performance, changing content,
blending, scaling, other outputs and default adoption remain unqualified.
Never rerun spent 03 or relabel failed 01/02. Captures remain private outside
repositories; source/artifact holds remain until explicitly released.

### Next gate: repeated static-scene CPU comparison

This design was selected before the source gate recorded below; it is not a
native performance result. Implement in a fresh Lom worktree from `8358cb3f`;
keep the qualified static
binary, source pools and spent native packages frozen. GPU direct-render work
remains after CPU qualification. A new `lom-cpu-cadence` executable under the
existing default-off diagnostic feature selects the workload by executable
path. It accepts the host's fixed `--serve`; ordinary Lom and the existing
`lom-cpu-reference` entry keep their behavior. No host driver, public renderer
mode, environment selector, timer thread or new rendering backend is needed.

After each exact Presented outcome, mark the same static scene dirty again.
Existing one-job scheduling, resource retirement, exact ring release and fresh
commit checks still decide when another render may start. Stop requesting work
after 2,120 presentations and retain the connection through settlement and the
sampler's closing bracket. The first 120 frames are warmup, not 120 seconds.
Identical pixels in a fresh candidate still require a fresh physical presentation;
the native gate must verify a fresh bound flip through candidate/frame/completion
joins. These joins do not independently measure predecessor resource retirement.
Every qualified candidate must be rendered, never an interaction-only reuse.

Use two workloads, separately: 1920x24 and 1920x512, the latter 3,932,160 bytes
within the 4 MiB resource grant. Each has fixed direct/copy/copy/direct order,
with copy as the analysis baseline, and
one signed diagnostic runtime for both selections. Pin equivalent static panel,
theme, desktop and admitted indicator inputs. Require constant produced
checksums within an arm and equality across equivalent arms. The narrow workload
retains the already qualified `a7bbb8c840a7303c` checksum; do not invent a larger
surface checksum. This remains a repeated static Vello scene, not normal idle
bar behavior or a changing-content correctness test.

CPU anchors come from `sophia_shell_native_completion.monotonic_usec`, joined
through `sophia_shell_native_binding` to candidates 120 and 2120. Require
`timestamp_source=kernel`, `missing_kernel_timestamp=0`,
`drm_timestamp_monotonic=1`, the same clock domain as the sampler,
and unique native binding/completion identities for every counted candidate.
Refuse extra native frames in the counted window, missing or coalesced candidates,
identity changes and incomplete sample brackets. Log polling only controls
collection lifetime; it is not a timestamp or evidence of bounded log lag.
Require consecutive native frame IDs from candidate 120 through 2120 and enumerate
all completions in that window, rather than filtering unexpected frames away.
Bootstrap redraws before the start anchor are outside this count. CPUwin uses
the reported microsecond timestamps multiplied by 1,000, not reconstructed
sub-microsecond physical boundaries.

Add diagnostic-only owner timestamps for the first eligible render attempt,
successful ring commit with Offer queued, and exact Presented observation.
Retain the first timestamp across direct acquisition backpressure, just as copy
upload backpressure remains inside its interval. The SDK slice copy is synchronous
before the second timestamp, but its control bytes may reach the host later.
Thus attempt-to-Offer includes queueing, rasterization, conversion and applicable
storage work; Offer-to-kernel includes dispatch, custody, composition and pacing.
Neither is isolated raster time, host CPU time or photon latency. Carry timestamps
with the exact job/pending candidate and check ordering against kernel completion.
Treat kernel timestamp `u` as the bucket `[u*1000, u*1000+1000)` ns for ordering:
Offer must be no later than its lower bound and Presented observation no earlier
than its upper bound; ambiguity refuses timing qualification. For each measured
job, the eligible attempt follows the start anchor's upper bound and the preceding
candidate's Presented observation. Require attempt <= Offer; the final Presented
observation may follow the end kernel anchor. Report the kernel clock's
microsecond resolution in elapsed-time summaries.
Use cumulative render/upload/reuse counters to reject extra or missing work;
counter deltas alone do not establish temporal containment.

Predeclare the CPU sum as Session + Lom + Hagia, with all three process rows and
the sampler's separate overhead. No dock is present. Reuse CPUwin's inner/outer
sample bounds over 2,000 completion intervals, with no interpolation, point
estimate or confidence-interval wording. Boundary observation/retirement work
can straddle the two kernel anchors; this is process CPU per completion interval,
not exact causal attribution to those jobs. The shared diagnostic clocks, counters
and per-candidate logging are part of the workload. The copy arm also allocates
an owned image, so the difference compares the two paths, not isolated memcpy.

Report cadence as an outcome, including missed refreshes; do not discard a slower
large-surface arm merely because it cannot sustain 60 Hz. Failure to reach the
declared count within the bounded session is a qualification refusal, not a zero
cost result. Analyze each geometry separately, allow overlapping accounting bounds
as inconclusive, and do not rerun until a desired result appears. Offline source
gates, exact release receipts, a reviewed fresh package and an attended launch
remain required. No production saving or default-adoption claim follows this
design or the earlier one-Hz descriptive CPU measurements.

The offline implementation is now signed and pushed on `cpu/cadence` as
`3fe206dc25f26905531ae8365b7f4580d75fa72b`, parent `8358cb3f`, tree
`289f14e43e3553d0fdcaed413a428edff19de6f2`, in `~/dev/lom-cpu-cadence`.
Root reviewed candidate SHA256
`1ed0406fdf3153a493b8aad4449f0b85cd98197afb2e99f1b29d613d8b163e56`
and verified the source hashes remained equal through the full default/feature
gate: 239 Rust tests passed, two existing subprocess entries ignored, 18 Python
tool tests passed, and both Clippy passes were warning-free. Six new real-service
tests cover both Vello CPU arms, quota stopping, withheld Presented/release,
unchanged static behavior, lost counters/extra work, and the actual CLI owner
loop's reporter/cadence setup order. Test sockets use fresh exclusively created
directories and the watchdog is joined.

The diagnostic emits `lom_cadence`, per-candidate `lom_cadence_sample`, and a
`lom_cadence_final` counter/status record, without changing the existing candidate
schema. Its first timestamp is after the owner clones the model; it does not
include that cloning cost. Extra externally dirtied work is not suppressed: it
refuses sampling or appears in the final counters, so a native reducer must
require the exact quota and complete final status. Existing slot custody and
rendering worker code are unchanged. The owner loop enables cadence counters
after the normal reporter initializes, and rejects any work-stat environment
variable before connecting. Two setup mutations were killed before the final
fixture/lint cleanup; the complete gate binds the final exact tree.

The immutable source receipt is
`~/.local/state/lom-cpu-cadence-01/RECEIPT.json`, SHA256
`135a5ea84460db15f3885e2af92622ea56aa6aa6f75109981dc5b95a4785bcf9`.
The checksum manifest verifies. Release provenance, package qualification and
attended native measurements are separate gates; this source result establishes
no cadence, timing, CPU saving or default-adoption result.

The release gate also passed on that unchanged signed tree. The two binaries
were built together with `cargo-slot lom cargo build --locked --release
--features diagnostic-cpu-reference --bin lom --bin lom-cpu-cadence`, jobs 2,
in the worktree's own pool. Runtime `lom-cpu-cadence` is SHA256
`889219dc359535cc087a591ab4b394708e89b7c82b616a14ba3990a045ea2474`,
device/inode `2067/66087122`; the feature-on ordinary `lom` config checker is
`16c4881aa08b5a64bbc85b97ea71ad85701baa86aacadaee0a2706a374d2f41d`,
`2067/66087123`. Both remain at their original Cargo paths, mode 0700, uid 1000;
no binary was copied. The checker accepted all four static panel configurations
(24/512 rows, direct/copy), with only PATH in its environment. It is not a
production-default artifact and must never be the desktop runtime in this gate.

Root independently verified both manifests, current artifact hashes/identities
and clean signed source. `RELEASE-READY.json` in the same evidence directory is
`7559f0c4d4230180a4f6df11a9f399a9d39a0040452c89bb58d86e4b8f92efb8`;
it binds release receipt
`f673dd7b7afab387ef98a04e7ef3ff66cff47131f752cf8cdd0c1fc4e9249baf`
and checker results
`0aa3ca3a975acbac590a161e4074cadc4faeb394982367aaf8e3995490d38337`.
The source and pool are frozen through package preparation, attended attempts
and audit. Offline package integration is authorized against these exact inputs;
no native launch or performance result follows the release gate.

## Existing renderer support and pixel contract

Baseline Lom `32e0bab3` pins SDK `0706ea2c78cd16ba59ed209620354f30a2c78906`
and `imaging_vello_cpu 0.0.1` (lock checksum
`fd411d3ad5f4b8271a45b4bc69df7f88bb4236b71fbd4e5a5177c6e93eb67f44`).
The imaging crate already implements `ImageRenderer::render_source_into` using
a borrowed `ImageBufferTarget`. `finish_into_target` validates dimensions,
tight stride and length, renders into those bytes, then unpremultiplies RGBA
in place (`src/lib.rs:251–289, 542+`). No imaging API fork is needed.

Preserve Lom's conversion from straight RGBA to premultiplied BGRA, including
alpha-zero black and `(channel * alpha + 127) / 255` rounding
(`src/protocol/pixels.rs`). Render into exactly `width * height * 4` bytes and
convert in place. Bypassing the unpremultiply/re-premultiply sequence would be
a separate optimization requiring equivalence proof.

The existing SDK `image_upload_ring_fill` is synchronous. `RingSlotFill` borrows
`&mut RingProducer`; that borrow cannot cross a persistent worker boundary while
the service continues polling the connection. Joining a scoped render thread on
the service thread would also block protocol work. No forged lifetime or shared
mutable raw mapping is acceptable.

## Proposed minimum owned-loan contract

Add one owned fill loan alongside existing borrowed and slice APIs. API names
remain provisional until the SDK implementation review.

1. Acquire only with a ready ring and free slot. Move the existing `MappedRing`
   into a non-cloneable loan exposing only that slot's logical payload, never
   headers or other slots. Allow one detached loan per producer. The exclusively
   owned mapping is already `Send` in `sophia-isb`; no new memory sharing is needed.
2. Keep wire I/O and slot/offer ledgers on the service thread. Other offers can
   receive status and exact SlotReleased events during rendering. Mapping-writing
   APIs return busy while loaned; registration cannot restart through a live loan.
3. Return the loan and render result through Lom's existing bounded completion
   channel. The worker cannot publish. Completion and worker-exit wakes cover
   success, failure and panic; failed sends must recover the owned job/result.
4. Commit only on the service thread, rechecking producer/loan identity, ring and
   connection state, current render identity, dimensions and control capacity.
   Today's borrowed guard prevents intervening producer mutation; an owned loan
   needs these explicit checks. On `Again`, retain/return the completed loan for
   bounded retry without rerendering or consuming offer IDs or slot generations.
5. Explicit abort returns the mapping and frees the unoffered acquisition without
   sending or spending anything. Render errors return the loan for abort.
   Dropping an unreturned loan must instead terminate that ring, never mark it
   reusable. A private producer identity, consumed-payload checks and an abandonment signal checked by owner
   polling avoid a second SDK worker/return queue. The exact signal and caller
   wake obligations still require implementation tests.
6. Disconnect, revoke, stale allocation and timeout cannot reclaim memory from a
   running worker or authorize a late commit. Retain custody until return/drop,
   reject late results and follow existing failure policy. Preserve one render
   job in flight; do not widen presentation/resource concurrency.

Source trace at the pinned SDK: `sophia-image-producer/src/ring.rs` contains
acquire/commit, event ledgers, close/journal loss and `RingSlotFill`;
`sophia-shell-client/src/files/image_adapter.rs` contains pending control and
final notification; `sophia-isb/src/{lib,mapping}.rs` owns the mapping. Mapping
access is needed for pre-Ready source publication and fill/commit, while ring
events update ledgers. Test every state entry point with the mapping loaned.

Lom currently sets `ring_capacity: 0` (`src/service.rs:268`), selecting the SDK's
negotiated maximum resource capacity rounded down to a multiple of four. That
registration is fixed while panel sizes can vary. Preserve it initially and
expose an exact frame-length slice within it; check untouched guard bytes.
An exact-sized ring per frame would be another lifecycle change, not this copy
removal. The serial benchmark's exact-capacity recipe is not a general Lom rule.

## Integration and qualification

SDK changes reach the producer, shell-client adapter/public exports, API docs and
lifecycle tests. Existing borrowed-fill and slice callers keep their contracts.
The loan itself needs no wire format, protocol allocation, W2 or host change.

Lom changes reach explicit CPU startup selection, a CPU worker using the retained
scene construction, job/result types, scheduler, presentation commit and shared
pixel conversion. CPU startup must not create a GPU grant/adapter. GPU/DMA stays
explicit and unchanged. Carry `RenderIdentity` (grant, output, allocation and scale
generations) with the job and revalidate on return. Prospective resource IDs must
not advance live counters before successful commit.

Before any default change or native recipe:

- Compare identical Vello CPU scenes through owned-buffer/slice upload and direct
  slot rendering, byte for byte after the same conversion. Include text, alpha,
  clipping, scaling and larger surfaces, not only opaque shade fill.
- Prove exact length, untouched guard bytes, foreign/stale loan refusal, one
  acquisition/commit, no spending on abort/Again/error, and reuse only after exact
  SlotReleased. Acceptance or ResourceReleased alone cannot free an offered slot.
- Exercise worker stall/panic/drop, request/completion send failure, disconnect,
  revoke, timeout, late return, pending control and all slots busy. Owner polling
  must continue while the worker stalls. Preserve existing slice/GPU/DMA tests.
- Prove CPU startup with GPU denied and GPU refusal without silent fallback.

Then qualify native pixels, cadence, release custody and cleanup. Isolate the
copy effect with **the same new CPU renderer**: owned buffer → conversion → slice
upload versus slot → conversion → commit. GPU readback is not that baseline.
Pin scenes, renderer, capacity, host and recipe. Predeclare combined/per-process
CPU intervals, elapsed render-to-ready time and cadence at bar and larger sizes,
plus a stop rule; no repeat-until-win. CPU accounting is not latency or GPU time.

A 1920×24 bar is 46,080 pixels / 184,320 bytes. CPU may finish simple small scenes
sooner by avoiding GPU submission/export overhead, but the crossover is unmeasured.
The [matched CPU comparison](iq4zf50w-how-do-we-compare-sophia-xlibre-and-niri-with-matched-pixel-producers.md)
does not establish a direct scene-rendering gain. The
[GPU qualification work](nb3mgbwc-what-must-match-before-comparing-gpu-buffers-across-sophia-niri-and-xlibre.md)
and direct-export-target optimization remain after this CPU work.
