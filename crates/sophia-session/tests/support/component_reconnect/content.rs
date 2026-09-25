//! Supply only the concrete-native submission boundary. Projection, resource
//! ownership, Prepared and the Session pending record are real; the shared
//! deterministic backend target replaces device submission. Observation later
//! runs the unchanged Session completion consumer.
use super::*;
use sophia_backend_live::{LiveShellContentLayer, session_content_fixture::SessionContentFixture};

pub(in crate::live_session) fn prepare(
    content: &mut LiveContentSession,
    transport: &mut ShellTransportConnection<'_>,
    fixture: &mut SessionContentFixture,
    descriptor: HeadlessOutput,
    bundle: ContentRenderBundle,
) {
    let allocations = transport.content_allocation_snapshots();
    let frame = project_render_bundle(&bundle, descriptor, bundle.output, &allocations).unwrap();
    let root = Rect {
        x: 0,
        y: 0,
        width: descriptor.size.width,
        height: descriptor.size.height,
    };
    let bands = candidate_bands(&bundle, &allocations, &[(descriptor.id, root)], root).unwrap();
    fixture.admit(frame, LiveShellContentLayer::Shell).unwrap();
    let grant = transport.content_grant().unwrap();
    transport
        .content_prepared(grant, bundle.output, bundle.candidate_generation, 1, 1, 0)
        .unwrap();
    content.facts_generation = 1;
    content.published_facts = vec![output_facts_entry(descriptor).unwrap()];
    content.pending.push(PendingPresentation {
        catalog: None,
        grant,
        output: bundle.output,
        candidate_generation: bundle.candidate_generation,
        bands,
        allocations: bundle.surfaces.iter().map(|s| s.allocation).collect(),
    });
}
