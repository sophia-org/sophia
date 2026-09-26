# Investigation and concept map

[Notebook guide](../README.md) · [ADRs](decisions.md) · [Historical topics](topic.md)

## Desktop startup

The [panel-only startup investigation](../investigations/startup-panel-only-startup-physical-acceptance.md)
tracks the remaining physical check after the launch/readiness cycle was repaired.
It motivates the concept that [readiness must name an obligation](../concepts/readiness-readiness-must-name-an-obligation.md)
and the [decision to separate desktop readiness from application proofs](../decisions/adr0001-separate-desktop-readiness-from-application-proofs.md).

## Desktop composition

The [Session ownership decision](../decisions/adr0002-session-owns-desktop-composition.md)
explains where users choose the WM, shell, and startup applications. It links to
the operator guide and its original implementation evidence.

## Shell direction

The historical [descriptor/content-shell discussion](../sources/2026-09/legacy-active-0634-2026-09-06--descriptor-and-content-shells-have-distinct-trust-contracts.md)
explains the distinction between descriptor capabilities and shell-owned
content. [Content shells](../../content-shell.md) owns the behavior, while the
[implementation record](../../lom-content-implementation.md) separates existing
CPU transport/lifecycle work from production admission and input gaps.
The [accepted execution decision](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
permits explicitly granted direct GPU rendering on stock Linux without making
Vello or a GPU bridge part of the shell wire. The
[paired Lom/Sophia plan](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md)
owns the integration sequence and native-acceptance exits.

Add connections here when they help someone find an investigation. Search the
whole collection with `zk list docs/notes --match "terms"`; this page is a curated
map, not a list of every note or another roadmap.

## Native shell component proposal

[Explicit scoped component grants](../concepts/k2d9l42p-native-shell-components-compose-through-explicit-scoped-grants.md) describes integrated and modular shells; the linked candidate plan starts with a bar and independent launcher. This is not current multi-client support.

## Session control plane

The [Plan 9 control-plane investigation](../investigations/5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
records the observed unprivileged-v9fs restriction and corrects its initial
administration-only scope. The [accepted 9P direction](../../sophia-9p-control-bus.md)
replaces public role IPC progressively, beginning with Hagia. Engine's internal
interfaces stay unchanged; direct clients require no filesystem mount.
