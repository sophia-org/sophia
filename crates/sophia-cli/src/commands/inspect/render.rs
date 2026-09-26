use sophia_9p::{QidKind, client::Listed, records::Attr};
use sophia_protocol::inspection::*;
use std::io::{self, Write};

pub(super) fn listing(out: &mut impl Write, entries: &[Listed], json: bool) -> io::Result<()> {
    if json {
        write!(out, "{{\"schema\":1,\"entries\":[")?;
    }
    for (index, entry) in entries.iter().enumerate() {
        // The caller validates the complete fixed vocabulary before rendering.
        let name = std::str::from_utf8(&entry.name).map_err(io::Error::other)?;
        if json {
            if index != 0 {
                write!(out, ",")?;
            }
            write!(
                out,
                "{{\"name\":\"{name}\",\"qid\":\"{}\",\"version\":{}}}",
                entry.qid.path, entry.qid.version
            )?;
        } else {
            writeln!(out, "{name}\tqid={}:{}", entry.qid.path, entry.qid.version)?;
        }
    }
    if json {
        writeln!(out, "]}}")?;
    }
    Ok(())
}

pub(super) fn stat(out: &mut impl Write, name: &str, attr: &Attr, json: bool) -> io::Result<()> {
    let kind = if attr.qid.kind == QidKind::Directory {
        "directory"
    } else {
        "file"
    };
    if json {
        writeln!(
            out,
            "{{\"schema\":1,\"path\":\"{name}\",\"type\":\"{kind}\",\"qid\":\"{}\",\"version\":{},\"mode\":{},\"size\":\"{}\"}}",
            attr.qid.path, attr.qid.version, attr.mode, attr.size
        )
    } else {
        writeln!(
            out,
            "{name} {kind} mode={:o} size={} qid={}:{}",
            attr.mode, attr.size, attr.qid.path, attr.qid.version
        )
    }
}

pub(super) fn status(out: &mut impl Write, value: &InspectionStatus, json: bool) -> io::Result<()> {
    if json {
        out.write_all(
            format_inspection_status(value)
                .map_err(io::Error::other)?
                .as_bytes(),
        )
    } else {
        writeln!(
            out,
            "WM epoch={} observer={} sequence={} snapshot={} events={}..{} losses={} state={} wire={} capabilities={:#x}",
            value.wm_epoch,
            value.generation,
            value.sequence,
            value.snapshot_available,
            value.event_floor,
            value.event_tail,
            value.loss_generation,
            state_name(value.state),
            value.wire.map_or("unavailable", wire_name),
            value.selected_capabilities,
        )
    }
}

pub(super) fn snapshot(
    out: &mut impl Write,
    record: &InspectionSnapshotRecord,
    json: bool,
) -> io::Result<()> {
    if json {
        return out.write_all(
            format_inspection_snapshot(record)
                .map_err(io::Error::other)?
                .as_bytes(),
        );
    }
    let value = &record.snapshot;
    let wire = wire_name(value.wire);
    let state = state_name(value.state);
    writeln!(
        out,
        "Session={} WM epoch={} wire={wire} state={state} scene={} capabilities={:#x}",
        value.session_generation,
        value.wm_epoch,
        value.scene_generation,
        value.selected_capabilities
    )?;
    writeln!(
        out,
        "Observer={} sequence={} cursor={} losses={} (Session view, not physical presentation)",
        record.generation, record.sequence, record.event_offset, record.loss_generation
    )?;
    for output in &value.outputs {
        write!(
            out,
            "output={} generation={} geometry=",
            output.id, output.generation
        )?;
        rect(out, output.geometry)?;
        write!(out, " work-area=")?;
        rect(out, output.work_area)?;
        match output.focus {
            Some(id) => writeln!(out, " focus={}/{}", id.index, id.generation)?,
            None => writeln!(out, " focus=none")?,
        }
    }
    for surface in &value.surfaces {
        write!(
            out,
            "surface={}/{} state-generation={} output=",
            surface.id.index, surface.id.generation, surface.state_generation
        )?;
        match surface.output {
            Some(id) => write!(out, "{id}")?,
            None => write!(out, "none")?,
        }
        write!(out, " geometry=")?;
        rect(out, surface.geometry)?;
        writeln!(out)?;
    }
    Ok(())
}

fn rect(out: &mut impl Write, value: InspectionRect) -> io::Result<()> {
    write!(
        out,
        "{},{} {}x{}",
        value.x, value.y, value.width, value.height
    )
}

fn wire_name(wire: InspectionWire) -> &'static str {
    match wire {
        InspectionWire::CurrentIpc => "current-ipc",
        InspectionWire::Files => "9p2000.L",
    }
}

fn state_name(state: InspectionState) -> &'static str {
    match state {
        InspectionState::Starting => "starting",
        InspectionState::Ready => "ready",
        InspectionState::Unavailable => "unavailable",
        InspectionState::Stopped => "stopped",
    }
}

pub(super) fn event(
    out: &mut impl Write,
    value: &InspectionEventRecord,
    json: bool,
) -> io::Result<()> {
    if json {
        return out.write_all(
            format_inspection_event(value)
                .map_err(io::Error::other)?
                .as_bytes(),
        );
    }
    let name = match value.event {
        InspectionEvent::SnapshotChanged => "snapshot_changed",
        InspectionEvent::ConnectionChanged => "connection_changed",
        InspectionEvent::ConfigurationChanged => "configuration_changed",
        InspectionEvent::ConfigurationRejected => "configuration_rejected",
        InspectionEvent::ProjectionCommitted => "projection_committed",
        InspectionEvent::ProjectionRejected => "projection_rejected",
        InspectionEvent::ProjectionTimedOut => "projection_timed_out",
        InspectionEvent::PresentationChanged => "presentation_changed",
        InspectionEvent::SessionOperationAccepted => "session_operation_accepted",
        InspectionEvent::SessionOperationRejected => "session_operation_rejected",
    };
    writeln!(
        out,
        "sequence={} observer={} event={name} (coalesced owner notification)",
        value.sequence, value.generation
    )
}
