//! Test-owned service process shared by runtime and independent SDK crash gates.
use super::support::*;

pub fn run() {
    let root = std::path::PathBuf::from(std::env::var_os("SOPHIA_ADMIN_CRASH_ROOT").unwrap());
    let service = ControlService::bind_files(&root).unwrap();
    until(|| service.publish(Arc::new(catalog(7)), &[]));
    std::fs::write(
        root.join("endpoint.tmp"),
        service.socket_path().as_os_str().as_encoded_bytes(),
    )
    .unwrap();
    std::fs::rename(root.join("endpoint.tmp"), root.join("endpoint")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let ticket = loop {
        if let Some(ticket) = service.try_request() {
            break ticket;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(ticket.command, action());
    if std::env::var("SOPHIA_ADMIN_CRASH_DISPATCH").as_deref() == Ok("1") {
        claim(&ticket);
        // A durable fixture marker represents an effect before a reply is visible.
        std::fs::write(root.join("effect"), b"one effect\n").unwrap();
    }
    std::fs::write(root.join("accepted"), b"ready\n").unwrap();
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}
