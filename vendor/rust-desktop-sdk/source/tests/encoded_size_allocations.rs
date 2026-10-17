//! Standalone allocation probe: compile with rustc --test against the built
//! sophia-shell-protocol rlib. Unsafe code is confined to this test allocator;
//! production crates keep their workspace unsafe-code prohibition.
use sophia_shell_protocol::{shell_files::*, *};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;
thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
fn allocation() {
    if ENABLED.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: every operation delegates the unchanged layout and pointer to System.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocation();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        allocation();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn measured<T>(f: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ENABLED.set(false);
        }
    }
    ALLOCATIONS.set(0);
    ENABLED.set(true);
    let reset = Reset;
    let result = f();
    drop(reset);
    (result, ALLOCATIONS.get())
}

#[test]
fn queries_do_not_allocate_and_encoding_is_a_positive_control() {
    let header = ShellFileHeader {
        kind: ShellFileKind::FrameDemand,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(1),
        record: ShellContentRecord::FrameDemand(ContentFrameDemand {
            grant: ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            },
            output: ContentOutputId {
                id: 1,
                generation: 1,
            },
            allocation: ContentAllocationId::default(),
            demand_id: 1,
            reason: 1,
        }),
    };
    let (size, allocations) = measured(|| shell_file_transaction_bytes(header, &record));
    assert_eq!(allocations, 0);
    let (encoded, allocations) = measured(|| encode_shell_file_transaction(header, &record));
    assert!(allocations > 0, "allocator positive control");
    assert_eq!(size.unwrap(), encoded.unwrap().len());

    let value = ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(1),
        record: ShellDescriptorRecord::DescriptorCandidate(ShellV1Candidate {
            connection_epoch: 1,
            snapshot_generation: 1,
            candidate_generation: 1,
            output: OutputId::from_raw(1),
            visible: true,
            selected_slot: Some(1),
            reservation: None,
            entries: vec![ShellV1CandidateEntry {
                slot: 1,
                generation: 1,
            }],
        }),
    };
    let header = ShellFileHeader {
        kind: ShellFileKind::DescriptorCandidate,
        ..header
    };
    let (size, allocations) = measured(|| shell_file_descriptor_bytes(header, &value));
    assert_eq!(allocations, 0);
    assert_eq!(
        size.unwrap(),
        encode_shell_file_descriptor(header, &value).unwrap().len()
    );
    let bad = ShellFileHeader {
        connection_epoch: 0,
        ..header
    };
    let (result, allocations) = measured(|| shell_file_descriptor_bytes(bad, &value));
    assert!(result.is_err());
    assert_eq!(allocations, 0);

    let tabs = ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(1),
        record: ShellDescriptorRecord::TabsCandidate(ShellTabCandidate {
            connection_epoch: 1,
            snapshot_generation: 1,
            candidate_generation: 1,
            groups: (1..=SOPHIA_SHELL_MAX_TAB_GROUPS as u64).rev().collect(),
        }),
    };
    let header = ShellFileHeader {
        kind: ShellFileKind::TabsCandidate,
        ..header
    };
    let (size, allocations) = measured(|| shell_file_descriptor_bytes(header, &tabs));
    assert_eq!(allocations, 0);
    assert_eq!(
        size.unwrap(),
        encode_shell_file_descriptor(header, &tabs).unwrap().len()
    );

    let candidate = ShellFileCandidate {
        transaction: TransactionId::from_raw(1),
        candidate: ContentCandidate {
            grant: ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            },
            candidate_generation: 1,
            output: ContentOutputId {
                id: 1,
                generation: 1,
            },
            facts_generation: 1,
            pacing_permit: 1,
            interaction_generation: 1,
            surfaces: vec![ContentSurface {
                allocation: ContentAllocationId {
                    id: 1,
                    generation: 1,
                },
                scale_generation: 1,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                reservation_extent: 24,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: ContentPixelRect::default(),
            }],
            placements: Vec::new(),
            targets: Vec::new(),
        },
    };
    let header = ShellFileHeader {
        kind: ShellFileKind::Candidate,
        ..header
    };
    let (size, allocations) = measured(|| shell_file_candidate_bytes(header, &candidate));
    assert_eq!(allocations, 0);
    assert_eq!(
        size.unwrap(),
        encode_shell_file_candidate(header, &candidate)
            .unwrap()
            .len()
    );

    let catalog = ShellFileCatalogCandidate {
        transaction: candidate.transaction,
        candidate: CatalogContentCandidate {
            candidate: candidate.candidate.clone(),
            catalog_generation: 1,
        },
    };
    let header = ShellFileHeader {
        kind: ShellFileKind::CatalogCandidate,
        ..header
    };
    let (size, allocations) = measured(|| shell_file_catalog_candidate_bytes(header, &catalog));
    assert_eq!(allocations, 0);
    assert_eq!(
        size.unwrap(),
        encode_shell_file_catalog_candidate(header, &catalog)
            .unwrap()
            .len()
    );

    let mut content = candidate.candidate;
    content.surfaces[0].role = 3;
    content.surfaces[0].reservation_extent = 0;
    content.placements.push(ContentPlacement {
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        surface_index: 0,
        destination_x_px: 0,
        destination_y_px: 0,
    });
    content.targets.push(ContentTarget {
        surface_index: 0,
        action_kind: 2,
        target_id: 1,
        target_generation: 1,
        action_id: 1,
        bounds_px: ContentPixelRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        },
    });
    let native = ShellFileNativeCandidate {
        transaction: candidate.transaction,
        candidate: NativeContentCandidate {
            candidate: content,
            opening: 1,
            catalog_generation: 1,
            state_revision: 1,
            selected: 1,
            rows: vec![1],
        },
    };
    let header = ShellFileHeader {
        kind: ShellFileKind::NativeCandidate,
        ..header
    };
    let (size, allocations) = measured(|| shell_file_native_candidate_bytes(header, &native));
    assert_eq!(allocations, 0);
    assert_eq!(
        size.unwrap(),
        encode_shell_file_native_candidate(header, &native)
            .unwrap()
            .len()
    );

    let indicator = ShellFileIndicatorActivate {
        transaction: TransactionId::from_raw(1),
        activation: ShellIndicatorActivation {
            connection_epoch: 1,
            snapshot_generation: 1,
            output: OutputId::from_raw(1),
            indicator: 1,
            action: 1,
            event_id: 1,
        },
    };
    let header = ShellFileHeader {
        kind: ShellFileKind::IndicatorActivate,
        ..header
    };
    let (size, allocations) = measured(|| shell_file_indicator_activate_bytes(header, &indicator));
    assert_eq!(allocations, 0);
    assert_eq!(
        size.unwrap(),
        encode_shell_file_indicator_activate(header, &indicator)
            .unwrap()
            .len()
    );
}
