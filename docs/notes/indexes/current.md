# Investigation and concept map

[Notebook guide](../README.md) · [ADRs](decisions.md) · [Historical topics](topic.md)

## Namespace composition

[Adopting the Plan 9 namespace model](../investigations/kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md)
investigates composable per-process file and service views, their Linux mapping,
and their relationship to Sophia's admission and isolation contracts.

## Desktop startup

The [panel-only startup investigation](../investigations/startup-panel-only-startup-physical-acceptance.md)
tracks the remaining physical check after the launch/readiness cycle was repaired.
It motivates the concept that [readiness must name an obligation](../concepts/readiness-readiness-must-name-an-obligation.md)
and the [decision to separate desktop readiness from application proofs](../decisions/adr0001-separate-desktop-readiness-from-application-proofs.md).

## Desktop composition

The [wmbench fault investigation](../investigations/djo84ohx-repeated-kms-software-mappings-account-for-the-wmbench-fault-storm.md)
distinguishes retained rendering targets from repeated Mesa software mappings,
records a 30.6% CPU reduction from retaining those mappings in the fixed guest
workload, and explains the limits of the XLibre/Sophia comparison.

The [remaining hot-path profile](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md)
locates CPU time after mapping retention, records a further 6.3% CPU reduction
from moving owned raster commands into their journal, and identifies the next
upload-ownership boundary to inspect before SIMD work.

The first two-monitor hardware baseline stopped at a
[CPU scene descriptor mismatch](../investigations/bxeem6rg-changing-the-primary-monitor-mismatched-the-cpu-scene-descriptor.md)
when the policy selected the second monitor as primary. The workload never ran;
the repair needs a fresh native qualification before profiling continues.

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

## Public role IPC and mounting

The [9P role replacement and mounting investigation](../investigations/5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
records the observed unprivileged-v9fs restriction and corrects its initial
administration-only scope. The [accepted 9P direction](../../sophia-9p-control-bus.md)
replaces public role IPC progressively, beginning with Hagia. Engine's internal
interfaces stay unchanged; direct clients require no filesystem mount.

The [desktop-role migration plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
connects the working WM path to daily acceptance, the independent Lom/Bemenu/
Provlita shell clients, and later output and administrative replacements.
The [inspection investigation](../investigations/pp3pk4dd-read-only-wm-inspection-preserves-host-admission-and-writer-progress.md)
records the separate HostDomain observer and the remaining workspace-gate limit.
