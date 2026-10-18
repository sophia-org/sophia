//! Supplied readback records only: no DRM device or real GL context is used.
use super::super::{
    OutputEnvironmentBatch, environment_percent_encode, output_environment_batch,
    output_environment_lines, send_output_environment,
};
use sophia_backend_live::{
    LiveProductionOutputEnvironmentReadback, LiveProductionOutputKmsReadback,
};
use sophia_engine::RenderHeadId;
use sophia_protocol::OutputId;
use sophia_renderer_native_egl::{NativeGlIdentity, NativeGlIdentityState};

fn record() -> LiveProductionOutputEnvironmentReadback {
    LiveProductionOutputEnvironmentReadback {
        kms: LiveProductionOutputKmsReadback {
            head: RenderHeadId::from_raw(1),
            card: 0,
            connector: 10,
            crtc: 20,
            plane: 30,
            mode: Some([
                1920, 1080, 120, 296951, 2008, 2052, 2200, 1084, 1089, 1125, 0, 0, 0,
            ]),
            properties: [("crtc.ACTIVE".into(), 1), ("crtc.VRR_ENABLED".into(), 0)].into(),
        },
        output: OutputId::from_raw(3),
        card_node: Some("card1".into()),
        connector_name: Some("DP-2".into()),
        driver: Some("amdgpu".into()),
        nominal_refresh_millihz: Some(120000),
        timing_refresh_millihz: Some(119980),
        applied_scale_milli: Some(1000),
        vrr_enabled: Some(false),
        gl_identity: NativeGlIdentityState::Observed(std::sync::Arc::new(NativeGlIdentity {
            vendor: "Mesa".into(),
            renderer: "AMD (GPU)".into(),
            version: "4.6 Mesa 25.2".into(),
        })),
    }
}

#[test]
fn environment_datagrams_preserve_records_and_refuse_missing_receivers() {
    use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};
    use std::os::unix::net::UnixDatagram;
    let directory = std::env::temp_dir().join(format!("sophia-env-send-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("environment.sock");
    let receiver = UnixDatagram::bind(&path).unwrap();
    rustix::net::sockopt::set_socket_passcred(&receiver, true).unwrap();
    receiver
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    let lines = output_environment_lines(&record(), std::process::id(), 123).unwrap();
    send_output_environment(&path, std::slice::from_ref(&lines)).unwrap();
    for expected in &lines {
        let mut bytes = [0; 4096];
        let mut space =
            [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmCredentials(1), ScmRights(1))];
        let mut control = RecvAncillaryBuffer::new(&mut space);
        let message = recvmsg(
            &receiver,
            &mut [std::io::IoSliceMut::new(&mut bytes)],
            &mut control,
            RecvFlags::CMSG_CLOEXEC,
        )
        .unwrap();
        assert!(
            !message
                .flags
                .intersects(rustix::net::ReturnFlags::TRUNC | rustix::net::ReturnFlags::CTRUNC)
        );
        assert_eq!(&bytes[..message.bytes], expected.as_bytes());
        let mut credentials = None;
        let mut senders = Vec::new();
        for item in control.drain() {
            match item {
                RecvAncillaryMessage::ScmCredentials(value) => {
                    assert!(credentials.replace(value).is_none());
                }
                RecvAncillaryMessage::ScmRights(rights) => senders.extend(rights),
                _ => panic!("environment writer sent unexpected ancillary data"),
            }
        }
        let credentials = credentials.expect("kernel sender credentials");
        assert_eq!(credentials.pid.as_raw_pid() as u32, std::process::id());
        assert_eq!(credentials.uid, rustix::process::getuid());
        assert_eq!(credentials.gid, rustix::process::getgid());
        assert_eq!(senders.len(), 1);
        use std::os::fd::AsRawFd;
        let number = senders[0].as_raw_fd();
        assert_eq!(
            std::fs::read_link(format!("/proc/self/fd/{number}")).unwrap(),
            std::path::Path::new("anon_inode:[pidfd]")
        );
        let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{number}")).unwrap();
        assert!(info.lines().any(|line| {
            line.strip_prefix("Pid:")
                .is_some_and(|p| p.trim() == std::process::id().to_string())
        }));
    }
    let invalid = [lines[0].clone(), "invalid\nrecord".into()];
    assert!(send_output_environment(&path, &[invalid]).is_err());
    receiver.set_nonblocking(true).unwrap();
    assert_eq!(
        receiver.recv(&mut [0; 4096]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "a later invalid line must not publish an earlier valid line"
    );
    assert!(send_output_environment(std::path::Path::new("relative.sock"), &[]).is_err());
    assert!(send_output_environment(&directory.join("missing"), &[lines]).is_err());
    drop(receiver);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn environment_records_preserve_programmed_timing_and_context_strings() {
    let lines = output_environment_lines(&record(), 4100, 123456).unwrap();
    assert_eq!(
        lines[0],
        "sophia_shell_environment_output schema=1 pid=4100 output=3 card=card1 connector=DP-2 width=1920 height=1080 refresh_millihz=120000 timing_refresh_millihz=119980 scale_milli=1000 vrr=off monotonic_usec=123456"
    );
    assert_eq!(
        lines[1],
        "sophia_shell_environment_renderer schema=1 pid=4100 output=3 card=card1 driver=amdgpu vendor=Mesa renderer=AMD%20%28GPU%29 version=4.6%20Mesa%2025.2 monotonic_usec=123456"
    );
    assert_eq!(environment_percent_encode("é%\n~"), "%C3%A9%25%0A~");
}

#[test]
fn environment_records_never_replace_missing_facts_with_policy() {
    for mutation in 0..12 {
        let mut r = record();
        match mutation {
            0 => r.card_node = None,
            1 => r.connector_name = None,
            2 => r.driver = None,
            3 => r.nominal_refresh_millihz = None,
            4 => r.timing_refresh_millihz = None,
            5 => r.applied_scale_milli = None,
            6 => r.vrr_enabled = None,
            7 => r.gl_identity = Default::default(),
            8 => r.kms.mode = None,
            9 => r.connector_name = Some("DP 2".into()),
            10 => r.applied_scale_milli = Some(0),
            _ => r.gl_identity.observe(std::sync::Arc::new(NativeGlIdentity {
                vendor: "another vendor".into(),
                renderer: "GPU".into(),
                version: "4.6".into(),
            })),
        }
        assert!(
            output_environment_lines(&r, 4100, 123456).is_none(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn environment_waits_for_every_active_context_without_publishing_a_partial_set() {
    let first = record();
    let mut second = record();
    second.kms.head = RenderHeadId::from_raw(2);
    second.output = OutputId::from_raw(4);
    second.gl_identity = Default::default();
    assert!(matches!(
        output_environment_batch(&[first.clone(), second.clone()], 4100, 1),
        OutputEnvironmentBatch::Pending
    ));
    second.gl_identity = first.gl_identity.clone();
    match output_environment_batch(&[first.clone(), second.clone()], 4100, 1) {
        OutputEnvironmentBatch::Ready(lines) => assert_eq!(lines.len(), 2),
        _ => panic!("both complete contexts must publish"),
    }
    second.vrr_enabled = None;
    assert!(matches!(
        output_environment_batch(&[first, second], 4100, 1),
        OutputEnvironmentBatch::Unavailable(_)
    ));
}
