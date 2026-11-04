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
| Eventual logical settlement | Settles, ResponsesDrain / FairSpec | Liveness configuration only; no bounded-time claim |

Mutation files select an explicit `Fault` constant; production-correct
configurations select `"none"`. The runner requires each mutation to fail its
specific invariant; parser errors or another failure do not count as a pass.

This table does not extend the modeling brief's scope. In particular, there is
no same-epoch reopen proof, no general accepted-backing reclamation proof, no
wire/parser refinement and no physical GPU/display correctness proof.
