// Real staged-file decode and custody hooks, independent of content admission.
#[test]
fn work_cost_classes_distinguish_demand_retire_and_invalid_typed_records() {
    let mut fixture = Fixture::new(64);
    let t = &mut fixture.transport;
    let mut peer = attach_files(t);
    let grant = t.state.content_grant.unwrap();
    let demand = ShellContentRecord::FrameDemand(ContentFrameDemand {
        grant,
        output: ContentOutputId { id: 1, generation: 1 },
        allocation: ContentAllocationId { id: 1, generation: 1 },
        demand_id: 1,
        reason: 1,
    });
    let retired = ShellContentRecord::ResourceRetire(ContentResourceRetire {
        grant,
        resource: ContentResourceId { id: 7, generation: 1 },
    });
    let rows = crate::shell_work_cost::tests::test_snapshot(|| {
        for (id, kind, record) in [
            (1, ShellFileKind::FrameDemand, demand),
            (2, ShellFileKind::ResourceRetire, retired),
        ] {
            assert!(peer.try_submit(&mut t.state, kind, |header| {
                let value = ShellFileTransactionRecord {
                    transaction: TransactionId::from_raw(id), record,
                };
                if kind == ShellFileKind::ResourceRetire {
                    encode_shell_file_resource_retire(header, &value).unwrap()
                } else {
                    encode_shell_file_transaction(header, &value).unwrap()
                }
            }));
        }
        // Valid envelope/identity but invalid typed body; must not count a demand.
        assert!(!peer.try_submit_with_refusal(&mut t.state, ShellFileKind::FrameDemand, 22, |header| {
            encode_shell_file_record_at_revision(header, &[], 8).unwrap()
        }));
        peer.quiet(&mut t.state);
    });
    for class in ["frame_demand", "resource_retire"] {
        let row = rows.iter().find(|r| r.contains(&format!(" class={class} "))).unwrap();
        assert!(row.contains("decoded=1 admitted=1"), "{row}");
    }
    let summary = &rows[0];
    assert!(summary.contains("staged_attempts=3 staged_errors=1"), "{summary}");
    assert!(summary.contains("staged_writes=3 staged_refused=1"), "{summary}");
}
