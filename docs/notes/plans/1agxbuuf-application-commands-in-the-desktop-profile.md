---
id: 1agxbuuf
date: 2026-09-10
kind: plan
tags: [plan, session, configuration]
---
# Application commands in the desktop profile

## Scope and exit

niltempus approved implementation on 2026-09-10. Task t076 makes the selected
desktop profile the ordinary place to define application commands and their
shortcuts. Both named applications and inline commands are supported. An
explicit `use-core` reference imports an advanced Sophia registration as a
whole; equal names do not merge fields. Existing role bindings remain valid.

Session resolves and launches commands. Hagia receives only its policy
fragment, and Engine matches opaque actions. Neither gains executable names,
argument vectors, application recognition, or process-launch authority.
Commands use literal argv and PATH lookup; shell evaluation requires an
explicit shell command. Sophia and Hagia check, print and migrate the same
grammar. Ordinary launcher registrations are defaults; explicit CLI overrides
retain precedence.

## Implementation and acceptance

Trusted preparation lowers inline commands into a bounded Session registry.
Staged Shortcut records contain references and the Policy fragment contains no
commands. Session reserves a bounded action range and refuses forged or stale
activations. A launch accepted into its queue retains the complete immutable
specification across subsequent reloads. Successful non-window commands do not
block later launches; optional first-surface placement remains separately bounded.

Registry, role selections and shortcut routing publish together at an idle key
ledger. Command-only reload reuses the active policy configuration without
restarting Hagia. Profile/launch identity stays separate from the policy
connection and configuration identity. A policy edit stages immutable fragment
directories and retains the prior bundle until the replacement configuration
is accepted. Failure restores the previous bundle and paths. Core and profile
reloads serialize, with bounded pending work and no replay of startup commands.
Initial fragments remain leased by unchanged shell/broker authorities; storage
is bounded to that initial generation, one current policy generation and one
pending replacement. The applied launch slice carries its own source snapshot
without pretending that deferred Session settings have activated.
Other authority edits retain their existing mechanisms and report deferrals.

Acceptance requires external tests for grammar, authority separation, literal
argv/environment, missing core references, stale action/slot reuse, immutable
queued launches, no-window commands, held shortcuts, core/profile reload races,
failed preparation/replacement rollback, and unchanged output topology on
command-only reload. Run the Sophia full gate and Hagia verification, record
signed candidate identities, and keep deterministic checks distinct from live
acceptance. No native owner restart or install is part of source validation.

After the compatible owner is installed, migrate the user's browser definition
into the desktop profile and verify named, inline, advanced-reference and
reload behavior. Preserve the active user config until then. This acceptance
also exercises the launch-selection regression tracked by
[t075](../investigations/dftv5tde-core-configuration-reload-drops-desktop-launch-selections.md).

## Boundaries

This task does not change a browser, add application allowlists or adapters to
Engine, or expand reload into a general seven-authority transaction. Its
normative contracts are [configuration](../../configuration.md),
[desktop composition](../../desktop-composition.md), and
[architecture](../../architecture.md).

## Implemented behavior

The shared profile loader validates named `exec` and `use-core` declarations and
lowers inline shortcut commands into Session records. Effective output renders
back into accepted source syntax. The same limits and grammar are implemented
in Hagia's standalone configuration tooling; its running policy client still
receives only the Policy fragment.

Session captures a complete command when accepting a launch. Normal role
shortcuts use the same registry as named commands, while explicit application
proofs retain their first-surface evidence. Launcher registrations supply bare
executable defaults; they no longer add proof titles or arguments to an ordinary
Hagia login.

Reload preparation holds the old router and command registry until the key
ledger is idle. Policy replacement uses immutable paths and rolls back the
launch specification together with its fragment ownership. The initial
fragment generation remains alive for unchanged shell and broker processes.
Core reload uses the applied launch slice and cannot overlap a policy
replacement. Previously executed startup commands remain child bookkeeping:
removing their declarations does not reject new commands or replay login work.
An active output transaction or its cancellation retains a pending output
reload request until it can be admitted.

External regressions exercise source/staged grammar, actual child argv and
environment, queue identity, command-only publication, held keys, replacement
failure and timeout, stale core state, retained fragments, startup removal, and
output admission. The policy-replacement harness uses a passive supervisor;
it drives production staging and settlement without starting a live owner.

The inactive migration candidate is
`.artifacts/t076-application-commands/desktop.kdl`. It defines literal `kitty`
and `brave-origin` commands in the desktop profile and preserves the rest of
the selected configuration. Both active user files remain unchanged pending a
compatible installed Session and Hagia pair.

## Deterministic verification

`SOPHIA_FIRST_FRAME_REQUIRE_AUX=1 cargo xtask check` passed: 3,034 Rust tests
across 260 result groups, zero failures and 29 intentional ignores. The 21
reload/registry regressions passed separately. Launcher argument checks passed
all seven cases. The full gate revalidated Hagia archives 5/5, mirror-group
archives 9/9 and direct-scanout archives 6/6, and proved buffer-age equivalence
plus GLX/EGL first-frame and pixmap-export pixels on this host. These isolated
render probes do not constitute installed-session shortcut acceptance.

An initial run caught a malformed KDL separator in a new test fixture; correcting
the fixture produced the passing results above. Clippy's one test-only clone
warning was corrected without changing the test's assertions. Final lint and
cross-project results are recorded with the candidate checkpoint in
`.artifacts/t076-application-commands/`.

The proposed user profile passes Sophia's full profile check. Its extracted
Policy fragment passes the release Hagia check, matching the runtime handoff.
Hagia's whole-file checker still rejects the pre-existing Session component
selector `window-manager`; that broader tooling discrepancy is outside the new
application grammar and is not reported as a passing whole-file check.

Remaining acceptance is an installed paired build, activation of the prepared
user profile, and normal named/inline/core-reference launches across a reload.
No active profile, installed binary, or running native owner was changed during
this implementation.


Final workspace clippy passed with `--all-targets --all-features -- -D warnings`,
as did formatting and all 21 focused reload regressions after the cleanup.
Hagia's isolated `nimble verify` passed on its final source, including the
cross-repository behavior corpus, both pregraphics admission tests, all local
suites, layout/format checks and foundation/lifecycle models. Its foundation
suite has 42 passing cases. `nimble test` also passed with isolated discovery.
The release Hagia binary SHA-256 is
`255f49f4ee5baf54ff8aa6df7c9153b55072fa425a50c0cd65c31fe128ae23ea`.

Paired Hagia source is signed commit `3459a85d5dd7a1943efcf526fa5d4ec297d246cd`. The Sophia source identity and release hash are recorded in the candidate checkpoint after signing.


## Physical launch observations on 2026-09-10

On installed Sophia `171345049bf620a40b24c48d738329b1f63decaf` and Hagia
`3459a85d5dd7a1943efcf526fa5d4ec297d246cd`, the operator confirmed visible
Kitty labels for named, inline and explicit core-reference commands. Changing
the named command from A to B and pressing Ctrl+Alt+R launched B; the owner,
Hagia, Narthex and Quickshell processes remained unchanged across those checks.
The first fixture used Kitty `--hold`, which returned to a shell prompt; the
corrected fixture prints a label and waits for Enter. Enter closes each test.

The active owner selects `~/.config/sophia/desktop.kdl`, a distinct file from
`~/.config/hagia/config.kdl`. Its direct Brave entry must use the absolute
`/home/niltempus/.local/bin/brave-origin` executable because the owner's PATH
excludes that directory. After this correction and desktop reload, the operator
confirmed Brave opens. After core watcher generation 5 applied, both Super+Enter
and Super+B worked without another desktop reload. Startup remains panel-only.

Artifacts and the exact session identity are retained in
`.artifacts/t076-desktop-acceptance/` and the [policy reload investigation](../investigations/v4geoq2j-policy-reload-compares-independent-configuration-generations.md).
The policy-change check exposed a separate generation mismatch and rolled back.
The cleaned `desktop-absolute-final.kdl` is now on disk, and chezmoi tracks
that exact file at `dot_config/private_sophia/private_desktop.kdl`. Its first signed
commit attempt was canceled by GPG. After signing was unlocked, chezmoi commit
`fbee4c3` was signed and pushed to `origin/main`.
The operator applied the cleaned profile with Ctrl+Alt+R and confirmed
Super+Enter and Super+B both still worked. Desktop generation 6 / launch
generation 10 was recorded. The temporary core registration was then removed;
watcher generation 6 applied the original core digest
`b74c43bad36cb75ce7a173f499f05f8e9921174116bcdcece99fe266d57c119c`. These observations do not pass that policy
replacement gate or the browser GPU gate.


The named/inline/core-reference and command-only reload acceptance for t076 is
satisfied on the installed pair identified above. All temporary definitions are
removed from the user's configurations. `cleanup-complete.json` retains the
final state: Sophia 22902, Hagia 15338, Narthex 22948 and Quickshell 22949. The
original core bytes are restored; the persistent desktop change is only the
direct Brave command. Policy replacement and picker repairs remain separate
t001/t002 gates requiring a repaired owner. Pending signed config publication
does not change these observed launch results.
