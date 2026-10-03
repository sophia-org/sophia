//! Check dispatch itself: an outer socket lock must not be needed to hide a
//! preparation. No socket service, observer or post-dispatch fixup runs here.
use super::*;

#[test]
fn timed_dispatch_constructs_both_kinds_unpublished_and_publication_is_once_only() {
    let ns = NamespaceId::from_raw(993);
    let mut runtime = XAuthorityRuntime::new();
    let mut atoms = crate::XAtomTable::new();
    let mut properties = XPropertyTable::new();
    let root = crate::X_SETUP_DEFAULT_ROOT;
    let pixmap = 0x400003u32;
    runtime
        .create_pixmap(
            ns,
            XResourceId::new(u64::from(pixmap), 1),
            Size {
                width: 4,
                height: 4,
            },
            24,
            1,
        )
        .unwrap();
    for (id, is_pixmap) in [(1, false), (2, true)] {
        let transaction = TransactionId::from_raw(id);
        let mut bytes = if is_pixmap {
            let mut bytes = vec![
                crate::X_PRESENT_MAJOR_OPCODE,
                crate::X_PRESENT_PIXMAP_MINOR_OPCODE,
                18,
                0,
            ];
            for value in [root, pixmap, 41] {
                bytes.extend(value.to_le_bytes());
            }
            bytes.resize(48, 0);
            bytes
        } else {
            let mut bytes = vec![
                crate::X_PRESENT_MAJOR_OPCODE,
                crate::X_PRESENT_NOTIFY_MSC_MINOR_OPCODE,
                10,
                0,
            ];
            for value in [root, 41, 0] {
                bytes.extend(value.to_le_bytes());
            }
            bytes
        };
        for value in [12u64, 0, 0] {
            bytes.extend(value.to_le_bytes());
        }
        let request = decode_x11_core_request(
            XWireClientContext {
                byte_order: XByteOrder::LittleEndian,
                namespace: ns,
                transaction,
                resource_id_range: None,
            },
            &bytes,
        )
        .unwrap();
        let response = crate::dispatch::dispatch_x11_wire_request_with_present_timing(
            XDispatchContext {
                byte_order: XByteOrder::LittleEndian,
                namespace: ns,
                transaction,
                sequence: id as u16,
                major_opcode: crate::X_PRESENT_MAJOR_OPCODE,
                client_id: 1,
                injection: crate::XTestAdmission::Absent,
                server_time: 1,
            },
            request,
            &mut runtime,
            &mut atoms,
            &mut properties,
            true,
        );
        assert!(response.outputs.is_empty());
        assert!(response.response.is_none());
        // Returning from dispatch already has safe state, even if its caller
        // were to release runtime before doing any more request bookkeeping.
        assert!(runtime.present_clock_admissions().is_empty());
        assert!(
            runtime
                .bind_present_clock_admission(transaction, hardware(10), None)
                .unwrap()
                .is_none()
        );
        assert!(runtime.ready_prepared_presents().is_empty());
        assert!(runtime.ready_prepared_msc_notifies().is_empty());
        assert!(runtime.publish_prepared_present_wire(transaction));
        assert!(!runtime.publish_prepared_present_wire(transaction));
        assert_eq!(runtime.present_clock_admissions().len(), 1);
        assert!(
            runtime
                .bind_present_clock_admission(transaction, hardware(10), None)
                .unwrap()
                .is_some()
        );
        if is_pixmap {
            runtime.cancel_prepared_standard_pixmap(transaction);
        } else {
            runtime.cancel_prepared_msc_notify(transaction);
        }
        assert!(!runtime.publish_prepared_present_wire(transaction));
    }
    let stats = runtime.present_timing_statistics();
    assert_eq!(
        (
            stats.wire_prepared,
            stats.wire_published,
            stats.wire_bound,
            stats.wire_hardware_bound
        ),
        (2, 2, 2, 2)
    );
    assert_eq!(
        (stats.wire_owner_notifications, stats.wire_executions),
        (0, 0)
    );
}

#[test]
fn direct_preparations_remain_ready_without_counting_as_wire_admissions() {
    let ns = NamespaceId::from_raw(993);
    let mut runtime = XAuthorityRuntime::new();
    let transaction = TransactionId::from_raw(1);
    runtime
        .prepare_present_msc_notify(
            1,
            transaction,
            ns,
            XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1),
            41,
            crate::XPresentMscTiming::notify(12, 0, 0).unwrap(),
        )
        .unwrap();
    assert_eq!(runtime.present_clock_admissions().len(), 1);
    assert!(
        runtime
            .bind_present_clock_admission(transaction, hardware(10), None)
            .unwrap()
            .is_some()
    );
    let stats = runtime.present_timing_statistics();
    assert_eq!(
        (stats.wire_prepared, stats.wire_published, stats.wire_bound),
        (0, 0, 0)
    );
}
