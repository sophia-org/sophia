---
id: sp5qem3i
date: 2026-09-25
kind: investigation
status: closed
tags: [session, policy, validation]
---
# The policy catalog must admit only available session actions

## Trigger and cause

t159 follows the [unconfigured Hagia observation](4m65c17q-primary-selection-does-not-reach-another-client-from-xterm.md).
On Sophia `787e6b46` and Hagia `ad3a738d`, Hagia's `installConfiguration`
offers its entire action vocabulary before receiving its first snapshot. This
includes browser and launcher slots even when the profile binds neither.
Sophia rejected the whole offer when either slot lacked an operation grant.
Separately, startup reported dropping unavailable compiled-default shortcuts,
but the WM resolver still tried to resolve those bindings.

## Repair

Session distinguishes the offered catalog from the admitted catalog. It omits
unavailable standard slots 1–7 before resolving shortcuts and publishing the
accepted catalog. Unknown slots still reject the configuration. Explicit
bindings still require an admitted operation or a Session-owned application
command; missing bindings are errors, not silently disabled user settings.

The resolver carries startup's existing compiled-default omissions through
initial configuration and policy replacement without changing the prepared
source or digest. A successful explicit profile reload clears those omissions;
a rejected reload retains them with the previous profile. Core application
reloads recompute the omissions together with the replacement command registry.
No provider is
enabled, no launch grant is added, and no configured shortcut is changed.

This follows the [WM contract](../../sophia-wm-api.md) and preserves
[explicit launcher admission](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md)
and [scoped component grants](../concepts/k2d9l42p-native-shell-components-compose-through-explicit-scoped-grants.md).

## Validation

Targeted session tests cover acceptance of the compiled desktop with no
applications, retained close-window shortcuts, absent browser/terminal
shortcuts, catalog omission, replacement, explicit unavailable bindings and
unknown-slot rejection. The 19 targeted reload tests pass, including adding a
terminal command by core reload while the browser remains unavailable.

Signed candidate `33355234dc660c45fadd8322712f29a3f139d135`, based on `1a7eb4df`,
passed the full `cargo xtask check` in a namespace with private devices, runtime,
X sockets and writable Git metadata. Workspace tests, strict Clippy, fmt, layout,
wire checks and verifier regressions passed. The native-session library ran
600 tests: 582 passed, 18 explicitly ignored, none filtered. The ignored hardware
and separate acceptance tests retain their own gates.

The candidate CLI rebuilt from that source also passed a real Hagia headless
run with `--no-config --no-input --session-mode=normal`, a fresh XDG config
directory, private display `:91`, a five-second bound and one xterm startup
application running `sleep 3`. Hagia was the signed t080 binary from
`ad3a738d7aee2af4ab31f7e7139d1ceecdc7dcbe`. Narthex was supplied beside it as the
default shell, with its binary retained by digest rather than a source claim.
The run committed four WM transactions, observed one application surface,
reported zero WM restarts and no degradation, and cleaned up its processes,
namespace and Xauthority. The compiled terminal and browser shortcuts were
reported as omitted; physical input and native presentation were disabled.

Checksummed logs, binary identities and the independent peer binaries are at
`~/.local/state/sophia/development-evidence/t159-33355234`. Initial headless
setup attempts failed before acceptance because the read-only namespace lacked
a bind destination, then because Hagia's inferred sibling Narthex was absent;
the accepted run explicitly supplies both peers. No physical-session claim or
live installation is involved.
