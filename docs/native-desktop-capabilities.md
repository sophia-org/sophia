# Native desktop capability map

**Role:** developer status map, not a new wire contract. Audited against Sophia
`4049fea8f265e50ae6f72976e011f68231e72e50` on 2026-09-18. The
[source audit](notes/investigations/vle7mt47-native-desktop-capability-audit-separates-contracts-from-client-ui.md)
records implementation references, test scope, and remaining evidence.

Sophia supplies the contracts needed to build a desktop. It does not need to
ship a notification center, settings panel, lock-screen theme, or another full
desktop UI to establish those contracts. Developers choose language, toolkit,
appearance, and whether features share a process. Lom, Bemenu, Provlita, and
Narthex are reference consumers, not privileged protocol identities.

## Read the status correctly

**Connected** means a production path exists for the stated scope, not that it
is stable or fully accepted. **Partial** means some machinery exists but the
developer operation is not complete. **Specified only** means a documented
target; **absent** means no public native contract was found in the audited
schema/config/service paths. **External** names a service outside shell wire
authority. Tests, independent-client interoperability, and physical acceptance
are separate evidence dimensions; missing tests are not proof of missing code.

The shell schema is experimental **revision 8**. Capabilities remain negotiated
and constrained by operator policy. The stable WM interface remains revision 3
with explicitly gated extensions. No toolkit types or Rust dependency are
required on the wire. Optional Rust and C helpers cover different subsets.

## What a developer can use

| Developer operation | State and public boundary | Owner, permission and limit | Remaining task |
| --- | --- | --- | --- |
| Choose separate native components | Connected for bar, application-launcher, dock; legacy single-shell mode remains | Session; explicit selections, maximum three components, exclusive configured roles | t104/t105 acceptance and contract reconciliation; t109 for other roles |
| Draw and update own pixels | Connected r5 `content_surface`; immutable resources, candidates, permits and exact release | Session/Engine; default-denied content, negotiated aggregate byte/resource/candidate limits | t097/t100, performance t102 |
| Render those pixels on a GPU | Connected startup grant, separate from wire content | Session; explicit render-node grant and bounded discovery projection, no KMS or foreign pixels; no hard aggregate VRAM quota | t097 acceptance; optional broker t103 remains candidate |
| Reserve a panel/dock edge | Connected `work_area_reservation` with content allocations | Session caps allowance; Engine resolves geometry; WM lays out apps; output/grant-scoped | t038/t083, component arbitration t106 |
| Present an anchored popout | Partial complete workflow; r5 allocation/parent/anchor machinery exists | Engine; exact presented parent, bounded size, outside dismissal/input ownership | t099 |
| Show a transient overlay | Connected for the r7 native application launcher; not a general overlay grant | Session/Engine; exact opening, output, candidate and focus lease, no reservation | General presentation admission t109 |
| Draw a desktop background | Specified direction; no native background component role | Engine placement/stacking, Session admission; client can rasterize its own image | t049 and t109 |
| Use output size and scale | Connected r5 output facts/allocation results | Engine; scoped output/generation/scale; not permission to inspect arbitrary desktop geometry | Topology/recovery t100/t106 |
| Change output configuration | Partial: experimental output codec and existing configuration owner, no declarative output role schema | Output authority/Session, not a shell permission implied by output facts | t022/t023; no new generic settings UI required |
| React to buttons and workspace clicks | Connected r5 discrete input and r6 indicator activation | Engine presented targets; independent ACK and WM admission; stale grant/publication refuses | t038/t100 and acceptance t081 |
| Search a launcher using keyboard/text | Connected r7 semantic launcher input | Exact opening/edit revision and revocable presented focus; no ambient key feed | t106/t107 acceptance; general text/IME t111 |
| Implement richer drag/resize/scroll interaction | Partial target-resolved input machinery; discrete shell actions are not a general toolkit event stream | Engine/input authority; target scope and revocation must survive the entire gesture | t040; text/IME remains t111 |
| Publish/work with workspaces | Connected r6 indicators and gated WM output launch contexts | WM owns workspace policy; Session publishes committed facts; actions and tokens are opaque | t043 must not duplicate indicators; placement t041/t108 |
| List and activate windows | Connected bounded sanitized descriptors/tabs and issuer-scoped actions; richer dock feed is partial | Metadata broker, Session and WM; recipient-scoped generations/redaction; no raw application identity by default | t043/t038 |
| Launch catalog applications | Connected r4 descriptors, r7 native launcher, r8 persistent catalog | Session supplies authorized catalog and supervises execution; clicked output/workspace captured as WM token | t105–t108, physical clean-exit acceptance remains |
| Bind shortcuts and request session actions | Connected WM-owned bindings, advertised operation catalog, separate control service | WM/Session; existing operations only; no arbitrary command authority from content | Missing system-service grants t113; t037 remains separate reload work |
| Display actionable notifications | Partial portal reducer and chrome machinery; no demonstrated end-to-end arbitrary native notification provider | Portal/Session authorize payload and actions; rendering is downstream | t046, presentation admission t109 |
| Provide clipboard history, drag/drop, capture or secure prompts | Partial, with different existing portal/frontend implementations; not shell capabilities | Portal owners and explicit source/recipient grants; X11 clipboard support is not native history/capture access | t046; secure lock t034 |
| Provide audio/network/battery controls or tray items | External service responsibility; confined access contract incomplete | Service-specific broker/Session grants; shell content must not imply unrestricted D-Bus/system access | t113 |
| Observe idle or inhibit idle actions | Absent public native idle/inhibit service in audited interfaces | Session/input authority; no raw input disclosure | t110 |
| Lock the session | Specified security transition, not ordinary content admission | Session/Engine input epoch, complete secure coverage and authenticated unlock | t034 |
| Provide general text input/IME or accessibility | Absent general native shell contracts; launcher text is narrower | Input authority plus explicitly disclosed client semantics; separate capabilities | t111/t112 |
| Request effects that sample other content | Specified direction, not arbitrary client GPU access | Engine-only scene sampling and clock; provider availability, bounded parameters and cancellation required | t047/t048; client-only drawing needs no new primitive |

Rows naming multiple tasks divide responsibilities; they do not create duplicate
implementations. UI-only choices—notification history styling, dock animation,
settings widgets, launcher ranking, clock format—remain client work. If their
existing generic primitive is sufficient, they need no Sophia task.

## Admission, lifecycle and interoperability

Admission never follows from connecting to a socket or naming a role. Effective
capabilities must remain within implementation support, client request and
operator grants. The current fixed three-role configuration is not a promise
that any fourth component can connect. A future notification/OSD/background
provider needs its actual presentation and service contracts first.

All future contracts must preserve output/grant identities, exact presented
input, bounded queues and resource accounting, revocation, backpressure, and
failure isolation. Changing focus or topology must not repurpose an old action.
Disconnect does not prove render consumers released their bytes. Prepared,
Presented, action acknowledgement, policy completion, and ResourceReleased are
distinct facts.

The independent C and Nim consumers are interoperability evidence for their
tested subsets; an SDK helper or golden-frame parser alone does not establish
the complete lifecycle. See the [family contract](sophia-policy-ipc.md),
the C client guide at `bindings/c/README-shell.md`, and
[component configuration](desktop-composition.md).

## Follow-up order

Finish existing popout and lifecycle/arbitration gaps (t099/t100/t106) and retain
their conformance/acceptance gates (t023/t107/t108). The first **new** foundation
is t109: specify bounded admission/presentation for additional providers using
those owners. Then extend metadata, portal and system-service boundaries from
their named consumers. Idle, text/IME, accessibility and lock remain separate
explicit contracts; none is granted by the broader role model.

The [gap plan](notes/plans/1sxw3fyj-native-desktop-protocol-gaps-after-the-three-component-baseline.md)
defines candidate exits. [todo.md](../todo.md) alone owns task status and order.
This audit does not promote those candidates or authorize a desktop UI suite.
