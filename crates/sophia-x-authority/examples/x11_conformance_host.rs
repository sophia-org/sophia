//! Private, software-only production frontend for external protocol clients.
//! No session, renderer, DRM, physical input, or operator display is opened.
//!
//! `--admit-xtest` additionally issues an XTEST injector to clients admitted
//! into this host's one namespace. It is off by default and has to be asked
//! for, because admitting synthetic input is a change of posture rather than a
//! convenience: without it XTEST is absent from discovery and every opcode
//! answers BadAccess, which is what an ordinary conformance run should see.
//!
//! What it buys is a suite purpose that needs a keyboard. Several XTS purposes
//! check where a keyboard event actually lands -- that a key goes to the focus
//! window rather than merely that the focus request was answered -- and no
//! amount of protocol traffic stands in for one. Those purposes cannot run
//! against a server that refuses to synthesise input, and they are the only
//! ones in the selection that test delivery against the focus contract.
use sophia_protocol::{
    ClientAdmissionContext, ClientAdmissionId, ClientAuthProvenance, DeviceId,
    NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile, SeatId,
};
use sophia_x_authority::{
    RoutedXTestInjector, X_AUTHORITY_OBSERVED_TRANSACTION_CHANNEL_CAPACITY,
    XAuthorityRoutedInputSender, XServerFrontendAdmissionError, XServerFrontendAdmissionPolicy,
    XServerFrontendAdmissionRequest, XServerFrontendConfig, XServerFrontendInjectionError,
    XServerFrontendInjectionPolicy, XServerFrontendRouteBroker, XTestInjector,
    run_x_server_frontend_routed_until_stopped,
};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc;

/// The namespace every client on this host is admitted into.
const HOST_NAMESPACE: u64 = 1;
/// Not the seat's physical keyboard or pointer, so injected events are
/// attributable to the injector rather than indistinguishable from hardware.
const XTEST_DEVICE: u64 = 3;

/// Issues an injector to clients of this host's namespace, and to no others.
///
/// The namespace test is not decoration on a host that only has one. It is the
/// same rule the live policy applies, written the same way, so that a second
/// namespace arriving later is denied by default rather than admitted by an
/// omission nobody revisits.
struct HostXTestInjectionPolicy {
    admitted: NamespaceId,
    sender: XAuthorityRoutedInputSender,
}

impl XServerFrontendInjectionPolicy for HostXTestInjectionPolicy {
    fn issue(
        &self,
        context: ClientAdmissionContext,
        _device: DeviceId,
    ) -> Result<Box<dyn XTestInjector>, XServerFrontendInjectionError> {
        if context.namespace.id != self.admitted {
            return Err(XServerFrontendInjectionError::Denied);
        }
        Ok(Box::new(RoutedXTestInjector::new(
            self.sender.clone(),
            SeatId::from_raw(1),
            DeviceId::from_raw(XTEST_DEVICE),
        )))
    }
}

/// Admits every client of this host into its one namespace, with a lease.
///
/// The lease is the point. An injector is issued only where an admission lease
/// and an injection policy meet, so a host that never leases cannot inject
/// however it is configured -- which is why naming the injection policy alone
/// left XTEST absent from discovery and every opcode still answering BadAccess.
struct HostAdmitOneNamespace {
    namespace: NamespaceContext,
    next_client: std::sync::atomic::AtomicU64,
}

impl XServerFrontendAdmissionPolicy for HostAdmitOneNamespace {
    fn admit(
        &self,
        request: XServerFrontendAdmissionRequest,
    ) -> Result<ClientAdmissionContext, XServerFrontendAdmissionError> {
        let index = self
            .next_client
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ClientAdmissionContext::new(
            ClientAdmissionId::from_raw(index + 1),
            self.namespace,
            ClientAuthProvenance::new(request.setup_authentication, 1)
                .ok_or(XServerFrontendAdmissionError::Unavailable)?,
        )
        .ok_or(XServerFrontendAdmissionError::Unavailable)
    }

    fn revoke(
        &self,
        _context: ClientAdmissionContext,
    ) -> Result<(), XServerFrontendAdmissionError> {
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args
        .next()
        .ok_or("usage: x11_conformance_host PRIVATE_SOCKET [--admit-xtest]")?;
    let mut admit_xtest = false;
    for argument in args {
        if argument == "--admit-xtest" {
            admit_xtest = true;
        } else {
            return Err("usage: x11_conformance_host PRIVATE_SOCKET [--admit-xtest]".into());
        }
    }
    let namespace = NamespaceContext::new(
        NamespaceId::from_raw(HOST_NAMESPACE),
        NamespaceProfile::ClassicShared,
        NamespaceCapabilities::NONE,
    )
    .ok_or("invalid namespace")?;
    // Production bounded worker admission and shared protocol state. The runner
    // owns the process lifetime and enforces an absolute deadline externally.
    let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(64).ok_or("zero queue")?);
    // No window manager runs against this host, and the suites it serves
    // (XTS among them) assume the reference server without one: a client
    // places its own toplevels, so a move to (0, 0) before drawing on the
    // root lands where the test expects (t189).
    let mut config = XServerFrontendConfig::new_with_namespace_context(path, namespace)?
        .with_client_toplevel_placement(true);
    if admit_xtest {
        config = config
            .with_admission_policy(Arc::new(HostAdmitOneNamespace {
                namespace,
                next_client: std::sync::atomic::AtomicU64::new(0),
            }))
            .with_injection_policy(Arc::new(HostXTestInjectionPolicy {
                admitted: NamespaceId::from_raw(HOST_NAMESPACE),
                sender: broker.routed_input_sender(),
            }));
    }
    // The production service loop, not a hand-rolled accept loop: it shares
    // the broker's input authority with the runtime and drives the broker's
    // routing, which is what turns an admitted XTEST injection (or a warp
    // through one) into delivered events. The host's own loop accepted and
    // reaped workers and never routed, so every injection waited for a
    // completion nobody could produce, and the injecting client hung with
    // it. Observed transactions are drained and discarded: no session sits
    // behind this host to receive them, and a full channel would otherwise
    // block dispatch.
    let (transactions, observed) =
        mpsc::sync_channel(X_AUTHORITY_OBSERVED_TRANSACTION_CHANNEL_CAPACITY);
    std::thread::spawn(move || while observed.recv().is_ok() {});
    // Held for the process's lifetime: a dropped sender reads as
    // StopAndDisconnect, and the runner ends this host by signal.
    let (_service_commands, commands) = mpsc::sync_channel(1);
    run_x_server_frontend_routed_until_stopped(config, transactions, broker, commands)?;
    Ok(())
}
