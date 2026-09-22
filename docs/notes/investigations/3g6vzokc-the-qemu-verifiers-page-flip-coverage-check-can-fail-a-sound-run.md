---
id: 3g6vzokc
date: 2026-09-22
kind: investigation
status: investigating
tags: [investigation, tooling, qemu]
---
# The QEMU verifier's page-flip coverage check can fail a sound run

## Question

`tools/verify_qemu_session_evidence.sh` fails a two-xterm QEMU run that the
harness itself completed cleanly. Is the run unsound, or is the check?

## Evidence

Found under t119 on `12cdc647` while proving the QEMU gate catches a no-WM
routing regression. Two runs of **byte-identical** healthy code
(`git diff` on `wm/layout.rs` empty between them), same image, same host,
same `SOPHIA_QEMU_TWO_XTERM=1 tools/qemu_session_harness.sh`:

| run | harness | `sophia_live_page_flip_clock … source=kernel_monotonic` | verifier |
| --- | --- | --- | --- |
| first | exit 0, `status=complete` | `timestamps=1 fallbacks=0 pending=0` | pass |
| second | exit 0, `status=complete` | `timestamps=0 fallbacks=0 pending=0` | **fail** |

The check at `verify_qemu_session_evidence.sh:178-181` requires the clock
line to match `timestamps=([1-9][0-9]*) fallbacks=0 pending=0`, so a run with
zero kernel page-flip timestamps is refused as lacking "complete kernel
page-flip timestamp coverage". Both runs are retained under
`.artifacts/t119-qemu-gate/` as `green-unmutated.log` and `green-restored.log`.

The count is marginal, one against zero, on a 300-tick software-GPU guest.
Nothing about the second run was otherwise unhealthy: the harness completed,
`runtime_max_surfaces=2`, and the persistent live-session verifier passed. Only
this assertion turned it red.

## Finding and resolution

Not yet diagnosed. Two readings to separate:

- The assertion is too strict for the software-GPU guest, where a kernel
  page-flip timestamp within the window is a matter of timing rather than
  correctness, and the check should either accept `timestamps=0` when
  `fallbacks=0 pending=0`, or require a longer run before asserting it.
- The second run genuinely produced no kernel flip timestamp when it should
  have, which would be a presentation defect the check correctly caught.

The first is the parsimonious reading given identical code and a one-versus-zero
margin, but it has not been shown, and a gate check should not be loosened on
parsimony alone. Re-run several times on unchanged code and read the
distribution of `timestamps=` before deciding which reading holds.

## Validation and remaining work

Open as t153 in [todo.md](../../../todo.md). This did not affect t119's
result: the gate's red/green discrimination for that task is the harness exit
and the pointer proof, both of which held across all runs; this assertion is
downstream and independent of routing.

## Connections

- [A no-WM session never routes its window to an output](z2pghbxr-a-no-wm-session-never-routes-its-window-to-an-output.md) --
  owns t119, where this was found.
