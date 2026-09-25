---
id: sp5qem3i
date: 2026-09-25
kind: investigation
status: investigating
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
a rejected reload retains them with the previous profile. No provider is
enabled, no launch grant is added, and no configured shortcut is changed.

This follows the [WM contract](../../sophia-wm-api.md) and preserves
[explicit launcher admission](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md)
and [scoped component grants](../concepts/k2d9l42p-native-shell-components-compose-through-explicit-scoped-grants.md).

## Validation

Targeted session tests cover acceptance of the compiled desktop with no
applications, retained close-window shortcuts, absent browser/terminal
shortcuts, catalog omission, replacement, explicit unavailable bindings and
unknown-slot rejection. Paired headless Hagia acceptance and the full repository
gate are pending. No physical-session claim or live installation is involved.
