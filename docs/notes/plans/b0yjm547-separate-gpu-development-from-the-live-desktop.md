---
id: b0yjm547
date: 2026-10-07
kind: plan
tags: [plan, development, rendering]
---
# Separate GPU development from the live desktop

The operator requests Codex and Claude work concurrently on separate GPUs and
close tasks once their acceptance criteria hold. The normal desktop remains
usable on discrete DP-1. Development automation is driven from the host; a
second physical keyboard or mouse is not required for automated controls.

## Ownership

- Codex: iGPU runner/admission, t289 and this setup.
- Claude: dGPU offscreen correctness, t309 startup diagnostics, and the
  existing t306/t307 work. A device runner does not waive source review.
- Each agent uses its existing checkout and private build target. Concurrent
  CPU work is allowed with bounded parallelism; comparative measurements still
  need an agreed quiet window across both agents and the live desktop.
- Accepted work with a green required gate is merged and pushed under the
  standing rule. A task requiring physical acceptance stays open until that
  acceptance, even when a code slice has landed.

Host identities observed on 2026-10-07:

| Use | PCI identity | Current render node | Connector |
| --- | --- | --- | --- |
| Daily desktop / Claude offscreen | 0000:03:00.0 | renderD128 | DP-1 |
| Codex development | 0000:16:00.0 | renderD129 | HDMI-A-2 |

Node numbers are observations, not selection policy. The runner must resolve
and check the physical identity each time. The normal desktop currently opens
both cards even though its profile disables HDMI-A-2.

## t311

Provide a bounded runner for an already-built, hashed renderer test executable.
Select exactly one PCI GPU and expose only its render node in a private mount,
PID, IPC and network namespace. Hide primary DRM nodes, physical input, VTs,
live display sockets, session control sockets and the user's home. Start with
clean environment and standard input; inherit no ambient device descriptors.
Give the test a private temporary home and runtime directory. Serialize runs
on each GPU while allowing different GPUs concurrently.

Record the test and runner hashes, command, physical device and node identity,
namespace-visible devices, exit status and timeout/cleanup outcome in a fresh
output directory. A successful process exit does not by itself prove hardware
rendering or pixel correctness: the selected test must provide those controls.
This is for correctness, not a quiet-host performance result.

Exit: deterministic controls reject ambiguous/wrong devices and prove the
command construction has no extra devices or ambient display state. An actual
sandbox control confirms denied access to the other GPU, card nodes, input,
VTs and host session sockets. A bounded renderer correctness invocation on
each GPU passes while the daily desktop remains alive. No KMS, seat change,
lock, install or hotplug is part of this exit.

### Qualification, 2026-10-07

Runner source `68c75b1064269360674a0c4b746d77bac97529ff` passed the
isolated full `cargo xtask check` gate with devices hidden. Its 17 Python
controls are part of that gate. Claude's read-only source review found no
blocking admission or confinement issue.

The actual sandbox control denied opens of the other render node, both primary
cards, physical input, physical VTs and session sockets. `/dev/tty` had no
controlling terminal, and the live abstract X11 endpoint was unreachable.
The first preflight attempt stopped before launch because bubblewrap required
explicit `--unshare-user` alongside `--unshare-all`; its failed receipt is kept.

Both agents then used the same frozen `snapshot_reuse` executable, SHA-256
`20b714f10418a1316d181f9938fa9ba0621e4ff6cab78eb5b527e04f43b44352`,
for one invocation per GPU with a 60-second limit:

| Agent | PCI GPU | Snapshot tests | Exit |
| --- | --- | --- | --- |
| Codex | 0000:16:00.0 | 6 passed, none skipped | 0 |
| Claude | 0000:03:00.0 | 6 passed, none skipped | 0 |

The tests assert immutable pixels after producer rewrites, bounded allocation
and import reuse, descriptor lifetime through donor destruction, and eviction
and cleanup. Each namespace exposed only its selected render node and inherited
no descriptor above standard streams. No test process remained afterwards.
The live desktop's PID and start time, its helper processes, and connector
states were unchanged: DP-1 enabled, HDMI-A-2 disabled.

Evidence is retained under `development-evidence/igpu-development-01`:
`runner-gate.log`, `runner-boundary-01`, `SNAPSHOT-QUALIFICATION.json`,
`igpu-snapshot-01`, `dgpu-snapshot-01`, `desktop-before.json`,
`desktop-after.json`, and `RESULT.txt`. `RUNNER-REVIEW-LIMITS.json` records
bubblewrap identity and receipt limitations. One optional mutation harness
stopped on a selection exception; it is retained without a mutation-score claim.

These are bounded offscreen correctness results while the desktop remained
alive. They establish neither performance nor exclusive GPU ownership. The
desktop still opens both render nodes; the runner lock coordinates cooperating
test runners only. Libraries are read-only but not frozen, and the reviewed
test can write its artifacts, including the admission file. This qualification
does not clear t306/t307 or establish the independent visible Session in t312.

## t312

An independent visible native development Session needs more than output
profile exclusion. Current scanout selection enumerates all primary nodes;
construction compares owned heads with global sysfs connectors. The normal
launcher changes VT modes and native --no-input is refused. Render inventory
already has udev seat filtering, but scanout and connector projection do not
share that admission boundary.

Define one admitted device set for primary/render nodes, connectors, initial
ownership, hotplug and replacement. Scope completeness checks to that set,
without ignoring a failed admitted device. Prove the daily session cannot
open the development primary card and the development session cannot open
the daily card, input or VT. Specify independent seat lifecycle, automated
input and cleanup before implementing a launcher. Do not bypass seat admission
or the existing recovery guard merely to make a second process start.

Exit: two visible sessions run concurrently; development start, bounded exit,
crash recovery and admitted-head changes leave the daily desktop presenting
and taking input. Device/seat configuration and any one-time daily relogin are
prepared for review before application. This remains open after t311 lands.

### First source slice: seat-scoped DRM discovery

Primary selection and render inventory now share a bounded udev seat-card
inventory. Missing `ID_SEAT` means seat0 only on an initialized record. The
seat comes from the opened libseat controller. An output profile or an
`--input-seat` argument does not grant ownership of another seat's cards.
An uninitialized card refuses discovery before its node is inspected, because
its assignment is not yet authoritative. This refusal is global, including a
provisionally foreign card. Startup fails without retry and currently records
an unclassified startup error. Render inventory reduces this error to
`DiscoveryUnavailable`; it cannot distinguish an uninitialized card from a
failed discovery service. The live inventory comparison retries after 250 ms.
No uninitialized card silently disappears from the completeness check.
The inventory is collected before opening nodes. A seated primary open must
preserve the admitted node's device number, inode, filesystem, physical path
and seat assignment before any KMS query uses that descriptor. An admitted
open failure refuses the selection instead of falling back to another card.

The seated scanout constructor reads connector facts only for those admitted
cards, including its completeness check. Connector IDs are matched together
with their card because their numeric values may overlap between GPUs. Its
image-import render devices come from the same card inventory. Startup,
topology replacement and seat reacquisition use this constructor. Standalone
card probes retain their explicit, host-wide discovery route.

CPU controls cover foreign-card exclusion, uninitialized-card refusal, failed admitted
discovery, capacity and identity aliases, returned-descriptor identity,
foreign-connector exclusion before reading its facts, connector-ID collisions
between cards, and refusal to fall back after an admitted open fails. Evidence
and review for this slice live in `development-evidence/t312-seat-admission-01`.
This is source preparation; physical seat assignment and concurrent visible
sessions have not been qualified.

The constructor's ordering has source review and controls for its individual
parts, but no end-to-end constructor control without devices. Existing seat
lease release calls the broker's close path; the raw descriptor ownership
repair remains in the unpublished t306 work. This slice does not qualify
that release behavior or change the unseated probe path.

The reviewed first slice is published through `3941fdf7d`. Its frozen isolated
full gate passed with 7,205 Rust tests, zero failed and 100 ignored; formatting
and the focused watchdog suite also passed. Two earlier red gates remain in
the evidence. One refused an application-diagnostics fixture as already
running; the timestamp-based root collision explanation is unproved, and the
unchanged suite passed later. The other exposed a watchdog test that observed
its completion flag before its thread became reapable. A test-only change
waits for the actual successful reap within the existing two-second bound.
No watchdog production behavior changed.

Both GPUs are still assigned to seat0 on this host. This change alone therefore
does not release the iGPU from the daily desktop. Remaining work is to scope
topology notifications and recovery to admitted devices, qualify an independent
seat without taking the daily VT or input, and prepare a private development
launcher with bounded crash cleanup. Any host configuration and daily relogin
must follow the prepared review, while the daily desktop remains protected.

### Second source slice: topology notices follow the seat

The topology monitor takes the opened libseat controller's seat. It subscribes
to kernel and processed udev events before collecting the shared card inventory.
Kernel output notices match cached admitted card paths, their exact connector
children, or strict render-node siblings under the same DRM parent. Removal
does not depend on a current sysfs lookup or seat property. An identical card
name on a different physical path does not match.

Processed device lifecycle events schedule a fresh comparison using the same
initialized seat inventory. Per-event seat properties are not used to admit
devices. Reassignment and removal compare against the prior membership before
replacing it, so losing admission still notifies the owner. New settled members
notify even without a HOTPLUG property. Replays and stable foreign changes do
not emit an output notice or advance the owner's input epoch.

A failed comparison retains the old identities and retries after 250 ms without
needing another event. The shared inventory refuses any uninitialized card,
even on another seat, so this retry can continue indefinitely while a foreign
udev record remains uninitialized. New local membership and reassignment
comparison wait for that record to settle. Known-path kernel revocations still
publish immediately. No partial inventory is installed. Foreign events can
cause metadata comparisons; this is not a claim of zero CPU work for them.

Controls cover physical-path and card-name separation, strict render siblings,
removal before and after baseline establishment, settled admission versus
replay, reassignment, inode/device-number replacement, paced retry, and a full
coalescing notice queue retaining revocation. Subscription and owner-loop
wiring are source-reviewed; these CPU controls do not exercise real udev
delivery or establish two visible sessions. Evidence is retained in
`development-evidence/t312-seat-topology-01`. t312 remains open.

This slice is published as `2c785caad` plus the test-only `06b88226a`.
The frozen full isolated gate passed: 7,215 Rust tests, zero failed and 100
ignored, with formatting and layout checks passing. Claude's source review
accepted both commits. Six mutation controls failed as expected on the first
pass; one exposed a test that sent another event after the failure. The added
no-event retry control rejects that mutation. Both runs are kept.

Two limits remain visible: failed comparisons currently warn on every 250 ms
retry, which can spend that record name's diagnostic quota during a prolonged
udev failure; kernel revocation followed by a settled inventory change can
produce two notices for one physical change. The bounded queue coalesces them,
and notice counts are not counts of physical changes. Neither source slice
was installed or exercised through a physical seat reassignment.

### Third source slice: independent login admission

The inputless native path is now conditional on `--development-seat` and a
runtime bound of at most 300 seconds. The Session queries logind through a
pure-Rust D-Bus client, using the authenticated caller (`GetSessionByPID(0)`),
not the process-local libsystemd cgroup parser. It requires stable session and
service identities, matching UID and non-seat0 seat, active/local graphical
user status, no TTY or VT, and `CanTTY=false`. Fresh property calls bypass
caches. The explicit logind backend and matching XDG session ID prevent
libseat's display-session fallback. The opened libseat seat is checked and
the login re-observed before native discovery.

An argument-only preflight does not establish login authority. The separate
`--validate-development-login` path checks the login and returns before
endpoints, libseat or devices. The Python host checker is a read-only receipt
tool; it loads host libelogind only into the host Python process. The actual
Session uses the system bus and does not load host libraries into Nix libc.

Controls cover the mode and time bound, physical-input exclusions, each login
field, changed observations, missing/contradictory environment, wrong opened
seat, and the real configuration/input-opening path remaining inputless.
An actual startup refusal control exercises the non-logind backend refusal
before endpoint creation. These do not qualify PAM registration, real
secondary-seat acquisition or the post-open success path on hardware.
Evidence is in `development-evidence/t312-development-login-01`.

The next launcher still needs authenticated PAM registration outside an
existing daily login, per-PCI confinement with private display/runtime
endpoints, a deadline covering startup and children, and reviewed host rules
with rollback. System-bus method timeouts do not bound connection setup.
The second login check follows libseat's TakeControl; a mismatch releases that
control through teardown. The launcher must bind the genuine host system-bus
socket, since the service-owner answer itself comes from that bus. This mode
has no physical recovery chord. zbus and its blocking executor are confined to
the native-session feature.
Both cards remain on seat0; no host rule, live seat, VT, input or installed
release was changed by this slice. t312 remains open.

The admission slice is qualified at `002c5f148` on `c1d876adc`: the isolated full
gate passed with 7,222 Rust tests, zero failed and 100 ignored; clippy, tool
controls and layout passed. The tree was clean before and after; stdin was
`/dev/null`. Claude accepted the source and startup-module extraction. The
preceding gate remains red: tests and clippy passed, but `run.rs` exceeded the
1,000-line limit. Moving startup preflight into `config/startup.rs` resolved
that layout failure without changing admission order.

### Next: the development login and cleanup owner

The prepared launcher design is in
`development-evidence/t312-development-launcher-01/DESIGN-02.txt`. It requires
a dedicated development account, distinct from the daily desktop's UID. The
root-owned login owner must start outside an existing login, register a bounded
non-VT PAM session, drop credentials before running development code, and own
cleanup through startup failures and the owner's own death. No such service or
account has been installed.

The offscreen runner cannot be reused unchanged: native topology monitoring
needs host udev event delivery and its root sender credentials. The proposed
launcher keeps the host network and user namespaces while isolating mounts,
process IDs, IPC and UTS. Private display/runtime paths, exactly the admitted
GPU nodes, the genuine host system bus, and restrictions on abstract sockets,
signals and network creation need their own controls. A delegated host udev
monitor could permit narrower namespaces later; it does not exist today.

The design must also preserve Sophia's nested protection domains. Blanket
namespace-creation refusal would break those children. Effective host polkit
rules could not be read by the agent; the privileged transaction preflight must
inspect them and establish the dedicated account's restrictions. These are
remaining implementation and qualification obligations, not an activation
recipe. t312 stays open, and physical head-change acceptance still depends on
t306.

### Fourth source slice: bounded login owner and launch boundary

The implementation is in `tools/development_seat/`. It is source for a fixed,
root-owned bundle, with no installation or activation command. Configuration
requires a dedicated locked account, an unoccupied non-VT seat, one expected
PCI device, pinned files and tools, and a Session bound of at most 300 seconds.
The root service starts outside existing logins. Its worker registers PAM and
attests that worker's own login before constructing the private environment.
Only the credential-drop helper and trusted auditor run before Sophia. The
auditor sends its receipt through a pipe closed before development code runs;
writable user artifacts cannot replace that host receipt.

The service declaration starts down and is intended for `sv once`. Its separate
hard timeout covers configuration, PAM, Session and cleanup. Pidfds and checked
parent-death signals keep children in custody. A namespace guard also covers
parent death before namespace init arms its signal. Worker cleanup reaps the
child before closing PAM. The owner can request termination only for the same
fully attested login while its leader remains unreaped; it never selects a
session by UID display or acts on a reused PID after reaping.

The source review found three cleanup/trust gaps and a CPU execution control
found an environment mismatch. Cleanup now blocks further TERM delivery until
exit; PAM also protects close/end when handling an earlier failed open. If the
sandbox cannot be reaped, the worker records failure and skips explicit PAM
close. Worker exit then triggers parent-death cleanup and closes the login
lifetime descriptor. Their completion order is not guaranteed, especially for
an uninterruptible kernel task; that path is not successful ordered teardown.

Each Python entry now starts through a standard-library-only bootstrap. It
checks root ownership, the exact bundle directory contents and source hashes
before importing bundle code. Startup disables site hooks and redirects
bytecode lookup to `/dev/null`, which cannot contain a cache. The interpreter,
standard library and initial bootstrap source remain deployment trust roots.
The auditor's fixed environment includes the `PWD` that bubblewrap sets. A real
CPU namespace control failed without that entry and passes with it.

The mount namespace exposes exactly the admitted primary and render nodes,
private home/runtime/output paths, read-only system metadata and the genuine
system bus socket. Host user and network namespaces remain for udev delivery.
After credential and capability removal, Landlock ABI 6 scopes abstract UNIX
sockets and signals. The inherited syscall filter allows UNIX and uevent
sockets while refusing other socket families/protocols, io_uring and setns.
Namespace creation remains available for Sophia's own protection domains.

That inherited filter also covers nested bubblewrap setup. Stock bubblewrap
tries NETLINK_ROUTE to configure private loopback, which the filter refuses.
The private 0.13.0 build omits only that setup call, retaining CLONE_NEWNET and
its failure checks; loopback stays down. It is never installed on PATH or made
setuid. The offline builder verifies the release archive and applies one hunk.
It removes build-directory RUNPATH before qualification. Configured privileged
tools and their explicit ELF loader paths must be root-owned and not writable
by other users. This does not freeze every transitive system-library load.
An alternative is to install the filter after stock bubblewrap setup, but that
would leave that setup outside the inherited restriction being qualified here.

CPU controls exercise private fixture PAM modules, every login field, file and
policy refusals, worker cleanup across failed admission and deadlines, real
signal/socket restrictions, nested namespaces and parent-death races. They
include a failed wait that retains its pidfd for final cleanup. Sophia's real
`ProcessSupervisor` also launched and reaped an ordinary protected child under
these restrictions with the private bubblewrap. Evidence and retained failed
build attempts are in `development-evidence/t312-development-launcher-02`.
These controls do not open the installed PAM stack or a GPU.

Activation still requires a reviewed root-owned bundle and host transaction.
The real credential transition, authenticated PAM registration and cgroup
inheritance, effective host authorization policy, root-owner death after
registration, processed udev delivery, and successful independent seat control
remain unqualified. File hashes alone do not prove that polkit loaded its rules;
the dedicated account needs negative authorization controls before a GPU run.
The service deliberately exposes the genuine system bus. Its dedicated UID,
restricted policy and private mounts are separate parts of that boundary.
Polkit does not govern every system-bus service: qualification must enumerate
reachable services and exercise their own authorization as the dedicated UID.
Read-only metadata mounts also do not prevent pathname-socket connections.
The current host permissions restrict those endpoints, but qualification must
check accessible sockets and FIFOs under `/run/udev` and `/run/systemd`; the
mounted metadata includes other sessions' readable state. The host dynamic
linker cache and transitive libraries remain trusted rather than fully pinned.
The full owner-loop and auditor success paths need that privileged fixture;
the CPU worker tests replace authority and mount/credential operations.

The successor controls in `development-evidence/t312-development-launcher-03`
cover unreaped children, TERM during owner/worker/PAM cleanup, stale unchecked
bytecode, changed or extra bundle files, and real bubblewrap environment
construction. They do not qualify a live PAM login or a physical seat.

No account, PAM/runit/polkit/udev file, seat assignment, installed release, input
device or live display has changed. t312 remains open until simultaneous
presentation and forced-exit recovery meet its physical exit.

## Evidence

- `igpu-development-01/FINDINGS-01.txt` and `READ-ONLY-01.json`: source survey,
  current desktop descriptors and seat identity. No device changes.
- [Login refusal and profile repair](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#subsequent-hardware-change-and-login-failure).
- [Validation contract](../../validation.md): device tests remain separate
  from CPU gates and do not establish native presentation timing.

Task status lives in [todo.md](../../../todo.md).
