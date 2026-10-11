// Wire-level fallback and descriptor sharing; no application or display device.
#[cfg(target_os = "linux")]
fn shm_admission_client(path: &std::path::Path, shmid: u32, refused: bool) {
    use std::io::{IoSlice, Write};
    use std::mem::MaybeUninit;
    use std::os::fd::AsFd;
    let order = XByteOrder::LittleEndian;
    let mut client = connect_x_socket(path);
    client
        .write_all(&setup_request(order, 11, 0, b"", b""))
        .unwrap();
    read_setup_success(&mut client, order);
    let segment = X_SETUP_DEFAULT_RESOURCE_ID_BASE + 1;
    client
        .write_all(&mit_shm_attach_request(order, segment, shmid, false))
        .unwrap();
    client
        .write_all(&mit_shm_query_version_request(order))
        .unwrap();
    let reply = read_x_reply(&mut client, order);
    if refused {
        assert_eq!(
            reply[0], 0,
            "inaccessible or confined SysV attach must fail before drawing"
        );
        assert_eq!(reply[1], XErrorCode::BadAccess.wire_code());
        assert_eq!(read_u16(order, &reply[2..4]), 1);
        assert_eq!(
            read_u16(order, &reply[8..10]),
            u16::from(X_MIT_SHM_ATTACH_MINOR_OPCODE)
        );
        assert_eq!(reply[10], X_MIT_SHM_MAJOR_OPCODE);
        assert_eq!(read_u32(order, &reply[4..8]), segment);
        assert_eq!(read_x_reply(&mut client, order)[0], 1);
    } else {
        assert_eq!(reply[0], 1, "shared accessible SysV attach must succeed");
        assert_eq!(read_u16(order, &reply[2..4]), 2);
        client
            .write_all(&mit_shm_detach_request(order, segment))
            .unwrap();
    }
    // A refused attachment did not claim the XID. Passing a real descriptor
    // with that same XID must work, including across the IPC namespace boundary.
    let (_mapping, fd) = sophia_sysv_shm::DescriptorMapping::create_sealed(4096).unwrap();
    let request = mit_shm_attach_fd_request(order, segment, false);
    let borrowed = [fd.as_fd()];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut ancillary = rustix::net::SendAncillaryBuffer::new(&mut space);
    assert!(ancillary.push(rustix::net::SendAncillaryMessage::ScmRights(&borrowed)));
    assert_eq!(
        rustix::net::sendmsg(
            &client,
            &[IoSlice::new(&request)],
            &mut ancillary,
            rustix::net::SendFlags::empty()
        )
        .unwrap(),
        request.len()
    );
    client
        .write_all(&mit_shm_detach_request(order, segment))
        .unwrap();
    client
        .write_all(&mit_shm_query_version_request(order))
        .unwrap();
    assert_eq!(
        read_x_reply(&mut client, order)[0],
        1,
        "descriptor attach and detach must survive SysV refusal"
    );

    // Ordinary core PutImage remains usable after refusal. Verify actual pixels.
    let pixmap = segment + 1;
    let gc = segment + 2;
    client
        .write_all(&create_pixmap_request(
            order,
            24,
            pixmap,
            X_SETUP_DEFAULT_ROOT,
            2,
            2,
        ))
        .unwrap();
    client
        .write_all(&create_gc_request(order, gc, pixmap))
        .unwrap();
    let pixels = [
        0x31, 0x52, 0x73, 0, 0x31, 0x52, 0x73, 0, 0x31, 0x52, 0x73, 0, 0x31, 0x52, 0x73, 0,
    ];
    client
        .write_all(&put_image_request(
            order,
            pixmap,
            gc,
            PutImageGeometry {
                width: 2,
                height: 2,
                dst_x: 0,
                dst_y: 0,
            },
            &pixels,
        ))
        .unwrap();
    client
        .write_all(&get_image_request(order, 2, pixmap, 0, 0, 2, 2, u32::MAX))
        .unwrap();
    let reply = read_x_reply(&mut client, order);
    assert_eq!(reply[0], 1, "fallback GetImage must succeed");
    assert_eq!(
        &reply[32..],
        &pixels,
        "core fallback must retain the drawn pixels"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn shm_attach_refusal_preserves_core_and_descriptor_fallbacks() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let host_segment = sysv_segment::Segment::new(29);
    for (profile, private_ipc, shmid, refused) in [
        (
            NamespaceProfile::ClassicShared,
            false,
            host_segment.0,
            false,
        ),
        (NamespaceProfile::ClassicShared, false, u32::MAX, true),
        // Even a valid host ID is not authority for a confined client.
        (NamespaceProfile::Confined, false, host_segment.0, true),
        (NamespaceProfile::Confined, true, host_segment.0, true),
    ] {
        let path = std::env::temp_dir().join(format!(
            "shm-admit-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let namespace = NamespaceContext::new(
            NamespaceId::from_raw(833),
            profile,
            NamespaceCapabilities::NONE,
        )
        .unwrap();
        let config = XServerFrontendConfig::new_with_namespace_context(&path, namespace)
            .unwrap()
            .with_admission_policy(Arc::new(TestXAdmissionPolicy::new(namespace, false)));
        // Bind before spawning so failures cannot leave a child waiting for setup.
        let mut frontend = XServerFrontend::bind(config).unwrap();
        let server = std::thread::spawn(move || frontend.serve_next().unwrap());
        if private_ipc {
            let output = std::process::Command::new("/usr/bin/bwrap")
                .args([
                    "--unshare-ipc",
                    "--ro-bind",
                    "/",
                    "/",
                    "--dev",
                    "/dev",
                    "--",
                ])
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "shm_private_ipc_child",
                    "--ignored",
                    "--nocapture",
                ])
                .env("SOPHIA_SHM_TEST_SOCKET", &path)
                .env(
                    "SOPHIA_SHM_PARENT_IPC",
                    std::fs::read_link("/proc/self/ns/ipc").unwrap(),
                )
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "private IPC child failed: {output:?}"
            );
            assert!(
                String::from_utf8_lossy(&output.stdout)
                    .contains("private IPC fallback pixels verified")
            );
        } else {
            shm_admission_client(&path, shmid, refused);
        }
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "invoked by shm_attach_refusal_preserves_core_and_descriptor_fallbacks"]
fn shm_private_ipc_child() {
    let path = std::env::var_os("SOPHIA_SHM_TEST_SOCKET").expect("parent socket");
    assert_ne!(
        std::fs::read_link("/proc/self/ns/ipc").unwrap().as_os_str(),
        std::env::var_os("SOPHIA_SHM_PARENT_IPC").unwrap()
    );
    let private_segment = sysv_segment::Segment::new(91);
    shm_admission_client(std::path::Path::new(&path), private_segment.0, true);
    println!("private IPC fallback pixels verified");
}
