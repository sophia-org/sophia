//! LIVE gate: the real `bemenu-sophia --serve` executable, from a prepared,
//! signed-revision artifact, over Sophia's native launcher 9P file contract,
//! against the production file export and native launcher owners, launched by
//! the production protected-process supervisor (Bubblewrap, Shell role).
//!
//! Opt-in: ordinary `cargo test` has no application artifact, so this test is
//! #[ignore]d. Its dedicated invocation fails closed on any absent or
//! mismatched input; it never passes or skips silently:
//!
//!   cargo xtask prepare-bemenu-artifact <source-repo> <signed-commit> <new-output-dir>
//!   SOPHIA_BEMENU_ARTIFACT=<output-dir> SOPHIA_BEMENU_SHA256=<binary sha256> \
//!   SOPHIA_BEMENU_COMMIT=<signed commit> nice -n 19 \
//!   cargo test -p sophia-runtime --test shell_bemenu_files -- --ignored --nocapture
//!
//! Covered: negotiation; catalog and output-fact objects; opening; the peer's
//! allocation request; the actual Cairo raster upload; Prepared/Presented;
//! exact focus; a text edit whose next candidate changes rows and pixels;
//! exactly one keyboard Accept admission; close, allocation invalidation and
//! peer resource retirement to owner settlement; reopen with a reset query and
//! a second close; graceful SIGTERM stop. Not covered: pointer/ContentAction
//! activation, focus loss, scale or facts change mid-opening, reconnect, and
//! every expiry path (see the fixture's clock note). Session decisions are
//! scripted; no launch-policy or physical-rendering claim is made.
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

#[path = "support/shell_bemenu_files/artifact.rs"]
mod artifact;
#[path = "support/shell_bemenu_files/fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/shell_files_oracle/process.rs"]
mod process;
#[path = "support/shell_bemenu_files/sandbox.rs"]
mod sandbox;

use sophia_protocol::NativeLauncherInputKind;

const TOTAL: Duration = Duration::from_secs(40);
const PHASE: Duration = Duration::from_secs(10);

/// Service the owners until `done`, bounded per phase and overall. Owner errors,
/// early exit and oversized output end the gate with Bemenu's stderr.
fn until(
    f: &mut fixture::Fixture,
    peer: &mut sandbox::Peer,
    start: Instant,
    what: &str,
    mut done: impl FnMut(&fixture::Fixture) -> bool,
) {
    let phase = Instant::now();
    loop {
        peer.check();
        if let Err(error) = f.tick() {
            panic!("{what}: {error}\n{}", peer.stderr());
        }
        if done(f) {
            return;
        }
        assert!(
            phase.elapsed() < PHASE && start.elapsed() < TOTAL,
            "bemenu live gate: {what} not reached\n{}",
            peer.stderr()
        );
        std::thread::sleep(Duration::from_micros(100));
    }
}

#[test]
#[ignore = "live Bemenu gate: needs SOPHIA_BEMENU_ARTIFACT, SOPHIA_BEMENU_SHA256, SOPHIA_BEMENU_COMMIT"]
fn bemenu_executable_serves_the_native_launcher_over_the_production_file_export() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = std::env::temp_dir().join(format!("sophia-bemenu-files-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let scratch = process::Scratch(root);
    std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o700)).unwrap();

    // Identity first: nothing executes before every binding check passes.
    let artifact = artifact::load(&repo, &scratch.0.join("bin"));
    let fonts = sandbox::fonts(&repo, &scratch.0.join("fonts"));
    let mut f = fixture::Fixture::new(&scratch.0);
    let socket = f.socket_path().to_path_buf();
    let mut peer = sandbox::launch(&scratch.0, &artifact.binary, &socket, &fonts);
    let start = Instant::now();
    peer.verify_domain(&artifact.binary, &socket, &fonts);
    f.authorize(peer.evidence());

    // a negotiation; b catalog and output-fact objects; c opening 7.
    until(&mut f, &mut peer, start, "negotiation", |f| f.live());
    f.publish();
    f.open(7);

    // d the peer's allocation and its actual Cairo raster upload.
    until(&mut f, &mut peer, start, "first raster", |f| {
        f.prepared().is_some()
    });
    let first = f.prepared().unwrap();
    assert_eq!((first.opening, first.revision), (7, 1));
    assert_eq!((first.rows.as_slice(), first.selected), (&[1, 2, 3][..], 1));
    for pixels in &first.pixels {
        assert!(
            pixels.iter().any(|b| *b != pixels[0]),
            "uploaded raster is uniform"
        );
    }
    let first_pixels = first.pixels.clone();
    assert_eq!(f.granted.len(), 1);

    // e Prepared -> Presented (scripted) -> the owner's exact focus.
    let focus = f.present();
    assert_eq!(
        (
            focus.opening,
            focus.catalog_generation,
            focus.interaction_generation,
            focus.state_revision,
            focus.allocation
        ),
        (7, 1, 1, 1, f.granted[0])
    );

    // f a text edit: rows and pixels both change in the next candidate.
    f.input(NativeLauncherInputKind::Text, "br");
    until(
        &mut f,
        &mut peer,
        start,
        "text ack and edited raster",
        |f| f.input_acks == 1 && f.prepared().is_some_and(|s| s.revision == 2),
    );
    let edited = f.prepared().unwrap();
    assert_eq!((edited.opening, edited.rows.as_slice()), (7, &[2][..]));
    assert_eq!(edited.selected, 2);
    assert_ne!(
        edited.pixels, first_pixels,
        "text edit left the pixels unchanged"
    );
    let focus = f.present();
    assert_eq!((focus.opening, focus.state_revision), (7, 2));

    // g keyboard Accept: exactly one activation, admitted by scripted Session.
    f.input(NativeLauncherInputKind::Accept, "");
    until(&mut f, &mut peer, start, "keyboard activation", |f| {
        f.input_acks == 2 && !f.activations.is_empty()
    });
    let activation = f.activations[0];
    assert_eq!((activation.cause, activation.slot), (1, 2));
    assert_eq!(activation.event.binding.opening, 7);

    // h close, invalidation and the peer's resource retirement.
    f.close(7);
    until(&mut f, &mut peer, start, "opening 7 settled", |f| {
        f.settled(7)
    });

    // i reopen: a fresh allocation and raster with the query reset.
    f.open(8);
    until(&mut f, &mut peer, start, "reopened raster", |f| {
        f.prepared().is_some_and(|s| s.opening == 8)
    });
    let reopened = f.prepared().unwrap();
    assert_eq!(
        (reopened.revision, reopened.rows.as_slice()),
        (1, &[1, 2, 3][..])
    );
    assert_eq!(reopened.selected, 1);
    assert_eq!(f.granted.len(), 2);
    let focus = f.present();
    assert_eq!((focus.opening, focus.allocation), (8, f.granted[1]));
    f.close(8);
    until(&mut f, &mut peer, start, "opening 8 settled", |f| {
        f.settled(8)
    });

    // j graceful stop, then the owners must hold nothing.
    assert_eq!(
        (
            f.input_acks,
            f.unmatched_acks,
            f.activations.len(),
            f.admissions,
            f.pointer
        ),
        (2, 0, 1, 1, 0)
    );
    let (stdout, stderr) = peer.stop();
    f.cleanup();
    assert!(stdout.is_empty(), "bemenu stdout: {stdout}");
    let lines = stderr.lines().collect::<Vec<_>>();
    let negotiated = format!(
        "bemenu_native status=negotiated revision=7 epoch={} wire=9p",
        fixture::EPOCH
    );
    let announced = lines
        .iter()
        .copied()
        .filter(|l| l.contains("status=negotiated"))
        .collect::<Vec<_>>();
    assert_eq!(announced, [negotiated.as_str()], "{stderr}");
    assert!(!stderr.contains("status=failed"), "{stderr}");
    assert_eq!(
        lines.last(),
        Some(&"bemenu_native status=stopped result=0"),
        "{stderr}"
    );
    println!(
        "sophia_bemenu_files status=pass bemenu={} binary_sha256={} signer={} sdk={} \
         openings=2 candidates={} edits=1 activations=1 fonts=isolated",
        artifact.commit,
        artifact.sha256,
        artifact.signer,
        artifact.sdk,
        f.shown.len()
    );
}
