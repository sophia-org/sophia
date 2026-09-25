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

**Session B.** The tab matrix remains prepared observations with no approved
runnable capture path. The native verifier requires:
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

## Built-binary identity (`5df5a380`)

Review (pN) found two routes by which the Hagia proofs could bind one Sophia
binary and run another:

- **Build output.** Both wrappers built with an inherited
  `CARGO_TARGET_DIR`, yet hashed and ran `ROOT_DIR/target/release/sophia`. A
  stale executable there was bound as the current commit's build.
- **Native hand-off.** The native gate hashed that path but handed its runner
  an inherited `SOPHIA_BIN`, which `start_sophia_tty3.sh` and
  `run_sophia_session.sh` execute.

A related case: an inherited `CARGO_BUILD_TARGET` moves the output under
`target/<triple>/`.

The repair:

- **Build target.** Both wrappers build with `--target-dir "$ROOT_DIR/target"`.
- **Cross target.** Both refuse `CARGO_BUILD_TARGET` before any build. No
  cross-compilation mode is added.
- **Native hand-off.** The native gate hands its runner and archive the
  hashed path explicitly.
- **Policy gate.** Its launcher already runs the literal hashed path and
  never reads `SOPHIA_BIN`. A documentation control records that, and it
  needed no change.

| Commit | Content |
| --- | --- |
| `622d614a` | Red: an inherited binary, target directory or stale executable is run or bound |
| `11713d4b` | Red: `CARGO_BUILD_TARGET` is not refused |
| `5df5a380` | Fix |

The fix is based on `9fdb7ff0`, on branch `gate/t018-sophia-bin-pin`.

Controls: the cargo stub honours `--target-dir` and then
`CARGO_TARGET_DIR`, and the runner stub executes the `SOPHIA_BIN` it is given.
The results:

- The device-hidden suite passes 15 of 15.
- Reverting only the three sources fails the native, policy and cross-target
  controls.
- Dropping only the native hand-off pin fails the native control.
- With the real signed Hagia `97ed593` and Narthex `7f51175` roots, the
  native and physical matchers and the profile preflight pass.

Logs are retained under `.artifacts/t018-pin/` in the repair worktree. No
API or session code changed.
