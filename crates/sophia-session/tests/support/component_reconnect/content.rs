//! Supply only the concrete-native submission boundary. Projection, resource
//! ownership, Prepared and the Session pending record are real; the shared
//! deterministic backend target replaces device submission. Observation later
//! runs the unchanged Session completion consumer.
use super::*;
use sophia_backend_live::{LiveShellContentLayer, session_content_fixture::SessionContentFixture};

pub(in crate::live_session) fn prime_outputs(
    content: &mut LiveContentSession,
    descriptor: HeadlessOutput,
) {
    content.facts_generation = 1;
    content.published_facts = vec![output_facts_entry(descriptor).unwrap()];
}

pub(in crate::live_session) fn prepare(
    content: &mut LiveContentSession,
    transport: &mut ShellTransportConnection<'_>,
    fixture: &mut SessionContentFixture,
    descriptor: HeadlessOutput,
    bundle: ContentRenderBundle,
) {
    let root = Rect {
        x: 0,
        y: 0,
        width: descriptor.size.width,
        height: descriptor.size.height,
    };
    prime_outputs(content, descriptor);
    content
        .submit_bundle_to(
            transport,
            &[descriptor],
            &[(descriptor.id, root)],
            root,
            bundle,
            LiveShellContentLayer::Shell,
            crate::shell_component_connections::content_now_msec(),
            |frame, layer| fixture.admit(frame, layer),
        )
        .unwrap();
}
