---
id: c9di7qpg
date: 2026-09-18
kind: investigation
status: investigating
tags: [investigation, config, session, shell]
---
# Independent shell providers were omitted from desktop launch reload validation

## Trigger and source finding

The daily Hagia login on installed `d444eba26aad6d7a4e56ac0a044bc9a0250b230d`
uses independent Lom and Bemenu components. After changing the terminal from
xterm to Kitty, Ctrl+Alt+R produced `reload_declined`; Super+Enter continued
using xterm. The retained session is
`00000001789753525467-12a165c8-3b63-4070-a468-38e473431e52`.
Structured logs omit the refusal detail, so the source diagnosis is independently
established by the deterministic control, not an invented live error message.

Startup validates shortcuts using its resolved `live_shell_enabled` capability.
`PreparedDesktopLaunch::prepare` instead checked only `shell_process.is_some()`.
Independent providers intentionally have no single shell process. Launcher and
switcher bindings therefore prevented even a terminal-only reload. Disk config
validation alone did not exercise this active-session boundary.

## Repair

Retain the startup-resolved shell shortcut capability in Session configuration
and reuse it in launch reload. There is no compatibility fallback or client-name
exception. Newly requested providers remain deferred and cannot authorize
shortcuts before startup admission. Launch publication, generation changes and
old-command retention keep the existing atomic transaction.

## Evidence and limits

`tests/support/component_launch_reload.rs` drives the actual desktop reload
handler and Super+Return router. Independent bar/launcher and Narthex cases both
replace xterm with Kitty, preserve the provider/WM setup, retire the old action
identity, and leave a held old command unchanged. A providerless session rejects
new shell-dependent shortcuts without changing its applications or generation.
The fixture supplies accepted WM configuration; no child, device or native
session is launched.

Artifacts: `.artifacts/component-config-reload/`. The old process-only check
compiles and makes the component regression fail (`Declined` versus `Applied`).
The unconditional-capability mutation compiles and makes the providerless
negative fail. Its final control uses a switcher without a launcher binding so
absence of an application catalog cannot mask the provider check. Initial
fixture-schema failures, a zero-test selector, and the earlier non-discriminating
mutation attempts remain separate logs, not passing mutation evidence.

Final device-hidden Session library run: 488 passed, zero failures, 17 ignored.
Strict Session library/test Clippy, workspace formatting and layout pass. The
first layout attempt lacked `rg` in the private namespace and is not layout
evidence; `format-layout-with-rg.log` records the corrected environment.

Installed physical reload acceptance remains pending: source changes cannot
replace code in the running compositor. This repair does not add automatic
watching of desktop profiles or broaden which settings can reload. The previous
[t076 acceptance](../plans/1agxbuuf-application-commands-in-the-desktop-profile.md)
covered the earlier provider configuration, not this component regression.
The daily-login integration remains within
[t101](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t101).

## Connections

[Configuration contract](../../configuration.md) owns reload semantics.
[Core launch reload](dftv5tde-core-configuration-reload-drops-desktop-launch-selections.md)
explains the related but distinct requirement to retain launch selections.
