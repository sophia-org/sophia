# Native launcher smoke

`lom-test launcher` selects independent Lom (bar, explicit GPU grant) and
Bemenu (application launcher, GPU denied) in the same bounded tty4 session.
Plain `lom-test` retains the previous single-shell 40-action panel workload.
This is a separately attended hardware run, never part of the offline gate.

The command requires a clean signed Sophia, Lom, Hagia and Bemenu checkout. It
builds Bemenu from an exact signed commit archive in the private evidence folder,
not a possibly stale in-tree executable; records all source/binary/config hashes;
and checks the hashes again before and after the session. Nothing is installed.
`SOPHIA_BEMENU_SOURCE` defaults to `$HOME/src/bemenu`. Existing Lom/Hagia source,
target and profile overrides retain their previous meanings.

The generated profile selects two roles with distinct process/config/GPU grants,
content and input enabled, 24-pixel panel allowance and no startup applications.
It neither replaces nor adds WM shortcuts or policy. The real profile composer
refuses a selected WM profile without an application-launcher key binding. The
operator's current profile uses **Super+Space**. The probe catalog explicitly
includes `/usr/bin/xterm` as `terminal` and `/usr/share/applications` desktop
entries under `trusted-host` launch policy. Application execution still requires
an admitted choice; catalog selection does not run applications. Use an explicit
`SOPHIA_LOM_CORE_CONFIG` only if it supplies `native-launcher-gate` deliberately.
Missing runtime dependencies or rejected catalog sources are failures, not
permission to run a legacy launcher or change the selected catalog silently.

After the separately armed protected GPU preflight, the command runs for 90
seconds (110-second recovery watchdog). Move the pointer to each monitor and
open the launcher, type a query, dismiss, reopen and check that the query resets.
Watch the other bar and clock continue. On each monitor obtain at least two
presented launcher generations. Finally select the catalog entry `terminal` and
press Enter once, close it, dismiss the menu and await normal automatic exit.
Keep observations of placement, visible pixels, focus restoration, mouse
outside-dismissal, reset and unaffected bars. A process-start record proves
supervised spawn, not that its window was visible or useful.

The host-only verifier requires distinct stable role/slot/connection/content
identities, revision 6 bar and revision 7 native launcher, exact per-role GPU
policy, one WM configuration and a nonempty catalog, two presented generations
on each of the same two outputs **for each grant**, exactly one started process,
and final actual aggregate quiescence. The wrapper separately requires exit 0,
TTY/keyd recovery and unchanged inputs. Failures, restarts, malformed/missing
fields, wrong/aliased grants and missing outcomes refuse. Host and client logs
are never merged as authority. A transcript pass does not establish visual
acceptance, exact kernel timing, the panel latency workload or all of t108.

Offline controls run the real generator/verifier with supplied command effects;
real Session parsing and WM-binding preparation have separate Rust controls.
They open no real display/VT/device. `tools/check_lom_gpu_content_proof_verifiers.sh`
includes this test suite, and must itself execute inside the device-hidden
canonical wrapper on this host. The new mode still needs its final frozen
canonical gate before physical readiness is declared.
