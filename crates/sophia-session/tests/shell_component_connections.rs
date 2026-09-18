//! Session connection owner + private socket codecs + real resource consumers.
//! Protection evidence is supplied, not a launched child; no native/focus proof.
use sophia_config::ShellComponentRole;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_session::shell_component_connections::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Harness {
    owner: ShellComponentConnections,
    directory: std::path::PathBuf,
}
impl Harness {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "session-component-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let mut owner = ShellComponentConnections::new().unwrap();
        for (id, role) in [
            ("panel", ShellComponentRole::Bar),
            ("menu", ShellComponentRole::ApplicationLauncher),
        ] {
            owner
                .add(
                    id,
                    role,
                    &directory.join(id),
                    rustix::process::geteuid().as_raw(),
                )
                .unwrap();
        }
        Self { owner, directory }
    }
    fn begin(&mut self, key: ComponentConnectionKey) -> UnixStream {
        self.owner
            .begin_negotiation(
                key,
                &evidence(),
                Duration::from_secs(2),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: key.slot == 1,
                },
            )
            .unwrap();
        let client = UnixStream::connect(self.owner.socket_path(key.slot).unwrap()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        client
    }
    fn connect(&mut self, key: ComponentConnectionKey) -> UnixStream {
        let mut client = self.begin(key);
        client.write_all(&hello(key.slot == 1)).unwrap();
        let events = self.owner.poll_negotiations(65536);
        let (received, welcome) = events.into_iter().flatten().next().unwrap();
        assert_eq!(received, key);
        let welcome = welcome.unwrap();
        assert_eq!(welcome.connection_epoch, key.grant.connection_epoch);
        assert_eq!(welcome.selected_revision, if key.slot == 1 { 7 } else { 6 });
        assert_eq!(
            welcome.capabilities & SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER != 0,
            key.slot == 1
        );
        assert_eq!(
            self.owner
                .with_connection(key, |t| t.supports_native_launcher())
                .unwrap(),
            key.slot == 1
        );
        read_frame(&mut client);
        read_frame(&mut client);
        client
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn evidence() -> ProtectionDomainEvidence {
    ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid: std::process::id(),
        roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
    }
}
fn hello(native: bool) -> Vec<u8> {
    if native {
        return encode_shell_v1_client_hello_frame(ShellV1ClientHello {
            minimum_revision: 7,
            maximum_revision: 7,
            required_capabilities: SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
                | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER,
        })
        .unwrap();
    }
    encode_shell_v1_client_hello_frame(ShellV1ClientHello {
        minimum_revision: 5,
        maximum_revision: 6,
        required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
            | SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS
            | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION,
    })
    .unwrap()
}
fn read_frame(client: &mut UnixStream) -> Vec<u8> {
    let mut frame = vec![0; SOPHIA_IPC_HEADER_LEN];
    client.read_exact(&mut frame).unwrap();
    let size = u32::from_le_bytes(frame[16..20].try_into().unwrap()) as usize;
    frame.resize(SOPHIA_IPC_HEADER_LEN + size, 0);
    client
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    frame
}
fn upload(
    h: &mut Harness,
    key: ComponentConnectionKey,
    client: &mut UnixStream,
    id: u64,
) -> ContentResourceLease {
    let grant = key.grant;
    let resource = ContentResourceId { id, generation: 1 };
    for record in [
        ShellContentRecord::ResourceBegin(ContentResourceBegin {
            grant,
            resource,
            width_px: 1,
            height_px: 1,
            rendered_scale_numerator: 1,
            rendered_scale_denominator: 1,
            pixel_format: 1,
            chunk_count: 1,
            total_bytes: 4,
        }),
        ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant,
            resource,
            ordinal: 0,
            offset: 0,
            bytes: vec![1, 2, 3, 255],
        }),
        ShellContentRecord::ResourceEnd(ContentResourceEnd {
            grant,
            resource,
            total_bytes: 4,
            chunk_count: 1,
        }),
    ] {
        client
            .write_all(&encode_shell_content_frame(TransactionId::from_raw(id), &record).unwrap())
            .unwrap();
    }
    h.owner
        .with_connection(key, |transport| {
            transport.service_content_resources(1).unwrap();
            transport.poll_io().unwrap();
        })
        .unwrap();
    for status in [1, 2] {
        let (_, ShellContentRecord::ResourceStatus(value)) =
            decode_shell_content_frame(&read_frame(client)).unwrap()
        else {
            panic!("status");
        };
        assert_eq!(value.grant, grant);
        assert_eq!(value.resource, resource);
        assert_eq!(value.status, status);
    }
    h.owner
        .with_connection(key, |transport| {
            transport.lease_content_resource(grant, resource).unwrap()
        })
        .unwrap()
}

#[test]
fn failed_launcher_attempt_burns_epochs_without_resetting_bar() {
    let mut h = Harness::new();
    let panel = h.owner.reserve_attempt(0).unwrap();
    let _bar = h.connect(panel);
    let first = h.owner.reserve_attempt(1).unwrap();
    let mut menu = h.begin(first);
    menu.write_all(&[0; 24]).unwrap();
    let events = h.owner.poll_negotiations(65536);
    assert_eq!(events.iter().flatten().count(), 1);
    assert!(events.into_iter().flatten().next().unwrap().1.is_err());
    assert_eq!(h.owner.phase(first), Ok(ComponentConnectionPhase::Revoked));
    assert_eq!(
        h.owner.phase(panel),
        Ok(ComponentConnectionPhase::Connected)
    );
    assert!(h.owner.poll_negotiations(65536).iter().all(Option::is_none));
    let second = h.owner.reserve_attempt(1).unwrap();
    assert!(second.grant.connection_epoch > first.grant.connection_epoch);
    assert!(second.grant.content_grant_epoch > first.grant.content_grant_epoch);
    assert_eq!(
        h.owner.close(first),
        Err(ComponentConnectionError::StaleAttempt)
    );
    let _new = h.connect(second);
    assert_eq!(h.owner.accounting().active_epochs, 2);
    h.owner.close(panel).unwrap();
    h.owner.close(second).unwrap();
    h.owner.close(second).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[test]
fn retained_launcher_bytes_refuse_replacement_while_bar_uploads() {
    let mut h = Harness::new();
    let panel = h.owner.reserve_attempt(0).unwrap();
    let mut bar = h.connect(panel);
    let menu_key = h.owner.reserve_attempt(1).unwrap();
    let mut menu = h.connect(menu_key);
    let before = h.owner.accounting();
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    h.owner
        .with_connection(panel, |transport| {
            assert_eq!(
                transport.content_prepared(menu_key.grant, output, 1, 1, 1, 1),
                Err(ShellTransportError::WrongContentGrant)
            );
            assert_eq!(
                transport.content_presented(menu_key.grant, output, 1, 1, 1, 1),
                Err(ShellTransportError::WrongContentGrant)
            );
            assert_eq!(
                transport.content_renderer_failed(menu_key.grant, output, 1),
                Err(ShellTransportError::WrongContentGrant)
            );
        })
        .unwrap();
    assert_eq!(h.owner.accounting(), before);
    let held = upload(&mut h, menu_key, &mut menu, 1);
    h.owner.close(menu_key).unwrap();
    let retained = h.owner.collect();
    assert_eq!(retained.retired_epochs, 1);
    assert_eq!(retained.memory.resident, 4);
    assert_eq!(retained.reserved_bytes, 40 * 1024 * 1024 + 4);
    assert_eq!(held.bytes(), &[1, 2, 3, 255]);
    assert!(matches!(
        h.owner.reserve_attempt(1),
        Err(ComponentConnectionError::Transport(
            ShellTransportError::ContentStore(ContentStoreError::Budget)
        ))
    ));
    let bar_bytes = upload(&mut h, panel, &mut bar, 1);
    assert_eq!(bar_bytes.description().grant, panel.grant);
    drop(held);
    let released = h.owner.collect();
    assert_eq!(released.retired_epochs, 0);
    assert_eq!(released.reserved_bytes, 40 * 1024 * 1024);
    let replacement = h.owner.reserve_attempt(1).unwrap();
    // Attempt 3 was consumed even though its budget reservation refused.
    assert_eq!(replacement.grant.connection_epoch, 4);
    assert_eq!(replacement.grant.content_grant_epoch, 4);
    let _replacement = h.connect(replacement);
    assert_eq!(
        h.owner
            .with_connection(panel, |t| t.content_grant())
            .unwrap(),
        Some(panel.grant)
    );
    assert!(h.owner.with_connection(menu_key, |_| ()).is_err());
    drop(bar_bytes);
    h.owner.close(panel).unwrap();
    h.owner.close(replacement).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[test]
fn bounded_negotiation_visits_both_peers_and_alternates_first_owner() {
    let mut h = Harness::new();
    let a = h.owner.reserve_attempt(0).unwrap();
    let b = h.owner.reserve_attempt(1).unwrap();
    let mut ac = h.begin(a);
    let mut bc = h.begin(b);
    ac.write_all(&hello(false)[..4]).unwrap();
    bc.write_all(&hello(true)).unwrap();
    assert!(h.owner.poll_negotiations(0).iter().all(Option::is_none));
    // Both still pending, but first visit rotates even with zero byte credit.
    let events = h.owner.poll_negotiations(65536);
    assert_eq!(events[0].as_ref().unwrap().0, b);
    assert!(events[1].is_none());
    assert_eq!(h.owner.phase(a), Ok(ComponentConnectionPhase::Negotiating));
    ac.write_all(&hello(false)[4..]).unwrap();
    let events = h.owner.poll_negotiations(65536);
    assert_eq!(events[0].as_ref().unwrap().0, a);
    assert!(events[1].is_none());
    for c in [&mut ac, &mut bc] {
        read_frame(c);
        read_frame(c);
    }
    h.owner.close(a).unwrap();
    h.owner.close(b).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[test]
fn admission_requires_exact_attempt_and_protected_role() {
    let mut h = Harness::new();
    let key = h.owner.reserve_attempt(0).unwrap();
    assert_eq!(
        h.owner.reserve_attempt(0),
        Err(ComponentConnectionError::Busy)
    );
    assert!(h.owner.with_connection(key, |_| ()).is_err());
    let forged = ComponentConnectionKey { slot: 1, ..key };
    assert_eq!(
        h.owner.close(forged),
        Err(ComponentConnectionError::StaleAttempt)
    );
    let mut wrong = evidence();
    wrong.roles.clear();
    assert!(
        h.owner
            .begin_negotiation(
                key,
                &wrong,
                Duration::from_secs(1),
                ShellContentAdmissionPolicy::Unavailable
            )
            .is_err()
    );
    assert_eq!(h.owner.phase(key), Ok(ComponentConnectionPhase::Revoked));
    assert!(h.owner.collect().quiescent());
    assert_eq!(
        h.owner.add(
            "third",
            ShellComponentRole::Bar,
            &h.directory.join("third"),
            rustix::process::geteuid().as_raw()
        ),
        Err(ComponentConnectionError::InvalidSelection)
    );
    assert!(!h.directory.join("third").exists());
}

#[test]
fn final_owner_transfer_refuses_live_admission_and_drops_the_actual_consumer() {
    let mut h = Harness::new();
    let key = h.owner.reserve_attempt(1).unwrap();
    let mut client = h.connect(key);
    let held = upload(&mut h, key, &mut client, 1);
    let held = h
        .owner
        .finish_after_backend_drop(held)
        .expect_err("live admission retains actual consumer");
    assert_eq!(held.bytes(), &[1, 2, 3, 255]);
    h.owner.close(key).unwrap();
    assert_eq!(h.owner.collect().retired_epochs, 1);
    let (settled, accounting) = h
        .owner
        .finish_after_backend_drop(held)
        .unwrap_or_else(|_| panic!("closed owner must accept final disposition"));
    assert_eq!(settled, 0);
    assert!(accounting.quiescent());
}

#[cfg(feature = "native-session")]
#[test]
fn borrowed_panel_service_uses_the_shared_registry_and_refuses_another_attempt() {
    use sophia_session::shell_panel_service::PanelComponentService;
    let mut h = Harness::new();
    let panel = h.owner.reserve_attempt(0).unwrap();
    let native = h.owner.reserve_attempt(1).unwrap();
    let mut client = h.connect(panel);
    let mut neighbor = h.connect(native);
    let accounting = h.owner.accounting();
    let mut service = h
        .owner
        .with_connection(panel, |t| {
            // The operator request cannot upgrade the actual negotiated grant.
            assert!(PanelComponentService::new(t, 30, true).is_err());
            assert!(PanelComponentService::new(t, 0, false).is_err());
            PanelComponentService::new(t, 30, false).unwrap()
        })
        .unwrap();
    assert_eq!(service.grant(), panel.grant);
    let publication = sophia_engine::PolicyIndicatorPublication {
        tab_groups: vec![],
        generation: 1,
        connection_epoch: Some(20),
        indicators: vec![],
        output_statuses: vec![],
    };
    h.owner
        .with_connection(native, |t| {
            assert!(PanelComponentService::new(t, 30, false).is_err());
            assert!(
                service
                    .service_indicators(t, Some(&publication), None)
                    .is_err()
            );
        })
        .unwrap();
    h.owner
        .with_connection(panel, |t| {
            service
                .service_indicators(t, Some(&publication), None)
                .unwrap();
            t.poll_io().unwrap();
        })
        .unwrap();
    let snapshot = sophia_session::shell_indicator_publication::indicator_snapshot(
        &publication,
        None,
        panel.grant.connection_epoch,
    );
    for expected in encode_shell_indicator_snapshot(TransactionId::from_raw(1), &snapshot).unwrap()
    {
        assert_eq!(read_frame(&mut client), expected);
    }
    // Unchanged snapshots enqueue nothing and the native peer receives no panel frames.
    h.owner
        .with_connection(panel, |t| {
            service
                .service_indicators(t, Some(&publication), None)
                .unwrap();
            t.poll_io().unwrap();
        })
        .unwrap();
    for peer in [&mut client, &mut neighbor] {
        peer.set_nonblocking(true).unwrap();
        assert_eq!(
            peer.read(&mut [0; 1]).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
    assert_eq!(h.owner.accounting(), accounting);
    h.owner.close(panel).unwrap();
    h.owner.collect();
    let replacement = h.owner.reserve_attempt(0).unwrap();
    let _new_client = h.connect(replacement);
    h.owner
        .with_connection(replacement, |t| {
            assert!(
                service
                    .service_indicators(t, Some(&publication), None)
                    .is_err()
            );
            assert_eq!(
                PanelComponentService::new(t, 30, false).unwrap().grant(),
                replacement.grant
            );
        })
        .unwrap();
    h.owner.close(replacement).unwrap();
    h.owner.close(native).unwrap();
    assert!(h.owner.collect().quiescent());
}

#[cfg(feature = "native-session")]
#[test]
fn borrowed_native_content_places_real_wire_request_without_granting_early_focus() {
    use sophia_backend_live::{LiveProductionCpuScene, LiveProductionVisualRuntime};
    use sophia_engine::HeadlessOutput;
    use sophia_session::shell_native_launcher::NativeLauncherContentService;
    let mut h = Harness::new();
    let key = h.owner.reserve_attempt(1).unwrap();
    let mut client = h.connect(key);
    let outputs = [HeadlessOutput {
        id: OutputId::from_raw(2),
        size: Size {
            width: 800,
            height: 600,
        },
        scale: 1,
    }];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(outputs[0].size);
    let catalog = ShellApplicationCatalog {
        connection_epoch: key.grant.connection_epoch,
        generation: 1,
        entries: vec![],
    };
    let opening = NativeLauncherOpening {
        grant: key.grant,
        opening: 1,
        output: ContentOutputId {
            id: 2,
            generation: 1,
        },
        catalog_generation: 1,
        state_revision: 1,
    };
    let mut service = h
        .owner
        .with_connection(key, |t| {
            let service = NativeLauncherContentService::new(t).unwrap();
            t.publish_native_launcher_opening(TransactionId::from_raw(1), opening)
                .unwrap();
            t.poll_io().unwrap();
            service
        })
        .unwrap();
    assert_eq!(service.grant(), key.grant);
    assert_eq!(
        decode_shell_native_launcher_frame(&read_frame(&mut client))
            .unwrap()
            .1,
        ShellNativeLauncherRecord::Opening(opening)
    );
    client
        .write_all(
            &encode_shell_native_launcher_frame(
                TransactionId::from_raw(2),
                &ShellNativeLauncherRecord::AllocationRequest(NativeLauncherAllocationRequest {
                    grant: key.grant,
                    opening: 1,
                    output: opening.output,
                    request_id: 1,
                    prior: ContentAllocationId::default(),
                    operation: 1,
                    edge: 1,
                    desired_width: 300,
                    desired_height: 100,
                    margins: ContentMargins::default(),
                }),
            )
            .unwrap(),
        )
        .unwrap();
    let root = Rect {
        x: 0,
        y: 0,
        width: 800,
        height: 600,
    };
    let mut serial = 10;
    h.owner
        .with_connection(key, |t| {
            service
                .service_open(
                    t,
                    &catalog,
                    &mut runtime,
                    &scene,
                    None,
                    &outputs,
                    &[(outputs[0].id, root)],
                    root,
                    &mut || {
                        serial += 1;
                        Ok(TransactionId::from_raw(serial))
                    },
                )
                .unwrap();
            assert_eq!(t.native_launcher_focus(), None);
            assert!(
                t.install_native_launcher_focus(TransactionId::from_raw(20))
                    .is_err()
            );
            assert!(!service.observe_presentation(t, &runtime).unwrap());
            let allocations = t.content_allocation_snapshots();
            assert_eq!(allocations.len(), 1);
            assert_eq!(allocations[0].native_opening, Some(1));
            assert_eq!(allocations[0].logical.x, 250);
            assert_eq!(allocations[0].allowed_reservation_extent, 0);
            t.poll_io().unwrap();
        })
        .unwrap();
    let (_, facts) = decode_shell_content_frame(&read_frame(&mut client)).unwrap();
    assert!(matches!(facts, ShellContentRecord::OutputFacts(_)));
    let (_, result) = decode_shell_content_frame(&read_frame(&mut client)).unwrap();
    let ShellContentRecord::AllocationResult(result) = result else {
        panic!("allocation reply missing")
    };
    assert_eq!(result.status, 1);
    assert_eq!(result.grant, key.grant);
    assert_eq!((result.pixel.x, result.pixel.width), (250, 300));
    let demand_transaction = TransactionId::from_raw(913);
    client
        .write_all(
            &encode_shell_content_frame(
                demand_transaction,
                &ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant: key.grant,
                    output: opening.output,
                    allocation: result.allocation,
                    demand_id: 1,
                    reason: 1,
                }),
            )
            .unwrap(),
        )
        .unwrap();
    let serial_before = serial;
    h.owner
        .with_connection(key, |t| {
            service
                .service_open(
                    t,
                    &catalog,
                    &mut runtime,
                    &scene,
                    None,
                    &outputs,
                    &[(outputs[0].id, root)],
                    root,
                    &mut || {
                        serial += 1;
                        Ok(TransactionId::from_raw(serial))
                    },
                )
                .unwrap();
            t.poll_io().unwrap();
        })
        .unwrap();
    let (permit_transaction, ShellContentRecord::FramePermit(permit)) =
        decode_shell_content_frame(&read_frame(&mut client)).unwrap()
    else {
        panic!("permit missing")
    };
    assert_eq!(permit_transaction, demand_transaction);
    assert_eq!(
        serial, serial_before,
        "reply must not mint a server transaction"
    );
    assert_eq!((permit.demand_id, permit.state), (1, 1));
    h.owner
        .with_connection(key, |t| {
            let close_tx = TransactionId::from_raw(914);
            let mut wrong = opening;
            wrong.opening += 1;
            assert!(
                service
                    .begin_close(t, wrong, close_tx, ContentReason::Cancelled)
                    .is_err()
            );
            service
                .begin_close(t, opening, close_tx, ContentReason::Cancelled)
                .unwrap();
            assert!(
                service
                    .begin_close(
                        t,
                        opening,
                        TransactionId::from_raw(915),
                        ContentReason::Cancelled
                    )
                    .is_err()
            );
            // No native candidate was submitted. Pixel absence must not imply that
            // the active allocation or the connection's grant has been released.
            for _ in 0..2 {
                assert!(
                    service
                        .service_close_pixels(t, &mut runtime, &scene, None)
                        .unwrap()
                );
                assert_eq!(t.content_allocation_snapshots().len(), 1);
                assert_eq!(t.content_grant(), Some(key.grant));
            }
            assert!(
                service
                    .service_open(
                        t,
                        &catalog,
                        &mut runtime,
                        &scene,
                        None,
                        &outputs,
                        &[(outputs[0].id, root)],
                        root,
                        &mut || Ok(TransactionId::from_raw(916)),
                    )
                    .is_err()
            );
        })
        .unwrap();
    h.owner.close(key).unwrap();
    h.owner.collect();
    let replacement = h.owner.reserve_attempt(1).unwrap();
    let _new_client = h.connect(replacement);
    h.owner
        .with_connection(replacement, |t| {
            assert!(service.observe_presentation(t, &runtime).is_err());
            assert_eq!(
                NativeLauncherContentService::new(t).unwrap().grant(),
                replacement.grant
            );
        })
        .unwrap();
    h.owner.close(replacement).unwrap();
    assert!(h.owner.collect().quiescent());
}
