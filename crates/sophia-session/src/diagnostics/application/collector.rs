//! Fair nonblocking pipe drain. No filesystem writes or child waits occur here.
use super::{LAUNCH_BYTES, Launch, PACKET_BYTES, Shared, records::Packet};
use std::io::{self, Read};
use std::sync::{Arc, atomic::Ordering, mpsc::SyncSender};
use std::time::Duration;

pub(super) fn drain(shared: Arc<Shared>, sender: SyncSender<Packet>) {
    loop {
        let stopping = shared.stop.load(Ordering::Acquire);
        let Ok(mut launches) = shared.launches.lock() else {
            break;
        };
        // One bounded read per live stream per visit: floods cannot starve a peer.
        for launch in launches.iter_mut() {
            let mut buffer = [0; PACKET_BYTES - Packet::HEADER];
            if let Some(reader) = launch.reader.as_mut() {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        launch.eof = true;
                        launch.reader = None;
                        launch.dirty = true;
                    }
                    Ok(length) => {
                        let offset = launch.read;
                        launch.read = launch.read.saturating_add(length as u64);
                        let keep = if launch.retain {
                            length.min(LAUNCH_BYTES.saturating_sub(offset) as usize)
                        } else {
                            0
                        };
                        if keep > 0
                            && sender
                                .try_send(Packet::new(1, launch.id, offset, &buffer[..keep]))
                                .is_ok()
                        {
                            launch.queued += keep as u64;
                            launch.dropped += (length - keep) as u64;
                        } else {
                            launch.dropped += length as u64;
                        }
                        launch.dirty = true;
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => {
                        launch.incomplete = true;
                        launch.reader = None;
                        launch.dirty = true;
                    }
                }
            }
            if stopping {
                launch.incomplete |=
                    !launch.eof || (launch.spawn == "spawned" && launch.exit.is_none());
                launch.reader = None;
                launch.dirty = true;
            }
            if launch.dirty && sender.try_send(metadata(launch)).is_ok() {
                launch.dirty = false;
            }
        }
        launches.retain(|launch| {
            launch.dirty
                || launch.reader.is_some()
                || launch.spawn == "pending"
                || (launch.spawn == "spawned" && launch.exit.is_none())
        });
        drop(launches);
        if stopping {
            if let Ok(launches) = shared.launches.lock() {
                shared.metadata_lost.fetch_add(
                    launches.iter().filter(|launch| launch.dirty).count() as u64,
                    Ordering::Relaxed,
                );
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn metadata(launch: &Launch) -> Packet {
    let (code, signal) = launch.exit.unwrap_or((None, None));
    let value = format!(
        "launch={} source={} transaction={} requested_utc_msec={} requested_boot_msec={} spawn={} pid={} exit_observed={} exit_code={} exit_signal={} eof={} incomplete={} retaining={} read_bytes={} queued_bytes={} dropped_bytes={} spawned_boot_msec={} exited_boot_msec={} spawn_reason={} executable={}\n",
        launch.id,
        launch.context.source.name(),
        launch
            .context
            .transaction
            .map_or("none".into(), |n| n.to_string()),
        launch.requested.utc_msec,
        launch.requested.boot_msec,
        launch.spawn,
        launch.pid.map_or("none".into(), |n| n.to_string()),
        launch.exit.is_some(),
        code.map_or("none".into(), |n| n.to_string()),
        signal.map_or("none".into(), |n| n.to_string()),
        launch.eof,
        launch.incomplete,
        launch.retain,
        launch.read,
        launch.queued,
        launch.dropped,
        launch.spawned.map_or(0, |stamp| stamp.boot_msec),
        launch.exited.map_or(0, |stamp| stamp.boot_msec),
        launch.spawn_reason,
        super::records::escape_bytes(&launch.executable),
    );
    Packet::new(2, launch.id, 0, value.as_bytes())
}
