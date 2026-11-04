# Enabled coverage

All configurations enable `PreparedOrder`, `WorkerSafe`, `PhysicalSettlement`,
`StreamBound`, `TypeOK`, `BudgetSafe`, `CreditSafe`, `SourceSafe`, `SceneSafe`,
`AuthoritySafe`, `JournalSafe`, `RetirementSafe` and `TerminalSafe`.

| Configuration | Frames / heads / credits / workers / response capacity | Purpose |
|---|---|---|
| MC | 2 / 1 / 1 / 1 / 5 | Competing input; credit reuse; copy cancellation; replacement epoch; delayed delivery |
| TwoCredits | 2 / 1 / 2 / 2 / 7 | Concurrent accepted copies; independent settlements; exact full reservation capacity |
| Mirror | 1 / 2 / 1 / 1 / 4 | Prepare-all, independent submits/flips, primary presentation, lagging predecessor |
| Liveness | 1 / 1 / 1 / 1 / 4 | `FairSpec`; `Settles` and `ResponsesDrain` under explicitly fair progress |
| Reopen | 2 / 1 / 1 / 1 / 5 | Same-epoch close/drain/reopen, two stream identities; old physical owners remain |

Each has two epochs and one possible facts change per epoch. Close, revoke,
validation failure, cancellation and arbitrary queue delay remain enabled in
all safety searches. Configurations share a spec rather than silently disabling
failure actions for individual hunts.

| Requirement | Enabled property / action | Deliberate failing control |
|---|---|---|
| D1 accepted-copy escrow | CreditSafe / Admit | CreditMutation: omit initial debit |
| D1 cancellation retains physical reservation | PhysicalSettlement, WorkerSafe / Terminal, CopyDone | CancelMutation: settle while reading |
| D1 whole-input response capacity | BudgetSafe / Admit, Refuse, all publications | Full-budget positive searches; byte/fragment admission remains outside scope |
| D2/D3 private capture and exact source handoff | SourceSafe / CopyDone, PublishSource, Deliver, Reuse | SourceMutation: publish release before worker completion |
| D4 complete prepared scene | SceneSafe / Handoff, Prepare, Submit | SceneMutation: bypass preparation and Prepared-transfer gates |
| D4 current authority at submit | AuthoritySafe / Submit | AuthorityMutation: bypass current/open check |
| D4 facts-bound stream | StreamBound / ChangeFacts | StreamMutation: silently retarget existing stream |
| D5 ordering and retirement | PreparedOrder, TerminalSafe, RetirementSafe / Terminal, FreePredecessor | RetirementMutation: free before lagging head drops predecessor |
| D6 stale completion isolation | JournalSafe, CreditSafe / Revoke, Terminal | Positive replacement-epoch interleavings; physical ownership not cleared on revoke |
| D6 stream reopening | ReopenSafe / StreamDrained, Reopen | ReopenMutation: issue a new stream before drain |
| Eventual logical settlement | Settles, ResponsesDrain / FairSpec | Liveness configuration only; no bounded-time claim |

Mutation files select an explicit `Fault` constant; production-correct
configurations select `"none"`. The runner requires each mutation to fail its
specific invariant; parser errors or another failure do not count as a pass.

This table does not extend the modeling brief's scope. In particular, there is
no unbounded stream-reuse proof, no general accepted-backing reclamation proof, no
wire/parser refinement and no physical GPU/display correctness proof.

## Separate bounded resource slice

`Resources.cfg` checks `BudgetSafe`, `ChargedUse`, `FreeSafe`,
`PublicationSafe` and `EpochSafe`. `ResourceProgress.cfg` adds fair
`RetiredDrains`. The bounds are two resource identities and consumers, sizes 1/2,
three total byte units and two epochs. Negative configurations independently
remove cancellation charge retention, lease retention, live-journal publication,
and epoch checking. The checker requires the specific expected violation for
each. These are not a composed proof of FrameCommit's resource/wire adapter.

## Separate bounded intake slice

`Intake.cfg` and `IntakeProgress.cfg` enable `BudgetSafe`, `CreditSafe`,
`ReaderSafe`, `AdmissionSafe`, `RefusalSafe`, `SettlementSafe`,
`FragmentsEnabled` and `ContinuationEnabled`. The latter configuration also
checks fair `InputDrains` and `AcceptedSettles`. Negative controls independently
remove refusal reservation, block fragments or continuations on a full budget,
and admit using a stale pre-close decision. Each must violate its named
invariant. This is proposed policy, not proof of today's candidate wire poller.
