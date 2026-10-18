use sophia_engine::{
    OutputFrameServiceObservation, OutputFrameServiceReducer, OutputFrameServiceRequest,
    OutputNativeFramePhase,
};
use sophia_protocol::OutputId;

#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

#[test]
fn stable_service_and_reordered_observations_allocate_nothing() {
    for count in [1, 2, 16] {
        let mut request = OutputFrameServiceRequest {
            outputs: (1..=count)
                .map(|id| OutputFrameServiceObservation {
                    output: OutputId::from_raw(id),
                    primary: id == 1,
                    native_phase: OutputNativeFramePhase::InFlight,
                    pending_frame: true,
                })
                .collect(),
            ..Default::default()
        };
        let before = ALLOCATOR.stats();
        for _ in 0..1000 {
            let mut reducer = OutputFrameServiceReducer::begin(&request).unwrap();
            request.outputs.reverse();
            for _ in 0..count {
                assert!(reducer.next_effect(&request).unwrap().is_some());
            }
            assert!(reducer.next_effect(&request).unwrap().is_none());
            for output in &mut request.outputs {
                output.native_phase = OutputNativeFramePhase::Idle;
            }
            for _ in 0..count {
                assert!(reducer.next_effect(&request).unwrap().is_some());
            }
            assert!(reducer.next_effect(&request).unwrap().is_none());
            for output in &mut request.outputs {
                output.native_phase = OutputNativeFramePhase::InFlight;
            }
            std::hint::black_box(reducer);
        }
        let measured = ALLOCATOR.stats() - before;
        eprintln!(
            "{count} outputs, 1000 service turns: {} allocations, {} reallocations",
            measured.allocations, measured.reallocations
        );
        assert_eq!(measured.allocations + measured.reallocations, 0);
    }
    // The counter is process-wide, so this measurement shares the one test
    // rather than running beside it on another thread.
    building_and_refusing_requests_allocates_nothing();
}

/// The owner rebuilds its request several times per pass. Building, copying
/// and validating one must not touch the heap at any supported output count,
/// nor when the owner overflows the capacity and is refused.
fn building_and_refusing_requests_allocates_nothing() {
    let observation = |id: u64| OutputFrameServiceObservation {
        output: OutputId::from_raw(id),
        primary: id == 1,
        native_phase: OutputNativeFramePhase::Idle,
        pending_frame: false,
    };
    for count in [1, 2, 16, 17, 64] {
        let before = ALLOCATOR.stats();
        for _ in 0..1000 {
            let mut outputs = sophia_engine::OutputFrameServiceObservations::new();
            for id in 1..=count {
                outputs.push(observation(id));
            }
            let request = OutputFrameServiceRequest {
                outputs,
                ..Default::default()
            };
            let collected = OutputFrameServiceRequest {
                outputs: (1..=count).map(observation).collect(),
                ..Default::default()
            };
            assert_eq!(request, collected);
            let result = OutputFrameServiceReducer::begin(&request);
            assert_eq!(result.is_ok(), count <= 16);
            std::hint::black_box(request);
        }
        let measured = ALLOCATOR.stats() - before;
        eprintln!(
            "{count} offered outputs, 1000 builds: {} allocations, {} reallocations",
            measured.allocations, measured.reallocations
        );
        assert_eq!(measured.allocations + measured.reallocations, 0);
    }
}
