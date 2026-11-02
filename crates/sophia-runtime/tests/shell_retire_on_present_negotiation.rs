//! W2 prototype: r12 negotiation of the owned ring with bit 17 between the
//! real SDK client and the runtime's file export. Compiled off, every bit-17
//! hello keeps today's result; compiled on, the owner policy decides. Every
//! refusal leaves exactly what today's refusal of the same kind leaves (a
//! non-W2 control), mints no grant and binds no image listener.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ShellClientError, ShellClientOptions};

#[allow(dead_code)]
#[path = "support/shell_required_image.rs"]
mod image;
#[allow(dead_code)]
#[path = "support/shell_sdk_image.rs"]
mod sdk_image;
#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;

static NEXT: AtomicU64 = AtomicU64::new(0);

const CONTENT: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;
const OWNED: u64 = 1 << 15;
const W2: u64 = 1 << 17;
#[cfg(feature = "experimental-shell-retire-on-present")]
const DRAW: u64 = 1 << 16;

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    }
}

fn session() -> (ShellComponentTransport, ContentEpochRegistry) {
    let directory = std::env::temp_dir().join(format!(
        "sophia-w2-negotiation-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut session = ShellComponentTransport::bind_for_supervised_uid(
        directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    session
        .reserve_content(&mut registry, ContentLimits::prototype(grant()))
        .unwrap();
    image::configure(&mut session, &ContentLimits::prototype(grant()));
    (session, registry)
}

/// What one negotiation left behind at both ends.
struct Outcome {
    host: Result<ShellV1ServerWelcome, ShellTransportError>,
    client: Result<(u16, u64, bool), ShellClientError>,
    /// Registry accounting and the grant once the client has gone.
    after: (ShellContentAccounting, Option<ContentGrant>),
    listener: bool,
}

/// The incremental handshake over the same authenticated attach.
fn authenticated_connecting(
    path: &std::path::Path,
    options: ShellClientOptions,
) -> Result<sophia_shell_client::ShellConnection, ShellClientError> {
    use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
    let deadline = Instant::now() + options.handshake_timeout;
    let mut pipeline =
        Pipeline::connect(path, PipelineLimits::default(), options.handshake_timeout).unwrap();
    let (tag, afid) = pipeline.auth(&[], &[]).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Auth(_)
    ));
    let (tag, root) = pipeline.attach_with_afid(afid, &[], &[]).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Attach(_)
    ));
    let tag = pipeline.clunk(afid).unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline).unwrap(),
        Reply::Clunk
    ));
    let mut connecting =
        sophia_shell_client::Connecting::attached(pipeline, root, options, 0, 2000)?;
    let started = Instant::now();
    loop {
        connecting.dispatch(started.elapsed().as_millis() as u64, 1)?;
        if let Some(error) = connecting.error() {
            return Err(error.clone());
        }
        if connecting.ready() {
            return Ok(connecting.take_connection().unwrap());
        }
        assert!(Instant::now() < deadline, "incremental startup stalled");
        std::thread::yield_now();
    }
}

fn negotiate(permit: bool, minimum: u16, maximum: u16, required: u64) -> Outcome {
    negotiate_with(false, permit, minimum, maximum, required)
}

fn negotiate_with(
    incremental: bool,
    permit: bool,
    minimum: u16,
    maximum: u16,
    required: u64,
) -> Outcome {
    let (mut session, mut registry) = session();
    #[cfg(feature = "experimental-shell-retire-on-present")]
    session.permit_retire_on_present(permit).unwrap();
    #[cfg(not(feature = "experimental-shell-retire-on-present"))]
    let _ = permit;
    let socket = session.socket_path().to_path_buf();
    let client = std::thread::spawn(move || {
        let options = ShellClientOptions {
            minimum_revision: minimum,
            maximum_revision: maximum,
            required_capabilities: required,
            handshake_timeout: Duration::from_secs(2),
        };
        let connected = if incremental {
            authenticated_connecting(&socket, options)
        } else {
            sdk_image::authenticated_client(&socket, options)
        };
        connected.map(|client| {
            let welcome = client.welcome();
            (
                welcome.selected_revision,
                welcome.capabilities,
                client.retire_on_present(),
            )
        })
    });
    let host = accept(
        &mut session,
        &mut registry,
        ShellContentAdmissionPolicy::Granted {
            discrete_input: false,
        },
    );
    // The client's limits walk, or its read of a refusal, needs more turns.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !client.is_finished() {
        assert!(Instant::now() < deadline, "client did not finish");
        let _ = session.poll_io(&mut registry);
        std::thread::yield_now();
    }
    let client = client.join().unwrap();
    let after = (
        session.content_accounting(&registry),
        session.content_grant(),
    );
    let listener = session
        .socket_path()
        .parent()
        .unwrap()
        .join("image.sock")
        .exists();
    Outcome {
        host,
        client,
        after,
        listener,
    }
}

fn accept(
    session: &mut ShellComponentTransport,
    registry: &mut ContentEpochRegistry,
    policy: ShellContentAdmissionPolicy,
) -> Result<ShellV1ServerWelcome, ShellTransportError> {
    session.begin_file_negotiation(registry, 1, Duration::from_secs(2), policy)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(welcome) = session.poll_negotiation(registry, 65536)? {
            return Ok(welcome);
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

/// Refused by both ends with exactly the end state of `control`, a refusal
/// of the same kind that predates W2, and no image listener.
fn refused_like(outcome: &Outcome, control: Outcome) -> bool {
    assert!(control.client.is_err() && control.after.1.is_none() && !control.listener);
    outcome.client.is_err() && !outcome.listener && outcome.after == control.after
}

fn unsupported(outcome: &Outcome) -> bool {
    matches!(outcome.host, Err(ShellTransportError::UnsupportedRevision))
        && refused_like(outcome, negotiate(false, 12, 12, CONTENT | OWNED))
}

fn missing(outcome: &Outcome) -> bool {
    // The owned ring without a content surface is refused the same way.
    let control = negotiate(
        false,
        10,
        10,
        SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | OWNED,
    );
    matches!(control.host, Err(ShellTransportError::MissingCapability))
        && matches!(outcome.host, Err(ShellTransportError::MissingCapability))
        && refused_like(outcome, control)
}

#[cfg(feature = "experimental-shell-retire-on-present")]
fn denied_w2(outcome: &Outcome) -> bool {
    // An unconfigured DMA-BUF bit is today's reason-4 denial.
    let control = negotiate(false, 10, 10, CONTENT | OWNED | (1 << 14));
    matches!(
        &control.host,
        Err(ShellTransportError::ContentAdmissionRefused(
            ContentAdmissionRefused { reason: 4, .. }
        ))
    ) && refused_like(outcome, control)
        && matches!(
            &outcome.host,
            Err(ShellTransportError::ContentAdmissionRefused(
                ContentAdmissionRefused {
                    reason: 4,
                    denied_capabilities: W2,
                }
            ))
        )
}

/// Granted at `revision` with exactly `capabilities` at both ends.
fn granted(outcome: &Outcome, revision: u16, capabilities: u64) -> bool {
    let welcome = outcome.host.as_ref().unwrap();
    welcome.selected_revision == revision
        && welcome.capabilities == capabilities
        && outcome.client.as_ref().unwrap() == &(revision, capabilities, capabilities & W2 != 0)
        && outcome.listener
}

const WAR: u64 = SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION;

#[test]
fn r10_owned_ring_negotiation_is_unchanged_whatever_the_policy() {
    for permit in [false, true] {
        assert!(granted(
            &negotiate(permit, 10, 10, CONTENT | OWNED),
            10,
            CONTENT | OWNED | WAR
        ));
        assert!(granted(
            &negotiate(permit, 10, 12, CONTENT | OWNED),
            10,
            CONTENT | OWNED | WAR
        ));
        assert!(unsupported(&negotiate(permit, 12, 12, CONTENT | OWNED)));
    }
}

#[cfg(not(feature = "experimental-shell-retire-on-present"))]
#[test]
fn compiled_off_every_bit_17_hello_keeps_todays_refusal() {
    // r10 with bit 17 reaches legacy validation, which does not know it.
    assert!(missing(&negotiate(false, 10, 10, CONTENT | OWNED | W2)));
    assert!(unsupported(&negotiate(false, 12, 12, CONTENT | OWNED | W2)));
    // Without an image bit, bit 17 stays on the legacy path.
    assert!(missing(&negotiate(false, 6, 6, CONTENT | W2)));
    assert!(unsupported(&negotiate(false, 12, 12, CONTENT | W2)));
}

#[cfg(feature = "experimental-shell-retire-on-present")]
#[test]
fn without_owner_policy_bit_17_is_refused_before_admission() {
    assert!(denied_w2(&negotiate(false, 12, 12, CONTENT | OWNED | W2)));
    assert!(denied_w2(&negotiate(false, 10, 12, CONTENT | OWNED | W2)));
    // Denial precedes the owned-ring check; the range check precedes both.
    assert!(denied_w2(&negotiate(false, 12, 12, CONTENT | W2)));
    assert!(unsupported(&negotiate(false, 10, 10, CONTENT | OWNED | W2)));
    assert!(unsupported(&negotiate(false, 6, 6, CONTENT | W2)));
}

#[cfg(feature = "experimental-shell-retire-on-present")]
#[test]
fn with_owner_policy_bit_17_selects_exactly_r12() {
    let w2 = CONTENT | OWNED | W2 | WAR;
    assert!(granted(
        &negotiate(true, 12, 12, CONTENT | OWNED | W2),
        12,
        w2
    ));
    assert!(granted(
        &negotiate(true, 10, 12, CONTENT | OWNED | W2),
        12,
        w2
    ));
    // No downgrade, no draw bit, no W2 without the owned ring.
    assert!(unsupported(&negotiate(true, 10, 10, CONTENT | OWNED | W2)));
    assert!(unsupported(&negotiate(true, 13, 13, CONTENT | OWNED | W2)));
    assert!(missing(&negotiate(
        true,
        12,
        12,
        CONTENT | OWNED | W2 | DRAW
    )));
    assert!(missing(&negotiate(true, 12, 12, CONTENT | W2)));
}

#[cfg(feature = "experimental-shell-retire-on-present")]
#[test]
fn the_incremental_handshake_agrees_with_the_host() {
    let w2 = CONTENT | OWNED | W2 | WAR;
    assert!(granted(
        &negotiate_with(true, true, 12, 12, CONTENT | OWNED | W2),
        12,
        w2
    ));
    assert!(granted(
        &negotiate_with(true, true, 10, 10, CONTENT | OWNED),
        10,
        CONTENT | OWNED | WAR
    ));
    assert!(denied_w2(&negotiate_with(
        true,
        false,
        12,
        12,
        CONTENT | OWNED | W2
    )));
    assert!(unsupported(&negotiate_with(
        true,
        true,
        10,
        10,
        CONTENT | OWNED | W2
    )));
}

#[cfg(feature = "experimental-shell-retire-on-present")]
#[test]
fn the_owner_policy_is_set_before_negotiation_only() {
    let (mut session, _registry) = session();
    session.permit_retire_on_present(true).unwrap();
    let mut unauthenticated = ShellComponentTransport::bind_for_supervised_uid(
        std::env::temp_dir().join(format!(
            "sophia-w2-unauthenticated-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    assert!(matches!(
        unauthenticated.permit_retire_on_present(true),
        Err(ShellTransportError::WrongContentGrant)
    ));
}
