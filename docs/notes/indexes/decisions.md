# Architecture Decision Records

[Notebook guide](../README.md) explains creation, acceptance, and supersession.
Current contracts are identified in the [documentation map](../../README.md).

| Record | Status | Scope |
| --- | --- | --- |
| [Separate desktop readiness from application proofs](../decisions/adr0001-separate-desktop-readiness-from-application-proofs.md) | Accepted, recorded retrospectively | Ordinary session lifecycle; physical acceptance remains pending |
| [Session owns desktop composition](../decisions/adr0002-session-owns-desktop-composition.md) | Accepted, recorded retrospectively | Operator component selection and restart semantics |
| [Adopt 9P2000.L as the target public interface](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md) | Accepted direction 2026-09-25; implementation reaffirmed 2026-09-26 | Progressive public desktop IPC replacement; WM implementation and acceptance in progress, other role contracts and the later application frontend remain open |

| [Separate grab ownership from presentation evidence](../decisions/mbvdvhk5-separate-grab-ownership-from-presentation-evidence.md) | Accepted 2026-09-07 | Application grab ordering, readiness, and scope evidence; physical acceptance remains separate |
| [Content capability design for sophia_shell_v1](../decisions/6ndjwffd-content-capability-design-for-sophia_shell_v1.md) | Proposed 2026-09-12 | Content shell transport, budgets, pixel semantics, wire records, invariants, and conformance corpus |
| [Separate shell presentation from GPU execution permission](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md) | Accepted 2026-09-13 | Renderer-neutral presentation, explicit portable direct GPU permission and optional mediation; implementation and native acceptance remain open |
| [Confine the Lom GPU domain with cgroup dmem](../decisions/odjw4jav-confine-the-lom-gpu-domain-with-cgroup-dmem.md) | Superseded 2026-09-13 | Historical hard-quota/kernel prerequisite, replaced by the explicit execution trust choice above |

| [Independent native launcher admission and presented input](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md) | Proposed 2026-09-16 | Bemenu beside Lom; independent admission, presented text lease and catalog activation; wire/runtime work pending |
| [Carry descriptor families as native shell file records](../decisions/4oapm903-carry-descriptor-families-as-native-shell-file-records.md) | Proposed 2026-09-28 | Preserve descriptor, tab, shortcut, reference and launcher functionality through native file records; codecs, export and independent peer acceptance pending |
| [Keep broker and portal file authority and custody separate](../decisions/xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md) | Design accepted 2026-09-28 | Separate native role contracts, atomic broker responses, explicit portal admission and bounded history; exports, SDKs and socket retirement remain unimplemented |
| [Keep profile persistence separate from runtime display trials](../decisions/vnem82wz-keep-the-desktop-profile-as-the-persistent-display-configuration-and-treat-runtime-output-changes-as-confirmed-trials.md) | Proposed 2026-09-30 | Revision-2 confirmation and host profile editing; revision-1 output acceptance remains unchanged |
| [Serve every public role from one 9P core with namespaces as composed trees and portals as binds](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md) | Proposed 2026-10-09 | One 9P server core for all public roles; application authority as an export; namespaces derived from the admission context by a recipe; portals executed as recipient binds; Engine, X authority and role contracts unchanged |

Use `zk adr --title "The proposed choice"` to start a record. It begins as
`proposed`. Add it here with its status and keep this table consistent when a
decision is accepted, rejected, or superseded. `zk list docs/notes/decisions`
finds records even before they have been added to this map.
