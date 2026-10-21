//! Two protected component processes keep independent stop and replacement
//! custody under real Factotum launch authentication and the production
//! component owner. Each child is this test binary: it authenticates over its
//! inherited relay with the Rust SDK, negotiates shell r9 with sealed images,
//! and sends one sealed image for an allocation the fixture's supplied
//! placement policy grants. Resource and budget assertions are retained from
//! the socket fixture at ef1f93a53. The same child also parks after an r9
//! negotiation for the production-launched launcher evidence test. Requires
//! nested user namespaces.
use super::*;
use crate::shell_component_connections::content_now_msec;
use crate::test_sdk_image as sdk_image;
use sophia_protocol::shell_files::ShellFileNativeLauncherRecord;
use sophia_protocol::*;
use sophia_runtime::{
    ContentCandidateContext, ContentResourceLease, NativeLauncherCandidateContext,
};
use sophia_shell_client::{
    NativeLauncherObservation, ProducerMetadata, ShellClientOptions, ShellConnection,
};
use std::path::{Path, PathBuf};

const OUTPUT: ContentOutputId = ContentOutputId {
    id: 1,
    generation: 1,
};
const ALLOCATION: ContentAllocationId = ContentAllocationId {
    id: 1,
    generation: 1,
};
const RESOURCE: ContentResourceId = ContentResourceId {
    id: 1,
    generation: 1,
};
const CHILD: &str =
    "shell_component_processes::authentication::tests::custody::protected_custody_child";

/// The descriptor bootstrap that hands the relay to the child as stdin.
struct Bootstrap(PathBuf);

impl Bootstrap {
    fn build() -> Self {
        let target = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").expect("cargo-slot"));
        let temporary = target.join("tmp");
        std::fs::create_dir_all(&temporary).unwrap();
        let binary = temporary.join(format!("t331-component-custody-{}", std::process::id()));
        assert!(
            std::process::Command::new("/usr/bin/cc")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .args([
                    "-std=c99",
                    "-Wall",
                    "-Wextra",
                    "-Werror",
                    "-pedantic",
                    "-O2"
                ])
                .arg(format!("-DSOPHIA_TEST_CHILD=\"{CHILD}\""))
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/support/lock_rust_bootstrap.c"
                ))
                .arg("-o")
                .arg(&binary)
                .status()
                .unwrap()
                .success()
        );
        Self(binary)
    }
}

impl Drop for Bootstrap {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn opening(grant: ContentGrant) -> NativeLauncherOpening {
    NativeLauncherOpening {
        grant,
        output: OUTPUT,
        opening: 1,
        catalog_generation: 1,
        state_revision: 1,
    }
}

fn snapshot(native: bool) -> ContentAllocationSnapshot {
    ContentAllocationSnapshot {
        native_opening: native.then_some(1),
        output: OUTPUT,
        allocation: ALLOCATION,
        scale_generation: 1,
        scale_numerator: 1,
        scale_denominator: 1,
        role: if native { 3 } else { 1 },
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 64,
            height: 16,
        },
        pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 64,
            height: 16,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: if native { 0 } else { 16 },
    }
}

/// One owner pass for a connected child: facts (and the launcher's opening)
/// once, then the supplied placement grant, then resource service.
fn serve(transport: &mut ShellTransportConnection<'_>, native: bool, published: &mut bool) {
    let grant = transport.content_grant().unwrap();
    let now = content_now_msec();
    if !*published {
        transport
            .publish_content_output_facts(
                TransactionId::from_raw(2),
                1,
                vec![ContentOutputFactsEntry {
                    output: OUTPUT,
                    local_width: 64,
                    local_height: 32,
                    scale_numerator: 1,
                    scale_denominator: 1,
                    scale_generation: 1,
                }],
            )
            .unwrap();
        if native {
            transport
                .publish_native_launcher_opening(TransactionId::from_raw(79), opening(grant))
                .unwrap();
        }
        *published = true;
    }
    if native {
        let catalog = ShellApplicationCatalog {
            connection_epoch: grant.connection_epoch,
            generation: 1,
            entries: Vec::new(),
        };
        transport
            .service_native_launcher_content(
                ContentCandidateContext {
                    output: OUTPUT,
                    facts_generation: 1,
                    interaction_generation: 1,
                    allocations: &[],
                },
                NativeLauncherCandidateContext {
                    opening: opening(grant),
                    state_revision: 1,
                    catalog: &catalog,
                },
                now,
            )
            .unwrap();
    } else {
        transport
            .service_content_allocation_requests(&[], now)
            .unwrap();
    }
    if let Some((_, request)) = transport.next_content_allocation_request() {
        transport
            .grant_content_allocation(request.allocation_request_id, snapshot(native), &[])
            .unwrap();
    }
    transport.service_content_resources(now).unwrap();
    transport.poll_io().unwrap();
}

struct Fixture {
    auth: factotum::Fixture,
    wake: sophia_wake::Wake,
    owner: ShellComponentProcesses,
}

impl Fixture {
    fn visit(&mut self) -> ComponentProcessVisit {
        self.auth.tick();
        let visit = self.owner.visit_authenticated(
            65536,
            &mut self.auth.coordinator,
            self.wake.notifier(),
            Instant::now(),
        );
        let failures = self.owner.turn_connected();
        assert!(
            failures.iter().all(Option::is_none),
            "shell turn failed: {failures:?}"
        );
        visit
    }

    /// Drive the actual owner until the child's sealed image is accepted.
    fn receive_resource(
        &mut self,
        key: ComponentConnectionKey,
        reports: &Path,
    ) -> ContentResourceLease {
        let report = reports.join(key.grant.connection_epoch.to_string());
        let mut lease = None;
        let native = key.slot == 1;
        let mut published = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let visit = self.visit();
            assert!(
                visit.processes.iter().flatten().all(|event| !matches!(
                    event,
                    ComponentProcessEvent::Failed(..) | ComponentProcessEvent::ProcessRetired(..)
                )),
                "protected component failed: {:?}",
                visit.processes
            );
            for (_, result) in visit.negotiations.into_iter().flatten() {
                result.unwrap();
            }
            if self.owner.phase(key).unwrap() == ComponentConnectionPhase::Connected {
                let leased = self
                    .owner
                    .with_connection(key, |t| {
                        // The authenticated attach was granted content.
                        assert_eq!(t.content_grant(), Some(key.grant));
                        serve(t, native, &mut published);
                        t.lease_content_resource(key.grant, RESOURCE)
                    })
                    .unwrap();
                if lease.is_none() {
                    lease = leased.ok();
                }
            }
            // The SDK must settle its offer before the parent may stop it.
            if std::fs::read(&report).ok().as_deref() == Some(b"settled")
                && let Some(lease) = lease.take()
            {
                assert!(
                    self.owner.process_retained(key),
                    "child exited after settling"
                );
                return lease;
            }
            assert!(Instant::now() < deadline, "sealed image not received");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn until_released(&mut self, keys: &[ComponentConnectionKey]) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while keys.iter().any(|key| self.owner.process_retained(*key)) {
            self.visit();
            assert!(Instant::now() < deadline, "protected component not reaped");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
#[ignore = "requires nested user namespaces; run explicitly inside the device-hidden fixture"]
fn two_protected_processes_retain_independent_stop_and_replacement_custody() {
    let bootstrap = Bootstrap::build();
    let executable = std::env::current_exe().unwrap();
    let directory = std::env::temp_dir().join(format!(
        "protected-component-custody-{}",
        std::process::id()
    ));
    let reports = directory.join("reports");
    std::fs::create_dir_all(&reports).unwrap();
    let auth = factotum::Fixture::new();
    let mut owner = ShellComponentProcesses::new().unwrap();
    for (name, role, audience) in [
        ("bar", ShellComponentRole::Bar, 4),
        ("launcher", ShellComponentRole::ApplicationLauncher, 6),
    ] {
        let slot = owner
            .add(
                name,
                role,
                &directory.join(name),
                rustix::process::geteuid().as_raw(),
            )
            .unwrap();
        owner
            .authenticate(
                slot,
                LaunchPolicy {
                    principal: audience,
                    namespace: 2,
                    audience,
                    role: ProtectionDomainRole::MetadataShell,
                },
                name,
                auth.coordinator.endpoint(),
            )
            .unwrap();
    }
    let mut f = Fixture {
        auth,
        wake: sophia_wake::Wake::new().unwrap(),
        owner,
    };
    let prepare = |key: ComponentConnectionKey, socket: &Path| {
        let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])
            .map_err(|e| e.to_string())?
            .path(ProtectionPath::read_only(socket.parent().unwrap()))
            .map_err(|e| e.to_string())?
            .path(ProtectionPath::read_only(&executable))
            .map_err(|e| e.to_string())?
            .path(ProtectionPath::read_write(&reports))
            .map_err(|e| e.to_string())?;
        let mut spec = ProcessLaunchSpec::new(&bootstrap.0)
            .env("SOPHIA_FIXTURE_ROLE", key.slot.to_string())
            .env("SOPHIA_SHELL_SOCKET", "/wrong/retired-endpoint")
            .env("SOPHIA_SHELL_9P_SOCKET", "/wrong/file-endpoint")
            .protection_domain(domain);
        spec.args = vec![
            executable.clone().into_os_string(),
            socket.as_os_str().to_owned(),
            reports
                .join(key.grant.connection_epoch.to_string())
                .into_os_string(),
        ];
        Ok(spec)
    };
    let policy = ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    };
    let bar = f.owner.start(0, prepare, policy).unwrap();
    let menu = f.owner.start(1, prepare, policy).unwrap();
    assert_ne!(bar.grant, menu.grant);
    assert!(f.owner.process_retained(bar) && f.owner.process_retained(menu));
    let bar_pixels = f.receive_resource(bar, &reports);
    let menu_pixels = f.receive_resource(menu, &reports);
    assert_eq!(bar_pixels.cpu_bytes().unwrap(), &[1, 2, 3, 255]);
    f.owner.request_stop(menu).unwrap();
    assert!(f.owner.start(1, prepare, policy).is_err());
    f.until_released(&[menu]);
    assert!(f.owner.process_retained(bar));
    assert_eq!(
        f.owner.phase(bar).unwrap(),
        ComponentConnectionPhase::Connected
    );
    // The successor fits beside the live bar by reducing its retiring budget;
    // the old lease stays charged until its consumer releases it.
    let retained = f.owner.collect();
    assert_eq!(retained.retired_epochs, 1);
    assert_eq!(retained.reserved_bytes, 24 * 1024 * 1024 + 4);
    assert_eq!(menu_pixels.cpu_bytes().unwrap(), &[1, 2, 3, 255]);
    assert!(f.owner.process_retained(bar));
    let replacement = f.owner.start(1, prepare, policy).unwrap();
    assert_ne!(replacement.grant, menu.grant);
    let replacement_pixels = f.receive_resource(replacement, &reports);
    assert_eq!(replacement_pixels.cpu_bytes().unwrap(), &[1, 2, 3, 255]);
    let limits = f
        .owner
        .with_connection(replacement, |t| t.content_limits().unwrap().clone())
        .unwrap();
    assert_eq!(limits.max_retiring_bytes, 8 * 1024 * 1024 - 4);
    assert_eq!(limits.max_resident_bytes, 12 * 1024 * 1024);
    assert_eq!(f.owner.accounting().reserved_bytes, 48 * 1024 * 1024);
    assert_eq!(menu_pixels.cpu_bytes().unwrap(), &[1, 2, 3, 255]);
    drop(menu_pixels);
    let released = f.owner.collect();
    assert_eq!(released.retired_epochs, 0);
    assert_eq!(released.reserved_bytes, 48 * 1024 * 1024 - 4);
    assert!(f.owner.request_stop(menu).is_err());
    for _ in 0..10 {
        f.visit();
        assert!(f.owner.process_retained(replacement));
        assert!(f.owner.process_retained(bar));
        std::thread::sleep(Duration::from_millis(1));
    }
    f.owner.request_stop(bar).unwrap();
    f.owner.request_stop(replacement).unwrap();
    f.until_released(&[bar, replacement]);
    drop((bar_pixels, replacement_pixels));
    assert!(f.owner.finish_after_backend_drop(()).unwrap().1.quiescent());
    drop(f);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires nested user namespaces; run explicitly inside the device-hidden fixture"]
fn joined_launcher_evidence_requires_exact_current_negotiation() {
    use crate::shell_component_session::ShellComponentSession;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let root =
        std::env::temp_dir().join(format!("joined-launcher-evidence-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let binary = root.join("menu/fixture-launcher");
    let selection = sophia_config::ShellComponentConfig {
        id: "menu".into(),
        role: ShellComponentRole::ApplicationLauncher,
        executable: binary.clone(),
        config: None,
        reservation: None,
        gpu: sophia_config::ShellGpuMode::Denied,
        transport: sophia_config::ShellTransportSelection::NineP2000L,
    };
    let mut owner = ShellComponentSession::prepare(
        &[selection],
        0,
        None,
        &root,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        },
        Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH),
    )
    .unwrap();
    let namespaces = factotum::namespaces();
    let mut auth = factotum::Fixture::with_namespaces(namespaces.clone());
    owner
        .authenticate(&auth.coordinator, &mut namespaces.lock().unwrap())
        .unwrap();
    // The production launcher supplies --serve and binds only the executable
    // and endpoint directory. Keep the bootstrap and child inside it; this
    // adapter changes the test-harness argv, not the launch authority.
    let bootstrap = Bootstrap::build();
    std::fs::copy(&bootstrap.0, root.join("menu/bootstrap")).unwrap();
    let peer = root.join("menu/peer");
    let current = std::env::current_exe().unwrap();
    if std::fs::hard_link(&current, &peer).is_err() {
        std::fs::copy(&current, &peer).unwrap();
    }
    std::fs::write(
        &binary,
        "#!/bin/sh\nset -eu\n[ \"$#\" -eq 1 ] && [ \"$1\" = --serve ]\nexport SOPHIA_FIXTURE_ROLE=park\nd=\"$(dirname -- \"$0\")\"\nexec \"$d/bootstrap\" \"$d/peer\" \"$SOPHIA_SHELL_9P_SOCKET\" unused\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let outputs = [sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(1),
        size: sophia_protocol::Size {
            width: 64,
            height: 64,
        },
        scale: 1,
    }];
    let mut runtime =
        sophia_backend_live::LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut poll = |owner: &mut ShellComponentSession| {
        auth.tick();
        owner
            .poll_authenticated(64 * 1024, &mut auth.coordinator, wake.notifier())
            .unwrap()
    };
    owner.set_presentation_available(true).unwrap();
    let mut previous = None;
    for _ in 0..2 {
        let key = owner.start(0).unwrap();
        assert!(
            owner.launch_evidence(key).is_err(),
            "spawn is not negotiation"
        );
        if let Some(old) = previous {
            assert!(owner.launch_evidence(old).is_err());
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while owner.phase(key).unwrap() != ComponentConnectionPhase::Connected {
            let visit = poll(&mut owner);
            assert!(visit.processes.iter().all(Option::is_none));
            for (_, result) in visit.negotiations.into_iter().flatten() {
                result.unwrap();
            }
            assert!(Instant::now() < deadline);
        }
        let (role, gpu) = owner.launch_evidence(key).unwrap();
        assert_eq!(role, ShellComponentRole::ApplicationLauncher);
        assert!(gpu.is_none(), "launcher cannot inherit a GPU grant");
        let mut wrong = key;
        wrong.grant.content_grant_epoch += 1;
        assert!(owner.launch_evidence(wrong).is_err());
        // The authenticated launcher keeps its negotiated connection alive and
        // in service until the deliberate stop below.
        for _ in 0..20 {
            let visit = poll(&mut owner);
            assert!(visit.processes.iter().all(Option::is_none));
            assert!(owner.process_retained(key));
            assert!(owner.launch_evidence(key).is_ok());
        }
        owner.stop(key).unwrap();
        assert!(owner.launch_evidence(key).is_err());
        while owner.process_retained(key) {
            poll(&mut owner);
            assert!(Instant::now() < deadline);
        }
        assert_eq!(owner.settle_revocations(Some(&mut runtime)).unwrap(), 1);
        previous = Some(key);
    }
    owner.request_shutdown().unwrap();
    owner.settle_revocations(Some(&mut runtime)).unwrap();
    assert!(owner.finish_after_backend_drop(()).unwrap().1.quiescent());
    drop(owner);
    std::fs::remove_dir_all(root).unwrap();
}

fn next_content(client: &mut ShellConnection) -> ShellContentRecord {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some((_, record)) = client.poll_content().unwrap() {
            return record;
        }
        assert!(Instant::now() < deadline, "missing content record");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The launcher's native observations, in order, until `wanted` accepts one.
fn native_until(
    client: &mut ShellConnection,
    mut wanted: impl FnMut(&NativeLauncherObservation) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        client.poll_io().unwrap();
        if let Some(observation) = client.take_native_launcher_observation().unwrap()
            && wanted(&observation)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "missing native launcher observation"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// `sdk_image::upload` for the launcher, whose content records arrive on its
/// native-launcher event stream rather than through `poll_content`.
fn native_upload(client: &mut ShellConnection, metadata: ProducerMetadata, pixels: &[u8]) {
    use sophia_shell_client::{ImageChannelState, OfferState};
    let deadline = Instant::now() + Duration::from_secs(5);
    while client.image_adapter().unwrap().channel() != Some(ImageChannelState::Ready) {
        client.poll_io().unwrap();
        assert!(Instant::now() < deadline, "image channel not ready");
        std::thread::yield_now();
    }
    let offer = client
        .image_begin(metadata, sdk_image::source(pixels), Instant::now())
        .unwrap()
        .offer;
    let mut accepted = false;
    loop {
        client.poll_io().unwrap();
        if let Some(observation) = client.take_native_launcher_observation().unwrap()
            && let NativeLauncherObservation::Content(_, ShellContentRecord::ResourceStatus(status)) =
                observation
        {
            assert_eq!(status.resource.id, metadata.resource);
            assert_eq!(status.status, 2, "{status:?}");
            accepted = true;
        }
        let info = client.image_adapter().unwrap().offer(offer).unwrap();
        if accepted && info.state == OfferState::Settled {
            assert_eq!(info.disposition, 1);
            break;
        }
        assert!(Instant::now() < deadline, "image not settled: {info:?}");
        std::thread::yield_now();
    }
    client.image_forget(offer).unwrap();
}

#[test]
#[ignore = "parent invokes this child under the protected shell supervisor"]
fn protected_custody_child() {
    use std::os::unix::net::UnixStream;
    let socket = PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap());
    // The owner strips the retired variable and replaces any supplied endpoint.
    assert!(std::env::var_os("SOPHIA_SHELL_SOCKET").is_none());
    assert_eq!(
        std::env::var_os("SOPHIA_SHELL_9P_SOCKET").as_deref(),
        Some(socket.as_os_str())
    );
    let mode = std::env::var("SOPHIA_FIXTURE_ROLE").unwrap();
    // A parked launcher only negotiates; Connected is its parent's evidence.
    let park = mode == "park";
    let native = park || mode == "1";
    eprintln!(
        "custody child role={}",
        if park {
            "parked"
        } else if native {
            "launcher"
        } else {
            "bar"
        }
    );
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let role = UnixStream::connect(&socket).unwrap();
    let name: &[u8] = if native { b"launcher" } else { b"bar" };
    let mut auth = sophia_launch_client::Client::over(role, relay, b"shell", name).unwrap();
    while !auth.ready() {
        auth.poll()
            .expect("Rust SDK shell authentication must succeed");
        auth.wait_for_io(Duration::from_millis(10)).unwrap();
    }
    let admitted = auth.into_authenticated().unwrap();
    // Bit 13: sealed images, required for every content attach at r9.
    let capabilities = (1 << 13)
        | if native {
            SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
                | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER
        } else {
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        };
    let mut client = ShellConnection::from_attached(
        admitted.role,
        admitted.root,
        ShellClientOptions {
            minimum_revision: 9,
            maximum_revision: 9,
            required_capabilities: capabilities,
            handshake_timeout: Duration::from_secs(5),
        },
    )
    .expect("Rust SDK must continue on the authenticated shell root");
    let welcome = client.welcome();
    assert_eq!(welcome.selected_revision, 9);
    if park {
        // Hold the negotiated connection until Session revokes and reaps it.
        std::thread::sleep(Duration::from_secs(10));
        drop(client);
        drop(admitted.relay);
        return;
    }
    let report = PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap());
    let ShellContentRecord::Limits(limits) = next_content(&mut client) else {
        panic!("limits");
    };
    assert_eq!(welcome.connection_epoch, limits.grant.connection_epoch);
    let grant = limits.grant;
    sdk_image::enable(&mut client, socket.parent().unwrap(), &limits);
    // A sealed image names a live allocation, requested once facts exist.
    if native {
        native_until(&mut client, |observation| {
            matches!(
                observation,
                NativeLauncherObservation::Record(ShellFileNativeLauncherRecord {
                    record: ShellNativeLauncherRecord::Opening(_),
                    ..
                })
            )
        });
        client
            .enqueue_native_launcher_tracked(&ShellFileNativeLauncherRecord {
                transaction: TransactionId::from_raw(80),
                record: ShellNativeLauncherRecord::AllocationRequest(
                    NativeLauncherAllocationRequest {
                        grant,
                        output: OUTPUT,
                        opening: 1,
                        request_id: 1,
                        prior: ContentAllocationId::default(),
                        operation: 1,
                        edge: 1,
                        desired_width: 64,
                        desired_height: 16,
                        margins: ContentMargins::default(),
                    },
                ),
            })
            .unwrap();
        native_until(&mut client, |observation| match observation {
            NativeLauncherObservation::Content(_, ShellContentRecord::AllocationResult(result)) => {
                assert_eq!(result.status, 1, "{result:?}");
                true
            }
            _ => false,
        });
    } else {
        while !matches!(
            next_content(&mut client),
            ShellContentRecord::OutputFacts(_)
        ) {}
        client
            .send_content(
                TransactionId::from_raw(80),
                &ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                    grant,
                    output: OUTPUT,
                    allocation_request_id: 1,
                    operation: 1,
                    role: 1,
                    edge: 1,
                    prior: ContentAllocationId::default(),
                    parent: ContentAllocationId::default(),
                    parent_presentation_epoch: 0,
                    anchor_parent_rect: ContentPixelRect::default(),
                    desired_width: 64,
                    desired_height: 16,
                    margins: ContentMargins::default(),
                }),
            )
            .unwrap();
        let ShellContentRecord::AllocationResult(result) = next_content(&mut client) else {
            panic!("allocation result");
        };
        assert_eq!(result.status, 1, "{result:?}");
    }
    let metadata = ProducerMetadata {
        resource: RESOURCE.id,
        resource_generation: RESOURCE.generation,
        admission_epoch: grant.content_grant_epoch,
        output_id: OUTPUT.id,
        allocation_id: ALLOCATION.id,
        topology_generation: 1,
        source_kind: 1,
        source_format: 1,
        width: 1,
        height: 1,
        stride: 4,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 4,
        ..Default::default()
    };
    if native {
        native_upload(&mut client, metadata, &[1, 2, 3, 255]);
    } else {
        sdk_image::upload(&mut client, metadata, &[1, 2, 3, 255]);
    }
    // Only a settled and forgotten offer is reported; the parent's lease waits.
    std::fs::write(&report, b"settled").unwrap();
    // Preserve both owners until Session revokes and reaps this process.
    std::thread::sleep(Duration::from_secs(10));
    drop(client);
    drop(admitted.relay);
}
