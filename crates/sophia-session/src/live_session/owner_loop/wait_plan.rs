// Deadline attribution preserves the existing short and maintenance budgets.
fn authority_wait_plan(
    physical_input: bool,
    proof_session: bool,
    held: OwnerHeldWork,
    cursor: bool,
    control: bool,
) -> owner_wake::WaitPlan {
    use owner_wake::{WaitPlan, WaitReason as R};
    let mut plan = WaitPlan::new(Duration::from_millis(25), R::Maintenance);
    // Record overlapping work independently of the one cap that wins. This
    // order is also the stable tie-break for equal 1 ms caps.
    for (pending, reason) in [
        (held.input, R::Input),
        (held.input_receipts, R::InputReceipts),
        (held.frames, R::Frames),
        (held.output_topology, R::Topology),
        (held.seat, R::Seat),
        (held.shell_interaction, R::ShellInteraction),
        (held.lifecycle, R::Lifecycle),
    ] {
        if pending {
            plan.pending(reason);
            if physical_input {
                plan.cap(Duration::from_millis(1), reason);
            }
        }
    }
    for (active, reason) in [
        (cursor, R::Cursor),
        (control, R::Controls),
        (physical_input && proof_session, R::Proof),
    ] {
        if active {
            plan.pending(reason);
            plan.cap(Duration::from_millis(1), reason);
        }
    }
    debug_assert_eq!(
        plan.timeout,
        authority_wait_timeout(
            owner_input_work_pending(physical_input, proof_session, held), cursor, control,
        ),
    );
    plan
}
