# Architecture Decision Records

[Notebook guide](../README.md) explains creation, acceptance, and supersession.
Current contracts are identified in the [documentation map](../../README.md).

| Record | Status | Scope |
| --- | --- | --- |
| [Separate desktop readiness from application proofs](../decisions/adr0001-separate-desktop-readiness-from-application-proofs.md) | Accepted, recorded retrospectively | Ordinary session lifecycle; physical acceptance remains pending |
| [Session owns desktop composition](../decisions/adr0002-session-owns-desktop-composition.md) | Accepted, recorded retrospectively | Operator component selection and restart semantics |

| [Separate grab ownership from presentation evidence](../decisions/mbvdvhk5-separate-grab-ownership-from-presentation-evidence.md) | Accepted 2026-09-07 | Application grab ordering, readiness, and scope evidence; physical acceptance remains separate |
| [Content capability design for sophia_shell_v1](../decisions/6ndjwffd-content-capability-design-for-sophia_shell_v1.md) | Proposed 2026-09-12 | Content shell transport, budgets, pixel semantics, wire records, invariants, and conformance corpus |
| [Separate shell presentation from GPU execution permission](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md) | Accepted 2026-09-13 | Renderer-neutral presentation, explicit portable direct GPU permission and optional mediation; implementation and native acceptance remain open |
| [Confine the Lom GPU domain with cgroup dmem](../decisions/odjw4jav-confine-the-lom-gpu-domain-with-cgroup-dmem.md) | Superseded 2026-09-13 | Historical hard-quota/kernel prerequisite, replaced by the explicit execution trust choice above |

| [Independent native launcher admission and presented input](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md) | Proposed 2026-09-16 | Bemenu beside Lom; independent admission, presented text lease and catalog activation; wire/runtime work pending |

Use `zk adr --title "The proposed choice"` to start a record. It begins as
`proposed`. Add it here with its status and keep this table consistent when a
decision is accepted, rejected, or superseded. `zk list docs/notes/decisions`
finds records even before they have been added to this map.
