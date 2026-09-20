# Independent X11 socket conformance gate

Run from this checkout:

```sh
python3 -B tools/probes/x11_conformance/check.py --output /tmp/sophia-x11-results
```

The evidence directory must be new. The command runs gate regressions, builds
the software-only `x11_conformance_host`, then runs every mandatory case in
both byte orders. `--target-dir /absolute/build-cache` selects a build cache.
Python, the offline Rust dependencies and the normal X-authority build
dependencies are required. Private Unix socket creation must be permitted.
Socket denial is a launch failure, not protocol evidence.

For comparisons across archived sources or when build freshness is in doubt,
pass a new, previously unused `--target-dir` for each candidate. Keep both reports;
do not infer a candidate's identity from a shared cached executable. The report
records the actual host digest and source identity, and marks dirty checkouts.

No Sophia session, renderer, DRM device, input device, VT, installation or
operator display is used. The runner clears inherited `SOPHIA_*`, `HAGIA_*`,
display endpoints and `PYTHONOPTIMIZE`. It creates a new mode-0700 temporary
directory and starts the production `XServerFrontend` with its real routed
broker and concurrent workers there. There is deliberately no public option
to attach the gate to an existing display. The test namespace is ClassicShared;
this gate does not certify confined namespace separation or Session policy.

The host has deterministic software output facts and no render-device provider.
DRI3 is therefore expected to be unadvertised. This fixture limitation is
distinct from the deliberate Composite/DAMAGE/XTEST/DPMS exclusions and from
missing implementations of mandatory protocol requests.

## Independent assertions and mandatory accounting

`wire.py` implements client framing directly from the
[X11 protocol specification](https://xorg.freedesktop.org/archive/X11R7.7/doc/xproto/x11protocol.html).
It imports no Sophia encoders, decoders, constants or generated bindings.
`cases.py` observes replies, error sequences/resources, subscribed events,
cross-client property/selection effects, grab contention and cleanup. GetInputFocus
round trips establish request ordering; a successful write is never a barrier.
Each case has an absolute socket deadline, record/backlog limits and an outer
client-process deadline. Progress or unrelated events do not reset the deadline.
Only process groups created by the runner are terminated.

Setup containment cases cover EOF before any bytes, each truncated prefix
length, truncation throughout padded authorization fields, an invalid byte-order
marker, and an unsupported major version. After each rejected connection, an
existing client's window must survive and a new client must complete setup and
query it. Both byte orders run. These cases certify containment, not complete
version-negotiation refusal semantics. The host polls and reaps workers without
waiting for another connection, so an idle blocking accept cannot hide a fatal
worker result. No preflight ever connects to an existing display.

Lifecycle cases distinguish explicit DestroyWindow from owner disconnect. They
check descendant event order, both StructureNotify/SubstructureNotify addresses,
no-mask suppression, stale subscriptions after XID reuse, and automatic unmap
before mapped destruction. DestroySubwindows is tested with empty/invalid targets
and with newer siblings restacked below older ones; allocation order cannot
accidentally satisfy the stacking assertion. Each behavior has its own mandatory
case so a passing notification-presence check cannot hide an ordering failure.

Drawing, graphics-context, pixmap and image cases (`drawing_cases.py`) judge a
request by what it left in the drawable: every pixel assertion reads the target
back through GetImage in ZPixmap form, decoded with the server's advertised
image byte order, never by the absence of an error. Expected pixels follow the
protocol's own pixelization rules (filled rectangles cover [x, x+width) by
[y, y+height); thin horizontal, vertical and 45-degree lines pass through pixel
centers; an arc is inscribed in its rectangle, so a full disc holds its center
and none of the box corners). Exposure obligations are read as events:
NoExposure for a wholly available copy source, GraphicsExposure for the missing
part, Expose for a ClearArea that asked for it, and silence when
graphics-exposures is False. Refusals cover extents, depth, XID choice,
enumerated values, root/depth binding, freed resources and InputOnly drawables.
These cases certify the software fixture's raster, not GPU composition.

`manifest.json` binds the mandatory profile to cases, core request numbers and
extension obligations. Every required case must produce exactly one PASS per
byte order. Missing, duplicate, unexecuted, unknown, malformed, NORESULT,
UNSUPPORTED, UNTESTED and timeout results fail. A PASS with a failing process
exit also fails. There are no XFAIL baselines or automatic skip-to-pass paths.
Python `-O` is rejected so assertions cannot silently disappear.

The source declaration inventory is a separate coverage audit, not an oracle
for wire behavior. Every currently decoded core request has either named cases
or explicit coverage debt. A new decoder arm missing from the inventory fails
the audit. Coverage debt is reported, not counted as tested. The selected gate
does not claim all core requests, every extension operation, actual input
delivery, GPU behavior, or the native protocol-family t023 exit. Add independently
specified cases when expanding those obligations; do not regenerate expected
behavior from Sophia replies.

`report.json` retains complete verdicts, scope, coverage debt, host digest,
harness/manifest digests, source commit and dirty-state flag. A source commit
alone does not identify a dirty build. Host logs are per case. During development,
changes between a build and a run require rebuilding; preserve the report with
its actual binary hash. A run on an older candidate remains older evidence.

XFixes selection obligations also exercise reasserted and rapidly changing
owners, explicit clear, replacement/zero masks, invalid subscription requests,
window destruction versus client close, subscription retirement on XID reuse,
and same-client delivery. Assertions cover the recipient sequence and resolved
CurrentTime fields. The semantics are checked against XFixes selection tracking
in [fixesproto](https://github.com/X11Libre/mirror.fdo.xorgproto/blob/master/fixesproto.txt)
and the local XLibre `Xext/xfixes/select.c` and `dix/selection.c`; no implementation
code is copied. This software fixture still does not certify namespace policy.

## Optional selected XTS5 adapter

XTS is a separate checkout/build; the yserver checkout does not supply it,
and nothing here is XTS evidence until a real scenario has run. Obtaining
it is an operator step, once, outside the repository:

```sh
xbps-install -S libXt-devel libXaw-devel libXmu-devel xorg-util-macros bdftopcf
git clone https://gitlab.freedesktop.org/xorg/test/xts.git ~/src/xts
cd ~/src/xts && ./autogen.sh && make -j                      # no install
```

The freedesktop GitLab refuses scripted fetches, so the clone needs a
browser-authenticated session or a mirror the operator trusts; record the
commit. Under a current GCC the K&R-era sources may need
`CFLAGS='-std=gnu89 -Wno-error=implicit-function-declaration -Wno-error=implicit-int'`.
The tree bundles TET, built to `src/tet3/tcc/tcc`; configure also wants
`bdftopcf` for the test fonts. Configure in-tree, so the paths it bakes are
relative.

One file from this directory goes into the checkout, over the `check.sh`
its configure generates. The generated one regenerates `xts5/tetexec.cfg`
through `xts-config`, which runs `xset q` and `xdpyinfo` against the display,
and the fixture host decodes neither the font-path nor the keyboard-control
requests those need. `xts_check.sh`, copied to the root as `check.sh`, writes
the configuration from the suite's own template instead (display `:99`,
empty font paths, no reset delay) and runs the bundled TET at
`src/tet3/tcc/tcc` the way `xts-run` does, with the journal placed where the
adapter looks. `xts_select.py` enumerates the selection from the suite that
will run, never by hand:

```sh
python3 -B tools/probes/x11_conformance/xts_select.py \
  --xts-root ~/src/xts --scenario selected-core --install \
  --case XDestroyWindow --case XMapWindow --case XInternAtom \
  --case XChangeProperty --case XGetSelectionOwner --case XSetInputFocus \
  --manifest tools/probes/x11_conformance/xts_expected_selected_core.json
```

It counts each case's purposes from the `>>ASSERTION` markers of its `.m`
sources, names the case the way the suite's scenario file and TET's journal
do (`/Xlib3/XDestroyWindow`), writes the manifest, and with `--install`
appends a `selected-core` scenario to `xts5/tet_scen`, replacing an earlier
one of that name, since the runner selects scenarios by name from that one
file. `tcc` selects whole cases, so every purpose of a selected case is
mandatory and must pass. Select the window, atom, property, selection and
focus requests the software fixture decodes; requests that need real devices,
fonts or text do not belong. Grow the selection by measurement: a case the
fixture fails is listed with its journal line, never dropped silently, and a
BLOCKED run stays BLOCKED with its blocker text.

```sh
python3 -B tools/probes/x11_conformance/xts.py \
  --host .artifacts/x11-conformance-target/debug/examples/x11_conformance_host \
  --xts-root ~/src/xts \
  --scenario selected-core \
  --expected tools/probes/x11_conformance/xts_expected_selected_core.json \
  --output /tmp/sophia-selected-xts --timeout 600
```

The adapter requires the separate checkout's `check.sh`, built `xts5` directory,
executable TET `tcc`, bubblewrap, the exact selected-purpose manifest and the
selected scenario. It copies the external tree privately, excludes old results
and `tetexec.cfg`, then runs with a tmpfs root and explicit runtime/data mounts,
a private `/tmp/.X11-unix/X99`, network namespace and `/dev`. Every program
libtool built in the checkout is a wrapper that names the checkout's original
absolute path for its libraries; `check.sh` names every built library
directory of the private copy in `LD_LIBRARY_PATH` instead, since the loader
skips a directory that is not there. Host `/run`, `/etc`
and home directories are not mounted; a read-only host root is not sufficient to
hide pathname sockets. An old wrapper hardcoding a host display cannot reach
that display. First-run unknowns to confirm and record: TET's journal header
without `/etc/passwd`, locale warnings without `/usr/share/locale` bindings,
and the suite startup's font-path calls with empty paths.

Exactly one fresh journal is required. Every declared mandatory purpose must
start and finish PASS. The numeric TET verdict must agree with its text;
duplicate/unstarted records fail. Process failure/timeout fails even when a
partial journal contains PASS results. This deliberately rejects the yserver
comparator's PASS-to-NORESULT and missing-candidate-purpose false positives.
No results from an older directory can supply a pass.

With missing dependencies the adapter writes `BLOCKED`, `suite_executed=false`
and concrete missing paths/tools, and exits 2. Under the gates,
`cargo xtask check x11-profile` and `m6-evidence` take `--xts-root`,
`--xts-expected` and `--xts-scenario` together or not at all, and
`--xts-timeout` (default 600 s, at most 1785, leaving the gate 30 s) becomes
the adapter's own deadline. A PASS on `selected-core` claims that the
manifested Xlib purposes pass against the software fixture host, and claims
nothing about the rest of XTS5, devices, fonts, text, namespace policy or
physical acceptance. Synthetic TET fixtures test adapter isolation and
reporting only; they are not XTS evidence.

## Private native input and XTEST profiles

The implementation contract is
[7xqjn8rp](../../../docs/notes/plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md).
`core` remains the default-off 100-execution profile. Both wire profiles launch
their clients only through the supervised containment entry; the old internal
`--child SOCKET` path is refused. `native-input` runs exact
named Rust obligations. `xtest` runs independent XTEST 2.1 clients in both byte
orders against the Session example. `all` requires all three profiles plus the
real containment regressions. An incomplete implementation fails; these
commands do not grant implementation or deployment acceptance by themselves.

```sh
python3 -B tools/probes/x11_conformance/check.py \
  --profile all \
  --target-dir .artifacts/native-input-target \
  --output /tmp/sophia-native-input-all
```

The same two profiles are produced by a gate rather than by hand through

```sh
cargo xtask check x11-profile --profile=all \
  --output=/ABS/.artifacts/x11-profile-<label> \
  --target-dir=/ABS/.artifacts/x11-profile-target
```

which refuses a dirty tree, snapshots the committed source beside the
evidence, runs each profile through this `check.py` with its own log and
absolute deadline, and judges each by the report it wrote: the probe's own
rules, plus that the report names the candidate commit and a clean tree, or
it is NORESULT rather than a pass. XTS5 is BLOCKED and unrun unless
`--xts-root` and `--xts-expected` are both supplied; a BLOCKED XTS never
passes by absence and never fails the profiles. `core` stays with its own
gate above.

Each output directory must be new. Keep large targets on disk instead of a
memory-backed `/tmp`. All offline commands clear inherited Sophia, Hagia,
DBUS, XDG and display settings. The private profile builds
`sophia-session --example native_input_conformance_host` without native-session.
It never selects an operator display, DRM device or VT.

For each XTEST case, the supervisor creates a contained instance and delegates
a fresh 32-byte setup credential to the host and authorized test clients.
Ordinary empty-auth clients remain ungranted. Wrong supplied credentials must
fail setup. Socket placement hides discovery; it is not the grant. The
example's enabled/disabled construction modes have no live-session equivalent.

`isolation.py` exposes only explicit runtime/artifact mounts and descriptor
delegation. It starts a new session with no terminal, discards ambient settings,
closes unrelated descriptors and validates kernel namespace descriptors before
the inner entry creates any sockets. `--inside` alone fails. Validation is a
runner contract, not attestation against an actor constructing its own genuine
namespaces. There is no ambient authorization callback or fallback backend.

```sh
python3 -B tools/probes/x11_conformance/test_isolation.py
```

This command must exit zero. Missing kernel isolation is BLOCKED, not a skip
that can satisfy acceptance. Fabricated endpoint reachability controls and
mount, descriptor, environment and namespace mutations establish that the
negative assertions can fail. No operator endpoint is probed.

`native_manifest.json` binds every native obligation to an exact Cargo package,
test target and test name. An unmapped obligation fails. The runner requires
the named execution and a summary with one pass and no ignored tests; exit
zero with no matching test is NORESULT. Native fixture tests labelled physical
are headless source models, never physical input acceptance. The native
authority profile remains distinct from what an X11 observer can establish.

## Gate regressions and references

```sh
python3 -B -m unittest discover -s tools/probes/x11_conformance -p test_gate.py -v
```

These include absent/empty results, NORESULT, unsupported/untested statuses,
duplicates, nonexistent mandatory cases, process timeouts despite continuing
output, optimized-Python refusal, extension inventory drift and strict TET
purpose accounting. The real socket run is separate and can correctly fail
while all gate-regression tests pass.

Reviewed external references: `~/src/yserver/tools/xts-run.sh`,
`xts-vs-baseline.py`, `fontset-probe.c`, `xid-exhaust-probe.c`; and XLibre's
`~/src/xserver/test/pyxtest` and `test/xi2`. XLibre's raw-protocol tests and
swapped-byte-order tests are useful models. Its default Xvfb/Xorg launchers and
live-display option are not used here. No external implementation code was copied.

See the [investigation](../../../docs/notes/investigations/wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md)
for candidate-specific failures and linked repair tasks. The gate must remain
red until those mandatory behaviors work; it does not establish a pinentry cause.

The XFixes stalled-watcher case leaves one subscriber unread while generating
4,096 bounded ownership assertions. A second subscriber must receive every
notification, the stalled socket must close, and both the sender and fresh
admission must remain usable. A live socket with silently lost notifications
fails by deadline. This pressure check does not certify every routed event
family; older destroy/MSC recipient handling is tracked separately as t090.

The mixed-owner descendant case closes a parent client while another client owns
its child and a separate selection. It requires actual child destruction,
client-close subtype 2 for the parent, window-destroy subtype 1 for the child,
retained ownership timestamps and continued service for the surviving peer.


## Canonical workspace checks without hardware access

Use the contained wrapper for an offline `cargo xtask check`. Clearing
`SOPHIA_*` and `HAGIA_*` is insufficient: parts of the canonical gate also
probe writable render nodes automatically. The wrapper supplies a private
`/dev` without DRM or input devices, clears the environment, and closes
unrelated descriptors before starting the unchanged canonical command.

```sh
python3 -B tools/probes/x11_conformance/offline_check.py \
  --source /absolute/clean-checkout \
  --verification-key /absolute/public-verification.gpg \
  --hagia-source /absolute/hagia \
  --hagia-commit a12fc5cc398fd4692bfb693df25184bbec9ebc76 \
  --narthex-source /absolute/narthex \
  --narthex-commit f270248f7368cef2e5a18023958627d12f8bf7eb \
  --target-dir /absolute/main-checkout/.artifacts/offline-target \
  --output /absolute/main-checkout/.artifacts/offline-check-new
```

Both output and target must be distinct, disk-backed children of the main
checkout's `.artifacts`; build targets under `/tmp` are refused. The source
must be clean and committed. The `rg` that is mounted must be one the
container can execute: statically linked, or linked against the system
loader the private root carries. A build linked against another loader,
such as linuxbrew's, exists on the host and cannot start inside, where exec
reports the file itself missing; the wrapper skips such a candidate on
`PATH`, names it and its loader in a `BLOCKED` report if nothing else is
found, and accepts `--rg PATH` to name the executable outright. The wrapper copies the exact commit into an
independent repository with one parent commit, records its tree/archive hash and
toolchain hashes, and mounts only the offline registry cache from Cargo home. It generates a
loopback-only `/etc/hosts` for regular-file refusal tests, generates its loader
cache from the allowlisted libraries, and links the private source copy's `target` to the explicitly owned target directory for profile
binary discovery. These fixtures expose neither an installed Sophia nor host `/etc`. The exact
`rg` executable is mounted separately and hashed; compiler and required helper
versions are checked before the workspace suite, so a missing helper cannot be
mistaken for source-layout evidence.

Full checks require both explicit sibling source/commit pairs. Commits must be
complete lowercase 40-hex identities, never branch names or moving `HEAD`.
The sibling repositories may contain unrelated working edits: the wrapper reads
only the specified immutable commit object and never looks for sibling checkouts
implicitly. The pinned values above are explicit inputs, not automatic upgrades.

Each sibling becomes a fresh, non-bare **identity repository**, not a source
snapshot. It contains the exact raw commit object, detached `HEAD` and a depth-one
shallow boundary. There is no checkout, index, remote, copied configuration,
alternate object store or external Git link. Tree identities are recorded, but
tree/blob objects and binaries are omitted; their contents are **not verified**.
Consumers needing those objects must fail instead of reaching a host checkout.
Only these generated identity repositories are mounted read-only, at
`/work/dependencies/hagia` and `/work/dependencies/narthex`. The wrapper sets
`SOPHIA_HAGIA_ROOT` and `SOPHIA_NARTHEX_ROOT` to those paths only inside containment.
Commit payloads have a 1-MiB bound enforced while reading subprocess output,
separate from the verifier's default 256-KiB output limit, and bounded Git waits. Object IDs
and SHA256 hashes are checked during copying, before/after signature verification
and after the contained command; sibling moving-HEAD and worktree state are not
used as identity evidence.

Full checks also require an explicit public OpenPGP export containing the
signers of the source commit, its parent and the two pinned sibling commits.
The archive-verifier fixtures use these identities and genuine signature
verification. Export only those known
fingerprints; for example, after identifying the required signer:

```sh
gpg --batch --no-options --no-autostart --no-auto-key-retrieve \
  --no-auto-check-trustdb --export-options export-minimal \
  --export EXACT_SIGNING_FINGERPRINT > /absolute/public-verification.gpg
```

The wrapper accepts one nonempty regular file up to 64 KiB, records its SHA256,
and mounts that file alone. It never mounts a host keyring, private keys,
ownertrust database, GnuPG configuration, or agent. Inside containment it
inspects packets before import, refuses secret-key packets, and imports only
accepted public data into a fresh private `GNUPGHOME`. Packet diagnostics stay
in bounded memory, not evidence logs. Signature commands have a 30-second
deadline and a 256-KiB output limit. The report records imported fingerprints
and genuine signature results for the source pair and both sibling commits,
all using the same fresh private GPG home. Unknown ownertrust warnings
are possible because host trust is not copied. Missing keys, malformed input,
and failed signatures block before the canonical command runs.

`--validate-only` checks tool versions and offline Cargo metadata without
building or running the workspace gate. It may omit complete sibling pairs,
which are reported `NOT_RUN`; partial pairs remain errors. Without a key it makes no signature
claim; supplying `--verification-key` adds the same signature preflight for
the source pair and any explicitly supplied siblings so its
prerequisites can be tested without a full check. Reports distinguish these
paths from an invoked full check. A failed invocation remains failed; hardware proofs and
host promoted archives remain unrun inside this environment. This command
cannot establish physical-input or display acceptance.

Wrapper regression commands (no full workspace check):

```sh
python3 -B -W error -m unittest discover -s tools/probes/x11_conformance -p test_offline_check.py
python3 -B tools/probes/x11_conformance/test_isolation.py
```

The second command requires working unprivileged namespaces and fails when they
are unavailable. Its socket endpoints and inherited descriptors are fabricated
by the tests; it never probes the operator's display or service endpoints.
