//! Confined application mapping: parsing, startup refusal, command selection
//! on every Session launch path, and the group socket preparation that comes
//! before any of them. Nothing here starts a session; the only processes
//! spawned are `/usr/bin/true` on the host path, and real Bubblewrap is only
//! asked for its version when present.
use super::*;
use std::collections::BTreeMap;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn parse(values: &[&str]) -> Result<x_groups::XGroups, Box<dyn std::error::Error>> {
    x_groups::XGroups::parse(&args(values))
}

#[test]
fn confined_application_mappings_are_explicit_and_bounded() {
    let groups = parse(&[
        "--confined-group=7",
        "--confined-group=3",
        "--confined-application=browser:7",
        "--confined-application=viewer:3",
    ])
    .unwrap();
    assert_eq!(
        groups.applications,
        BTreeMap::from([("browser".to_owned(), 7), ("viewer".to_owned(), 3)])
    );
    // The group is after the last colon; the application ID is the rest.
    let colon = parse(&["--confined-group=7", "--confined-application=a:b:7"]).unwrap();
    assert_eq!(colon.applications.get("a:b"), Some(&7));
    let refused: &[&[&str]] = &[
        &[
            "--confined-group=7",
            "--confined-application=browser:7",
            "--confined-application=browser:7",
        ],
        &["--confined-group=7", "--confined-application=browser:3"],
        &["--confined-group=7", "--confined-application=browser:0"],
        &["--confined-group=7", "--confined-application=:7"],
        &["--confined-group=7", "--confined-application=browser"],
        &["--confined-group=7", "--confined-application=browser:x"],
        &["--confined-group=7", "--confined-application"],
    ];
    for refused in refused {
        assert!(parse(refused).is_err(), "must refuse {refused:?}");
    }
    // No mapping, no change: the default maps nothing.
    assert!(parse(&[]).unwrap().applications.is_empty());
    let eight = (1..=8)
        .map(|id| format!("--confined-group={id}"))
        .collect::<Vec<_>>();
    assert_eq!(x_groups::XGroups::parse(&eight).unwrap().confined.len(), 8);
}

fn registered(id: &str) -> SessionApplicationSpec {
    SessionApplicationSpec {
        id: id.to_owned(),
        executable: PathBuf::from("/usr/bin/true"),
        arguments: vec!["--fixture".to_owned()],
        placement_classification: None,
    }
}

fn applications(startup: &[&str]) -> SessionApplicationConfig {
    let mut config = SessionApplicationConfig::default();
    for id in ["browser", "editor", "terminal"] {
        config.applications.insert(id.to_owned(), registered(id));
    }
    config.startup = startup.iter().map(|id| (*id).to_owned()).collect();
    config
}

fn catalog(
    applications: &[&str],
    terminal: Option<&str>,
) -> sophia_config::ApplicationCatalogConfig {
    sophia_config::ApplicationCatalogConfig {
        name: "fixture".to_owned(),
        sources: Vec::new(),
        applications: applications.iter().map(|id| (*id).to_owned()).collect(),
        terminal: terminal.map(str::to_owned),
        terminal_arguments: Vec::new(),
    }
}

#[test]
fn a_mapping_no_launch_path_could_confine_is_refused_at_startup() {
    let mapped = parse(&["--confined-group=7", "--confined-application=browser:7"]).unwrap();
    let selected = catalog(&["browser", "editor", "terminal"], Some("terminal"));
    let validate =
        |normal, startup: &[&str], catalog: Option<&sophia_config::ApplicationCatalogConfig>| {
            mapped.validate(normal, &applications(startup), catalog, false)
        };
    // A secondary startup application is launched through the confined path.
    assert!(validate(true, &["editor", "browser"], Some(&selected)).is_ok());
    for (normal, startup, chosen, reason) in [
        (false, &[][..], Some(&selected), "normal"),
        (true, &[][..], None, "catalog"),
        (true, &["browser"][..], Some(&selected), "primary startup"),
    ] {
        let error = validate(normal, startup, chosen).unwrap_err().to_string();
        assert!(error.contains(reason), "{error}");
    }
    let absent = catalog(&["editor"], None);
    assert!(
        validate(true, &[], Some(&absent))
            .unwrap_err()
            .to_string()
            .contains("not in the selected catalog")
    );
    let unregistered = parse(&["--confined-group=7", "--confined-application=ghost:7"]).unwrap();
    let ghost = catalog(&["ghost"], None);
    assert!(
        unregistered
            .validate(true, &applications(&[]), Some(&ghost), false)
            .unwrap_err()
            .to_string()
            .contains("unregistered")
    );
    let terminal = parse(&["--confined-group=7", "--confined-application=terminal:7"]).unwrap();
    assert!(
        terminal
            .validate(true, &applications(&[]), Some(&selected), false)
            .unwrap_err()
            .to_string()
            .contains("catalog terminal")
    );
    // Without a mapping nothing about the catalog or session is required.
    let unmapped = parse(&[]).unwrap();
    assert!(
        unmapped
            .validate(false, &applications(&[]), None, false)
            .is_ok()
    );
    // The XTEST refusal moved here unchanged.
    let xtest = parse(&["--admit-xtest"]).unwrap();
    assert!(
        xtest
            .validate(false, &applications(&[]), None, true)
            .is_err()
    );
    assert!(
        xtest
            .validate(false, &applications(&[]), None, false)
            .is_ok()
    );
}

const DISPLAY: u32 = 47;

/// A short owner-only group layout with a bound socket, and an owner-only
/// cookie. Under the temporary directory, because socket paths are limited
/// to 107 bytes and the target directory is not.
struct GroupFixture {
    root: PathBuf,
    group: PathBuf,
    cookie: PathBuf,
    _listener: std::os::unix::net::UnixListener,
}

impl GroupFixture {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("scx-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let group = root.join("confined-7");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&group)
            .unwrap();
        let socket = group.join(format!("X{DISPLAY}"));
        assert!(socket.as_os_str().len() < 100, "{}", socket.display());
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let cookie = root.join("Xauthority");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&cookie)
            .unwrap();
        Self {
            root,
            group,
            cookie,
            _listener: listener,
        }
    }
}

impl Drop for GroupFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

static GROUP_DIRECTORY: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// The group-directory resolver must be a plain function, so the fixture's
/// directory reaches it through a static.
fn fixture_group(display: u32, group: u32) -> std::io::Result<PathBuf> {
    assert_eq!((display, group), (DISPLAY, 7));
    GROUP_DIRECTORY
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| std::io::Error::other("no fixture group"))
}

fn unresolvable_group(_: u32, _: u32) -> std::io::Result<PathBuf> {
    Err(std::io::Error::other("group directory unavailable"))
}

fn mapping() -> BTreeMap<String, u32> {
    BTreeMap::from([("browser".to_owned(), 7)])
}

fn bubblewrap() -> &'static Path {
    Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH)
}

#[test]
fn only_a_mapped_application_gets_the_confined_command() {
    let fixture = GroupFixture::new("select");
    *GROUP_DIRECTORY.lock().unwrap() = Some(fixture.group.clone());
    let map = mapping();
    let confinement = crate::application_catalog::ApplicationConfinement::new(
        &map,
        bubblewrap(),
        &fixture.cookie,
        Some(DISPLAY),
        fixture_group,
    );
    let true_path = Path::new("/usr/bin/true");
    // Unmapped, or no mapping at all: no confined command, the host path runs.
    assert!(
        confinement
            .command("editor", true_path, &[], None)
            .is_none()
    );
    assert!(
        crate::application_catalog::ApplicationConfinement::default()
            .command("browser", true_path, &[], None)
            .is_none()
    );
    // A working directory is a host path the sandbox cannot honour.
    assert!(matches!(
        confinement.command("browser", true_path, &[], Some(Path::new("/"))),
        Some(Err(_))
    ));
    assert!(
        bubblewrap().is_file(),
        "confined command selection requires Bubblewrap"
    );
    let command = confinement
        .command("browser", true_path, &["--fixture".to_owned()], None)
        .unwrap()
        .unwrap();
    assert_eq!(command.get_program(), bubblewrap());
    // Nothing from the host launch path: no DISPLAY, XAUTHORITY, control or
    // inspection socket set on Bubblewrap, and no host working directory.
    assert_eq!(command.get_envs().count(), 0);
    assert_eq!(command.get_current_dir(), None);
    let argv = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let group = fixture.group.to_string_lossy().into_owned();
    let display = format!(":{DISPLAY}");
    for window in [
        [
            "--ro-bind",
            group.as_str(),
            sophia_runtime::CONFINED_X_SOCKET_DIRECTORY,
        ],
        ["--setenv", "DISPLAY", display.as_str()],
        [
            "--setenv",
            "XAUTHORITY",
            sophia_runtime::CONFINED_X_AUTHORITY_FILE,
        ],
    ] {
        assert!(
            argv.windows(3).any(|w| w == &window[..]),
            "missing {window:?} in {argv:?}"
        );
    }
    assert_eq!(argv.last().map(String::as_str), Some("--fixture"));
}

#[test]
fn a_mapped_application_that_cannot_be_confined_never_launches_on_the_host() {
    let fixture = GroupFixture::new("refuse");
    let map = mapping();
    for confinement in [
        crate::application_catalog::ApplicationConfinement::new(
            &map,
            bubblewrap(),
            &fixture.cookie,
            Some(DISPLAY),
            unresolvable_group,
        ),
        crate::application_catalog::ApplicationConfinement::new(
            &map,
            bubblewrap(),
            &fixture.cookie,
            None,
            fixture_group,
        ),
    ] {
        let command = crate::application_catalog::ApplicationLaunchCommand {
            executable: PathBuf::from("/usr/bin/true"),
            arguments: Vec::new(),
            working_directory: None,
        };
        let environment = || crate::application_catalog::CatalogProcessEnvironment {
            display: ":47",
            xauthority: &fixture.cookie,
            control_socket: None,
            inspection_socket: None,
            confinement,
        };
        // Both catalog paths: a mapped registered identity fails closed ...
        assert!(
            crate::application_catalog::spawn_catalog_process_with_transaction(
                &command,
                environment(),
                Some(1),
                Some("registered:browser"),
            )
            .is_err()
        );
        // ... while an unmapped one, and a scanned desktop entry running the
        // very same executable, take the host path exactly as before.
        for identity in ["registered:editor", "browser.desktop"] {
            let mut child = crate::application_catalog::spawn_catalog_process_with_transaction(
                &command,
                environment(),
                Some(1),
                Some(identity),
            )
            .unwrap();
            assert!(child.wait().unwrap().success());
        }
        // Startup and shortcut launches share spawn_session_application.
        let context = crate::diagnostics::application::LaunchContext {
            source: crate::diagnostics::application::LaunchSource::Shortcut,
            transaction: Some(1),
        };
        let spawn = |app: &SessionApplicationSpec| {
            PersistentXtermSessionConfig::spawn_session_application(
                app,
                ":47",
                &fixture.cookie,
                None,
                None,
                confinement,
                context,
            )
        };
        assert!(spawn(&registered("browser")).is_err());
        assert!(
            spawn(&registered("editor"))
                .unwrap()
                .wait()
                .unwrap()
                .success()
        );
    }
}

#[test]
fn only_a_genuinely_stale_group_socket_is_cleared() {
    let root = std::env::temp_dir().join(format!("scx-stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let path = root.join("X47");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&root)
        .unwrap();
    // Nothing there: nothing to do.
    clear_stale_group_socket(&path).unwrap();
    // A live listener answers and refuses startup; its socket stays.
    let live = std::os::unix::net::UnixListener::bind(&path).unwrap();
    assert!(
        clear_stale_group_socket(&path)
            .unwrap_err()
            .to_string()
            .contains("already active")
    );
    assert!(path.exists());
    // Dropping the listener leaves the file; ECONNREFUSED marks it stale.
    drop(live);
    assert!(path.exists());
    clear_stale_group_socket(&path).unwrap();
    assert!(std::fs::symlink_metadata(&path).is_err());
    // A regular file or a link where the socket goes is refused, untouched.
    std::fs::write(&path, b"").unwrap();
    assert!(
        clear_stale_group_socket(&path)
            .unwrap_err()
            .to_string()
            .contains("not a socket")
    );
    assert!(path.exists());
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink("/tmp/.X11-unix/X0", &path).unwrap();
    assert!(clear_stale_group_socket(&path).is_err());
    assert!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn group_sockets_are_prepared_before_and_confirmed_after_the_frontend_binds() {
    let root = std::env::temp_dir().join(format!("scx-layout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout =
        socket_directories::SocketDirectoryLayout::rooted_at(root.clone(), DISPLAY).unwrap();
    let prepared = prepare_confined_group_sockets(&layout, &[7, 3]).unwrap();
    assert_eq!(
        prepared.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        [7, 3]
    );
    // The frontend's part, played here: bind each group's socket.
    let listeners = prepared
        .iter()
        .map(|(_, path)| std::os::unix::net::UnixListener::bind(path).unwrap())
        .collect::<Vec<_>>();
    let mut server = None;
    verify_confined_group_sockets(&layout, &prepared, &mut server).unwrap();
    // A live group socket refuses a second preparation.
    assert!(prepare_confined_group_sockets(&layout, &[7]).is_err());
    // An extra entry in a group directory refuses confirmation.
    let planted = prepared[1].1.with_file_name("planted");
    std::fs::write(&planted, b"").unwrap();
    assert!(verify_confined_group_sockets(&layout, &prepared, &mut server).is_err());
    std::fs::remove_file(&planted).unwrap();
    // After the listeners are gone their sockets are stale and are cleared.
    drop(listeners);
    // Closing our handles is not proof that the kernel has released every
    // reference: parallel child creation may still be between fork and exec.
    // The production connect probe remains authoritative and must keep refusing
    // a live socket. Bound the fixture's wait for actual staleness.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match prepare_confined_group_sockets(&layout, &[7, 3]) {
            Ok(_) => break,
            Err(error) if error.to_string().ends_with("is already active") => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "closed group sockets must become stale: {error}"
                );
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => panic!("stale group socket preparation failed: {error}"),
        }
    }
    assert!(std::fs::symlink_metadata(&prepared[0].1).is_err());
    std::fs::remove_dir_all(&root).unwrap();
}

/// Every record in an Xauthority file, as (family, address, number, name, data).
type XAuthorityRecord = (u16, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);

fn xauthority_records(bytes: &[u8]) -> Vec<XAuthorityRecord> {
    let mut offset = 0;
    let field = |offset: &mut usize| {
        let len = usize::from(u16::from_be_bytes([bytes[*offset], bytes[*offset + 1]]));
        let value = bytes[*offset + 2..*offset + 2 + len].to_vec();
        *offset += 2 + len;
        value
    };
    let mut records = Vec::new();
    while offset < bytes.len() {
        let family = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
        offset += 2;
        let address = field(&mut offset);
        let number = field(&mut offset);
        let name = field(&mut offset);
        let data = field(&mut offset);
        records.push((family, address, number, name, data));
    }
    records
}

/// The selection rule libxcb and x11rb apply: first MIT-MAGIC-COOKIE-1
/// record whose family and address match (or either is wild, 65535) and
/// whose display number matches (or is empty).
fn select_cookie(bytes: &[u8], host: &[u8], display: u32) -> Option<Vec<u8>> {
    const FAMILY_LOCAL: u16 = 256;
    const FAMILY_WILD: u16 = 65535;
    let display = display.to_string();
    xauthority_records(bytes)
        .into_iter()
        .find(|(family, address, number, name, _)| {
            (*family == FAMILY_WILD || (*family == FAMILY_LOCAL && address == host))
                && (number.is_empty() || number == display.as_bytes())
                && name == b"MIT-MAGIC-COOKIE-1"
        })
        .map(|(.., data)| data)
}

#[test]
fn the_sandbox_hostname_selects_the_same_cookie_as_the_host() {
    let directory = std::env::temp_dir().join(format!("scx-cookie-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let system = rustix::system::uname();
    let host = system.nodename().to_bytes();
    let sandbox = sophia_runtime::PROTECTION_DOMAIN_HOSTNAME.as_bytes();
    assert_ne!(host, sandbox, "the fixture needs two distinct names");
    // Repeats and the host's own name add nothing.
    let (authority, cookie) =
        LiveXAuthorityFile::create_in_with_local_hosts(&directory, 77, &[sandbox, host, sandbox])
            .unwrap();
    let bytes = std::fs::read(authority.path()).unwrap();
    let records = xauthority_records(&bytes);
    assert_eq!(
        records
            .iter()
            .map(|record| record.1.as_slice())
            .collect::<Vec<_>>(),
        [host, sandbox]
    );
    // No wildcard, one display, one cookie.
    assert!(records.iter().all(|(family, _, number, _, data)| {
        *family == 256 && number == b"77" && data == &cookie
    }));
    assert_eq!(
        select_cookie(&bytes, host, 77).as_deref(),
        Some(&cookie[..])
    );
    assert_eq!(
        select_cookie(&bytes, sandbox, 77).as_deref(),
        Some(&cookie[..])
    );
    assert_eq!(select_cookie(&bytes, b"elsewhere", 77), None);
    assert_eq!(select_cookie(&bytes, sandbox, 78), None);
    // The default file is unchanged: the host record alone.
    let (plain, _) = LiveXAuthorityFile::create_in(&directory, 77).unwrap();
    let plain = xauthority_records(&std::fs::read(plain.path()).unwrap());
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].1, host);
    drop(authority);
    std::fs::remove_dir_all(&directory).unwrap();
}
