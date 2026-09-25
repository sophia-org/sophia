---
id: dftv5tde
date: 2026-09-10
kind: investigation
status: closed
tags: [investigation, config, session]
---
# Core configuration reload drops desktop launch selections

## Trigger and boundary

On installed `fb8fe8be7960bbe0939f828dfa20d7a98eba00b9`, niltempus asked to remove
the explicit Chromium launch adapter from the browser registration and launch
his Go `brave-origin` launcher directly. The replacement core configuration
validated, but it was not written to the live watched file after the reload
path was found to discard the selected launch identities.

The current architecture gives Hagia named session actions. Sophia resolves
those actions to executable registrations; changing this division was not
requested by the configuration edit. The adapter existed in the user's core
configuration, not in Hagia's shortcut binding. Its direct replacement needs no
arguments and no device override. The desktop entry already launches normally.

## Source finding

Startup combines `applications_from_core` with retained
`SessionApplicationOverrides::prepare` and the prepared desktop session profile.
This includes command-line applications and arguments, profile-selected browser
and terminal identities, and startup choices.

`service_core_config_reload` instead calls `CoreConfigState::reload` directly,
then assigns `applications_from_core` alone to the active launch table. In the
observed configuration, the core declares application IDs 5 and 4; the terminal
comes from command-line additions and the browser identity from the desktop
profile. Reload therefore drops both selections. It also publishes the core
snapshot before preparing the dependent launch table, and a later `?` may leave
partially changed state while terminating the owner.

This is source-confirmed, not a live destructive reproduction. The watched file
remains unchanged. The Linux watcher observes both ordinary writes and atomic
replacement, and the installed owner has no supported reload-pause operation.

## Repair and acceptance

Prepare a cloned bounded core state, resolve the candidate applications with
retained launch overrides and the active session profile, then publish the state
and launch table together. A prepared startup profile is usable only when no
active profile exists. An uncommitted replacement cannot override active profile
selections. Invalid merged references must preserve active applications,
generation, digest and any previous pending-restart candidate, and produce an
ordinary rejected reload rather than terminate the session.

The helper runs only for configuration reloads; it adds no rendering or per-input
work. Tests must exercise the production helper, including executable changes
without losing terminal/browser selection, invalid references, startup choices,
CLI precedence and pending-restart retention. Installed acceptance requires the
repaired owner before writing the validated direct-launch configuration.

Candidate configuration and backup are retained with mode 0600 under
`.artifacts/t071-live-fb8fe8be/config-normal.kdl` and `config-before.kdl`.

## Implemented repair and deterministic evidence

The Session-owned `reload_core_config` helper now prepares the cloned core state
and merged launch table before publishing either. The owner-loop reload handler
uses that helper and records a preparation refusal without terminating the
session. The retained CLI overlay is reused unchanged; no command parsing or
application recognition was added to Engine or Hagia.

Six external regressions pass through the production helper. They cover a direct
browser command retaining the CLI terminal and profile selections, active-profile
precedence over a staged replacement, explicit CLI precedence, rejection with an
existing pending-restart candidate followed by valid recovery, unchanged reload,
and refusal without active or prepared session authority. Independent review
found no blocker. These tests do not exercise file-watcher delivery or a running
owner's reload loop.

`SOPHIA_FIRST_FRAME_REQUIRE_AUX=1 cargo xtask check` passed with no compiler or
clippy warnings. Workspace tests include all six regressions. Promoted archive
checks passed (Hagia 5/5, mirror groups 9/9, direct scanout 6/6), as did host
buffer-age pixel equivalence and GLX/EGL first-frame and pixmap-export pixels.
Logs are retained in `.artifacts/t075-core-reload/`; its checkpoint records the
signed source commit and release binary identity above parent
`21ed038b8fbd108d51e3e4acae35ed8fc634de55`.

The installed owner remains `fb8fe8be7960bbe0939f828dfa20d7a98eba00b9`. The watched
user configuration was rechecked against the saved original and is unchanged.
The remaining gate is to run the repaired owner, apply the validated direct
registration, and verify the reload keeps terminal and browser actions usable.
That acceptance is separate from the deterministic checks above and does not
establish unmodified-browser GPU acceptance for t069.

Open work is tracked in [todo.md](../../../todo.md); the accepted GPU feedback
work remains separate in the [installed signaling record](../milestones/urqkkdzp-exact-copied-present-evidence-gates-reallocation-advice.md#installed-acceptance-on-2026-09-10).


## Desktop command integration

The approved [application-command plan](../plans/1agxbuuf-application-commands-in-the-desktop-profile.md)
now includes this reload repair in its command registry transaction. The pending
user migration defines the direct Brave command in the desktop profile rather
than replacing the core registration. Core reload preserves that applied launch
slice and serializes with policy replacement. Its external regressions retain
the active/pending rollback checks and add removal of already-executed startup
references. Installed-owner acceptance remains required; the earlier core-only
candidate is superseded by the paired Session/Hagia candidate in that plan.


## Installed launch-preservation observation on 2026-09-10

The paired installed owner `171345049bf620a40b24c48d738329b1f63decaf` and Hagia
`3459a85d5dd7a1943efcf526fa5d4ec297d246cd` ran the current acceptance session.
The direct Brave override is in the active Sophia desktop profile. A bare
`brave-origin` command failed to spawn because the owner's PATH omits
`~/.local/bin`; using `/home/niltempus/.local/bin/brave-origin` fixed launch.
It retains literal `--`, `--ozone-platform=x11` and
`--render-node-override=/dev/dri/renderD128` arguments.

After the corrected profile was applied, removing only a core test comment
produced watcher `Applied` generation 5, digest
`26351b1750d42f7b5fa12bd2abadacaefc137333627c5c5bc89049fa91baabf6`.
The operator then confirmed Super+Enter and Super+B both worked without a
further desktop reload. Evidence is in `.artifacts/t076-desktop-acceptance/`;
the [session investigation](v4geoq2j-policy-reload-compares-independent-configuration-generations.md)
records exact binaries and the separate policy-replacement failure. Cleanup is complete: the operator applied desktop generation 6 and confirmed
both shortcuts again, then the watcher applied generation 6 with the original
core digest `b74c43bad36cb75ce7a173f499f05f8e9921174116bcdcece99fe266d57c119c`.
`cleanup-complete.json` records unchanged owner, shell and panel identities.
The t075 launch-preservation gate is satisfied on this installed pair. This is launch-preservation
evidence, not GPU acceptance.
