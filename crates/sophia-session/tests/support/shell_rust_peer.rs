//! Protected child: the independent Rust SDK owns authentication and shell wire.
use sophia_launch_client::Client;
use sophia_shell_client::{ShellClientOptions, ShellConnection};
use std::{os::unix::net::UnixStream, time::Duration};

#[test]
#[ignore = "parent invokes this child under the protected shell supervisor"]
fn protected_shell_child() {
    let socket = std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap();
    let report = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap());
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let role = UnixStream::connect(socket).unwrap();
    let name = std::env::var("SOPHIA_TEST_SHELL_ROLE").unwrap_or_else(|_| "descriptor".into());
    let (revision, capabilities) = match name.as_str() {
        "descriptor" => (8, 1),
        "bar" => (6, 0x81),
        "launcher" => (7, 0x9a0),
        "dock" => (8, 0x11a2),
        _ => panic!("unknown test role"),
    };
    let mut auth = Client::over(role, relay, b"shell", name.as_bytes()).unwrap();
    while !auth.ready() {
        auth.poll()
            .expect("Rust SDK shell authentication must succeed");
        auth.wait_for_io(Duration::from_millis(10)).unwrap();
    }
    let admitted = auth.into_authenticated().unwrap();
    let connection = ShellConnection::from_attached(
        admitted.role,
        admitted.root,
        ShellClientOptions {
            minimum_revision: revision,
            maximum_revision: revision,
            required_capabilities: capabilities,
            handshake_timeout: Duration::from_secs(5),
        },
    )
    .expect("Rust SDK must continue on the authenticated shell root");
    let temporary = report.with_extension("tmp");
    std::fs::write(
        &temporary,
        format!("{name} ready {}\n", connection.connection_epoch()),
    )
    .unwrap();
    std::fs::rename(temporary, report).unwrap();
    // Preserve both owners until Session revokes and reaps this process.
    std::thread::sleep(Duration::from_secs(10));
    drop(connection);
    drop(admitted.relay);
}
