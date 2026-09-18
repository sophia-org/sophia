//! Real private sockets with independently assembled setup bytes. No display,
//! GPU or synthetic-input grant is constructed by these admission tests.
#![cfg(unix)]

use std::io::{Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sophia_input_authority::InstanceId;
use sophia_protocol::{
    ClientAdmissionContext, ClientAdmissionId, ClientAuthProvenance, ClientAuthenticationMethod,
    NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile,
};
use sophia_x_authority::{
    XServerFrontend, XServerFrontendAdmissionError, XServerFrontendAdmissionPolicy,
    XServerFrontendAdmissionRequest, XServerFrontendConfig, XServerFrontendSetupAuthorization,
};

const PRIVATE_NAME: &[u8] = b"SOPHIA-PRIVATE-INPUT-1";
const COOKIE: [u8; 32] = [0x6a; 32];

#[derive(Clone, Copy)]
enum Order {
    Little,
    Big,
}

impl Order {
    fn put(self, value: u16) -> [u8; 2] {
        match self {
            Self::Little => value.to_le_bytes(),
            Self::Big => value.to_be_bytes(),
        }
    }
    fn get(self, bytes: &[u8]) -> u16 {
        let pair = [bytes[0], bytes[1]];
        match self {
            Self::Little => u16::from_le_bytes(pair),
            Self::Big => u16::from_be_bytes(pair),
        }
    }
}

struct Policy {
    namespace: NamespaceContext,
    requests: Mutex<Vec<XServerFrontendAdmissionRequest>>,
}

impl XServerFrontendAdmissionPolicy for Policy {
    fn admit(
        &self,
        request: XServerFrontendAdmissionRequest,
    ) -> Result<ClientAdmissionContext, XServerFrontendAdmissionError> {
        let mut requests = self.requests.lock().unwrap();
        requests.push(request);
        Ok(ClientAdmissionContext::new(
            ClientAdmissionId::from_raw(u64::try_from(requests.len()).unwrap()),
            self.namespace,
            ClientAuthProvenance::new(request.setup_authentication, 1).unwrap(),
        )
        .unwrap())
    }
    fn revoke(&self, _: ClientAdmissionContext) -> Result<(), XServerFrontendAdmissionError> {
        Ok(())
    }
}

struct PrivateDirectory(PathBuf);
impl PrivateDirectory {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = PathBuf::from(format!(
            "/tmp/sophia-private-setup-{}-{unique}",
            std::process::id()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn read_exact(stream: &mut UnixStream, bytes: &mut [u8], deadline: Instant) {
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .expect("absolute setup deadline");
        stream.set_read_timeout(Some(remaining)).unwrap();
        let count = stream.read(&mut bytes[offset..]).unwrap();
        assert_ne!(count, 0, "setup ended before its mandatory record");
        offset += count;
    }
}

fn setup(order: Order, name: &[u8], data: &[u8]) -> Vec<u8> {
    let mut result = vec![
        match order {
            Order::Little => b'l',
            Order::Big => b'B',
        },
        0,
    ];
    for value in [
        11,
        0,
        u16::try_from(name.len()).unwrap(),
        u16::try_from(data.len()).unwrap(),
        0,
    ] {
        result.extend_from_slice(&order.put(value));
    }
    result.extend_from_slice(name);
    result.resize(result.len().next_multiple_of(4), 0);
    result.extend_from_slice(data);
    result.resize(result.len().next_multiple_of(4), 0);
    result
}

fn exchange(
    authorization: XServerFrontendSetupAuthorization,
    order: Order,
    attempts: &[(&[u8], &[u8], bool)],
) -> Vec<XServerFrontendAdmissionRequest> {
    let directory = PrivateDirectory::new();
    let socket = directory.0.join("authority.sock");
    let namespace = NamespaceContext::new(
        NamespaceId::from_raw(945),
        NamespaceProfile::ClassicShared,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let policy = Arc::new(Policy {
        namespace,
        requests: Mutex::new(Vec::new()),
    });
    let config = XServerFrontendConfig::new_with_namespace_context(&socket, namespace)
        .unwrap()
        .with_setup_authorization(authorization)
        .with_admission_policy(policy.clone());
    let mut frontend = XServerFrontend::bind(config).unwrap();
    let count = attempts.len();
    let (completed, receive) = mpsc::sync_channel(1);
    let server = std::thread::spawn(move || {
        let result = (0..count).try_for_each(|_| frontend.serve_next());
        let _ = completed.send(result);
    });
    let mut admitted = 0;
    for &(name, data, success) in attempts {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(&setup(order, name, data)).unwrap();
        let mut prefix = [0u8; 8];
        read_exact(&mut stream, &mut prefix, deadline);
        assert_eq!(prefix[0], u8::from(success), "unexpected setup status");
        assert_eq!(order.get(&prefix[2..4]), 11);
        let mut body = vec![0; usize::from(order.get(&prefix[6..8])) * 4];
        read_exact(&mut stream, &mut body, deadline);
        if success {
            admitted += 1;
            // Admission still supplies a healthy ordinary X connection. This
            // request is assembled without Sophia's encoder or decoder.
            let length = order.put(1);
            stream.write_all(&[43, 0, length[0], length[1]]).unwrap();
            let mut reply = [0u8; 32];
            read_exact(&mut stream, &mut reply, deadline);
            assert_eq!(reply[0], 1);
            assert_eq!(order.get(&reply[2..4]), 1);
        } else {
            assert!(
                body.windows(b"authorization failed".len())
                    .any(|part| part == b"authorization failed")
            );
            stream
                .set_read_timeout(Some(
                    deadline.checked_duration_since(Instant::now()).unwrap(),
                ))
                .unwrap();
            assert_eq!(
                stream.read(&mut [0u8; 1]).unwrap(),
                0,
                "setup refusal must close the connection"
            );
        }
        assert_eq!(
            policy.requests.lock().unwrap().len(),
            admitted,
            "failed setup reached admission policy"
        );
    }
    receive
        .recv_timeout(Duration::from_secs(3))
        .expect("private server completion deadline")
        .unwrap();
    server.join().unwrap();
    policy.requests.lock().unwrap().clone()
}

fn private(instance: u64, cookie: [u8; 32]) -> XServerFrontendSetupAuthorization {
    XServerFrontendSetupAuthorization::PrivateInputCookie {
        instance: InstanceId::new(instance),
        cookie,
    }
}

#[test]
fn private_setup_anonymous_remains_ordinary_in_both_orders() {
    for order in [Order::Little, Order::Big] {
        let requests = exchange(private(701, COOKIE), order, &[(b"", b"", true)]);
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].setup_authentication,
            ClientAuthenticationMethod::TrustedLocal
        );
        assert!(requests[0].verified_private_input.is_none());
        assert!(requests[0].peer_credentials.is_some());
    }
}

#[test]
fn private_setup_cookie_verifies_configured_instance_in_both_orders() {
    for order in [Order::Little, Order::Big] {
        let requests = exchange(
            private(702, COOKIE),
            order,
            &[(PRIVATE_NAME, &COOKIE, true)],
        );
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].setup_authentication,
            ClientAuthenticationMethod::TrustedLocal
        );
        assert_eq!(
            requests[0].verified_private_input.unwrap().instance(),
            InstanceId::new(702)
        );
        let evidence = format!("{:?}", requests[0].verified_private_input.unwrap());
        assert_eq!(
            evidence,
            "XServerFrontendVerifiedPrivateInputAuthorization { instance: InstanceId(702) }"
        );
    }
}

#[test]
fn private_setup_supplied_wrong_or_partial_credentials_never_reach_policy() {
    let mut wrong = COOKIE;
    wrong[31] ^= 1;
    for order in [Order::Little, Order::Big] {
        let requests = exchange(
            private(703, COOKIE),
            order,
            &[
                (b"SOPHIA-PRIVATE-INPUT-2", &COOKIE, false),
                (PRIVATE_NAME, &wrong, false),
                (b"", &COOKIE, false),
                (PRIVATE_NAME, b"", false),
                (PRIVATE_NAME, &COOKIE[..31], false),
                (b"MIT-MAGIC-COOKIE-1", &COOKIE, false),
                (PRIVATE_NAME, &COOKIE, true),
                (b"", b"", true),
            ],
        );
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].verified_private_input.unwrap().instance(),
            InstanceId::new(703)
        );
        assert!(requests[1].verified_private_input.is_none());
    }
}

#[test]
fn private_setup_instance_binding_comes_from_configuration() {
    for order in [Order::Little, Order::Big] {
        let other_cookie = [0x3c; 32];
        let first = exchange(
            private(704, COOKIE),
            order,
            &[(PRIVATE_NAME, &COOKIE, true)],
        );
        let other = exchange(
            private(705, other_cookie),
            order,
            &[
                (PRIVATE_NAME, &COOKIE, false),
                (PRIVATE_NAME, &other_cookie, true),
            ],
        );
        assert_eq!(
            first[0].verified_private_input.unwrap().instance(),
            InstanceId::new(704)
        );
        assert_eq!(
            other[0].verified_private_input.unwrap().instance(),
            InstanceId::new(705)
        );
    }
}

#[test]
fn strict_mit_cookie_does_not_gain_anonymous_fallback() {
    let cookie = [0x5e; 16];
    for order in [Order::Little, Order::Big] {
        let requests = exchange(
            XServerFrontendSetupAuthorization::MitMagicCookie(cookie),
            order,
            &[
                (b"", b"", false),
                (b"", &cookie, false),
                (b"MIT-MAGIC-COOKIE-1", b"", false),
                (PRIVATE_NAME, &COOKIE, false),
                (b"MIT-MAGIC-COOKIE-1", &cookie, true),
            ],
        );
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].setup_authentication,
            ClientAuthenticationMethod::MitMagicCookie1
        );
        assert!(requests[0].verified_private_input.is_none());
    }
}

#[test]
fn ordinary_setup_does_not_convert_claimed_private_name_into_verified_authorization() {
    for order in [Order::Little, Order::Big] {
        let requests = exchange(
            XServerFrontendSetupAuthorization::UnauthenticatedLocal,
            order,
            &[
                (PRIVATE_NAME, &COOKIE, true),
                (PRIVATE_NAME, b"", true),
                (b"", b"", true),
            ],
        );
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().all(|r| r.verified_private_input.is_none()
            && r.setup_authentication == ClientAuthenticationMethod::TrustedLocal));
    }
}

#[test]
fn private_setup_debug_redacts_cookie() {
    assert_eq!(
        format!("{:?}", private(706, COOKIE)),
        "PrivateInputCookie { instance: InstanceId(706), cookie: \"[redacted]\" }"
    );
}
