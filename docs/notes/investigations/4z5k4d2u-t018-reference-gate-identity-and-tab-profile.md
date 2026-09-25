---
id: 4z5k4d2u
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, hagia, narthex, tabs, physical-gate, t018]
---
# t018 reference gate: identity hand-off, worktrees and the tab profile

The [September 25 audit](4l5bntke-critical-desktop-exits-require-separate-device-tab-and-workload-evidence.md#tabs-reference-shell-and-component-desktop)
found that the physical Hagia/Narthex gate launcher was not yet an approved
entrypoint for the [t018 matrix](../plans/queue-06-4-exercise-real-development-workflows.md#t018).
This note records the preparation that followed. No physical, TTY, GUI or
live session was run. t018 remains open.

## Confirmed defects

Both gate chains refused every armed run at identity binding, before any DRM
takeover:

- **Policy gate.** `tools/hagia_policy_physical_gate.sh` tested the undefined
  `recorded_hagia_shell_sha256` under `set -u`, so bash aborted with
  "unbound variable". It also never format-checked the Narthex commit.
- **Native wrapper.** `tools/run_current_hagia_native_gate_tty4.sh` exported
  the Narthex binary as `SOPHIA_HAGIA_NATIVE_HAGIA_SHELL_SHA256`. The gate reads
  `SOPHIA_HAGIA_NATIVE_NARTHEX_SHA256` and `..._NARTHEX_COMMIT`, and the
  wrapper exported neither, so the gate always refused with its "bind all
  signed commits" message.
- **Native gate.** It never bound Narthex's clean state, HEAD or signature,
  although it recorded Narthex's commit.
- **Worktrees.** Checkout checks tested for a `.git` directory. A linked
  worktree has a `.git` file, so every worktree was refused.

## Repair (`77ebf01f`)

- **Hand-off.** The policy gate checks `recorded_narthex_sha256` and the
  Narthex commit format. The native wrapper exports the Narthex binary digest,
  commit and root. The native gate binds Narthex's clean state, HEAD and
  signature, and the wrapper re-checks Narthex after its build.
- **Checkout roots.** The eleven checkout sites of the two Hagia gate and
  archive chains share one predicate, `proof_checkout_root` in
  `tools/lib/proof_checkout.sh`. Git must report the path's real location as
  the repository top level, so a worktree is accepted and a subdirectory of
  another repository is refused. The seven other `.git` sites in `tools/`
  are unchanged findings.
- **Reference profile.** `SOPHIA_HAGIA_NATIVE_PROFILE` may name a profile. It
  is chosen before any build and must be an absolute, tracked, unmodified
  regular file of the Sophia or Hagia checkout. A symlink is refused whatever
  it targets (`787e0f95`): review showed that a tracked link to an external
  file passed while the target's bytes changed, because git records the link
  and not those bytes. The red control is `fab01fa5`. The default profile is unchanged.
  `tools/fixtures/t018_tab_reference.kdl` holds only Hagia policy:
  - the native-workflow keys;
  - scroller at start;
  - layout selection for `frame-tree`, `notion` and `i3`;
  - Hagia's documented frame, tab and split-tree actions;
  - fullscreen and floating.

  Narthex is still selected by the gate's `--shell-process`. Installed Hagia
  and Sophia `config check` both accepted the fixture (digest `a841b87e…`).
  The wrapper re-runs both checks with the exact candidate builds.

## Controls

These are in `tools/tests/physical_gate_identity_test.py`, which `cargo xtask
check` runs. They use real git repositories. Signing, the tty4 check, the Nim
and Cargo builds, and everything from the session start onward are stubbed,
and no device is opened.

| Control | Proves |
| --- | --- |
| Predicate cases | A repository root and a linked worktree are accepted. A subdirectory, a symlink into one, a plain directory and a missing path are refused. The profile must be absolute, tracked, unmodified, not a symlink, and inside a named root. |
| Native dry run (pseudo-terminal) | The wrapper hands every identity to the gate for a worktree Hagia and the reference profile, and reaches the stub session start. |
| Profile refusals | Relative, ignored-untracked and outside profiles refuse before any build. |
| Armed gate refusals | Both gates reach the session start when bound. A mismatched Sophia, Hagia or Narthex digest, or a dirty or unsigned Narthex, refuses first with no "unbound variable". |

Against master's three gate scripts, the native dry-run cases fail at the
worktree check. Reintroducing each defect separately fails its intended
control:
- the undefined policy name;
- the stale native export;
- a missing Narthex signature check;
- no root equality;
- untracked profiles accepted.

The first runs of both Hagia verifier-matcher checks failed, and a first
reading blamed signature verification in the device-hidden sandbox. That was
wrong. Both runs, including the comparison run, checked HEAD `1fbaf4cb`, an
unsigned documentation commit, so the matchers correctly refused an unsigned
source. On signed `1a9ab46c`, in the same wrapper with frozen signed Hagia
`97ed593` and Narthex `7f51175` roots, both matchers pass. The first
failing logs are retained with the passing ones.

## Sessions

**Session A.** The unchanged native workflow can run through the repaired
chain with the reference profile:
- the proof text;
- three terminals;
- focus-next;
- close;
- logout.

Its verifier keeps its exact counts. A PASS proves that workflow and the
bound identities only. It does not accept t018.

**Session B.** The tab matrix is prepared as operator observations. Its
reference capture path (`ff711f4e`, branch `gate/t018-reference-capture`, under
review) is described below. The native verifier requires:
- exactly three terminal launches;
- one close and one logout;
- one Hagia Shell admission at connection epoch 1.

The matrix needs more windows and a Narthex restart, so the same gate would
fail by count. That failure is not a reference result. A capture-only path is
a separate proposal.

## Operator command for Session A (prepared, not run)

On tty4, with Sophia at the signed candidate and Hagia and Narthex at clean,
signed HEADs:

```
SOPHIA_HAGIA_ROOT=$HOME/dev/hagia SOPHIA_NARTHEX_ROOT=$HOME/dev/narthex \
SOPHIA_HAGIA_NATIVE_PROFILE=$HOME/dev/sophia/tools/fixtures/t018_tab_reference.kdl \
  $HOME/dev/sophia/tools/run_current_hagia_native_gate_tty4.sh
```

## Reference capture (`ff711f4e`, under review)

`SOPHIA_HAGIA_NATIVE_CAPTURE=reference` runs Session B through the same chain
and owners. It retains a `hagia_reference_capture` record whose line reads
`native_acceptance=false tab_observations=unverified`.

What a capture claims:
- the bound identity and profile;
- an observed exit 0;
- one validated TTY recovery record from this invocation's own rotated runner log;
- the retained evidence.

What it does not claim: any native workflow result, clean retirement, or tab
observation. The native verifier never runs on a capture, and the guide
automates no matrix action.

Validation is shared by the gate, the archive and re-verification. Each
required family must hold exactly one record of the exact form.

Captures are stored under `sophia/reference/hagia-tab-captures`, never the
promotion directory. Every existing reader keeps the native default and refuses
a capture. Only an explicit `--expected-kind=reference` reads one.

The runner rotates its session and recovery logs on every run, so a capture
binds them by requiring exactly one rotation since the session started. An
intervening run refuses, and prior files are kept. A reference run root that
is, contains or sits inside promotion storage is refused before the session.

Operator observations may be retained with their digest. At most 256 KiB
plus one byte is ever read, the retained copy must fit 256 KiB, and they
stay unverified.

Shell recovery is an explicit operator step. The guide prints the lookup
of this capture's newest shell record, `status=ready` for the first peer
and `status=reconnected` for each replacement, and names only a positive
numeric `peer_pid` from that record. The operator confirms it with `ps`
and then signals it. No process is matched by name, and no restart API is
added. A ready-only lookup kept naming the first peer after a restart (red
7c817ef9).

The capture carries the built-binary identity repair (cherry-picked from
`gate/t018-sophia-bin-pin`). The session and archive run the Sophia binary
the wrapper built and hashed.

Prepared command, not run: on tty4, with Sophia at the signed candidate and
Hagia and Narthex at clean, signed HEADs:

```
SOPHIA_HAGIA_ROOT=\$HOME/dev/hagia SOPHIA_NARTHEX_ROOT=\$HOME/dev/narthex \\
SOPHIA_HAGIA_NATIVE_CAPTURE=reference \\
SOPHIA_HAGIA_NATIVE_PROFILE=\$HOME/dev/sophia/tools/fixtures/t018_tab_reference.kdl \\
SOPHIA_HAGIA_REFERENCE_OBSERVATIONS=\$HOME/t018-observations.txt \\
  \$HOME/dev/sophia/tools/run_current_hagia_native_gate_tty4.sh
```

The observations file is written by the operator during the session, from the
guide's terminal or another one, and is copied at archive time.
