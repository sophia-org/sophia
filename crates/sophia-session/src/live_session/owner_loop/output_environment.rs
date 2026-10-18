// These records describe one completed readback. Requested output policy is
// never a substitute for a missing kernel property or renderer identity.
fn output_environment_lines(
    record: &sophia_backend_live::LiveProductionOutputEnvironmentReadback,
    pid: u32,
    monotonic_usec: u64,
) -> Option<[String; 2]> {
    let mode = record.kms.mode?;
    let card = record.card_node.as_deref()?;
    let connector = record.connector_name.as_deref()?;
    let driver = record.driver.as_deref()?;
    let nominal = record.nominal_refresh_millihz?;
    let timing = record.timing_refresh_millihz?;
    let scale = record.applied_scale_milli?;
    let vrr = if record.vrr_enabled? { "on" } else { "off" };
    let gl = record.gl_identity.observed()?;
    let name = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    };
    if pid == 0
        || mode[0] == 0
        || mode[1] == 0
        || nominal == 0
        || timing == 0
        || scale == 0
        || ![card, connector, driver].into_iter().all(name)
        || [&gl.vendor, &gl.renderer, &gl.version]
            .into_iter()
            .any(|s| s.is_empty())
    {
        return None;
    }
    let output = record.output.raw();
    Some([
        format!(
            "sophia_shell_environment_output schema=1 pid={pid} output={output} card={card} connector={connector} width={} height={} refresh_millihz={nominal} timing_refresh_millihz={timing} scale_milli={scale} vrr={vrr} monotonic_usec={monotonic_usec}",
            mode[0], mode[1],
        ),
        format!(
            "sophia_shell_environment_renderer schema=1 pid={pid} output={output} card={card} driver={driver} vendor={} renderer={} version={} monotonic_usec={monotonic_usec}",
            environment_percent_encode(&gl.vendor),
            environment_percent_encode(&gl.renderer),
            environment_percent_encode(&gl.version),
        ),
    ])
}

fn environment_percent_encode(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            // Writing to String is infallible; uppercase encoding is canonical.
            write!(&mut encoded, "%{byte:02X}").expect("format into String");
        }
    }
    encoded
}

/// The opt-in campaign receiver authenticates Linux sender credentials on each
/// datagram. Child text in the shared diagnostic log cannot substitute for
/// this delivery. Nonblocking sends fail the proof rather than stall the owner
/// or silently lose part of the environment set.
fn send_output_environment(
    destination: &std::path::Path,
    lines: &[[String; 2]],
) -> std::io::Result<()> {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixDatagram;
    if !destination.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "environment socket path must be absolute",
        ));
    }
    for line in lines.iter().flatten() {
        if line.is_empty() || line.len() > 65_536 || line.contains(['\r', '\n', '\0']) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "environment record exceeds datagram bounds",
            ));
        }
    }
    let socket = UnixDatagram::unbound()?;
    socket.set_nonblocking(true)?;
    socket.connect(destination)?;
    // A TGID pidfd ties queued datagrams to this process even after numeric
    // PID reuse. If unavailable, fail this opt-in proof; never send bare text.
    let sender = rustix::process::pidfd_open(
        rustix::process::getpid(),
        rustix::process::PidfdFlags::empty(),
    )?;
    for line in lines.iter().flatten() {
        loop {
            let rights = [sender.as_fd()];
            let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
            let mut control = SendAncillaryBuffer::new(&mut space);
            if !control.push(SendAncillaryMessage::ScmRights(&rights)) {
                return Err(std::io::Error::other("environment pidfd control buffer"));
            }
            match sendmsg(
                &socket,
                &[std::io::IoSlice::new(line.as_bytes())],
                &mut control,
                SendFlags::NOSIGNAL,
            ) {
                Ok(count) if count == line.len() => break,
                Ok(_) => return Err(std::io::Error::other("short environment datagram")),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

enum OutputEnvironmentBatch {
    Pending,
    Unavailable(sophia_engine::RenderHeadId),
    Ready(Vec<[String; 2]>),
}

fn output_environment_batch(
    records: &[sophia_backend_live::LiveProductionOutputEnvironmentReadback],
    pid: u32,
    usec: u64,
) -> OutputEnvironmentBatch {
    let active: Vec<_> = records.iter().filter(|r| r.kms.mode.is_some()).collect();
    if active.is_empty() {
        return OutputEnvironmentBatch::Pending;
    }
    let mut lines = Vec::new();
    let mut pending = false;
    for record in active {
        match output_environment_lines(record, pid, usec) {
            Some(pair) => lines.push(pair),
            None if record.gl_identity == Default::default() => pending = true,
            None => {
                return OutputEnvironmentBatch::Unavailable(record.kms.head);
            }
        }
    }
    if pending {
        return OutputEnvironmentBatch::Pending;
    }
    OutputEnvironmentBatch::Ready(lines)
}

/// Retry until all active heads have a usable GL identity. A context may first
/// be created after the startup KMS baseline. Publish the complete set once,
/// rather than duplicating the ready heads while another worker is pending.
fn trace_output_environment(
    native: &LiveProductionNativeScanout,
) -> Result<bool, Box<dyn std::error::Error>> {
    let records = native.output_environment_readback()?;
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let usec =
        u64::try_from(i128::from(clock.tv_sec) * 1_000_000 + i128::from(clock.tv_nsec) / 1000)?;
    let lines = match output_environment_batch(&records, std::process::id(), usec) {
        OutputEnvironmentBatch::Pending => return Ok(false),
        OutputEnvironmentBatch::Unavailable(head) => {
            // Do not read back every turn forever for an absent VRR property
            // or conflicting context. This marker cannot qualify a campaign.
            tracing::warn!(
                "sophia_shell_environment_unavailable schema=1 head={} monotonic_usec={usec}",
                head.raw(),
            );
            return Ok(true);
        }
        OutputEnvironmentBatch::Ready(lines) => lines,
    };
    if let Some(destination) = std::env::var_os("SOPHIA_OUTPUT_ENVIRONMENT_SOCKET") {
        send_output_environment(std::path::Path::new(&destination), &lines)?;
    }
    for pair in lines {
        for line in pair {
            tracing::info!("{line}");
        }
    }
    Ok(true)
}
