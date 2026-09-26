//! Tool-only capture framing. Public codecs remain the sole row/body validators.
use sophia_protocol::PolicyProfileCommand;
use sophia_protocol::wm_files::*;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Read;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Snapshot,
    Events,
}

pub struct Options {
    pub mode: Mode,
    pub path: PathBuf,
    pub epoch: u64,
    pub capabilities: u64,
}

impl Options {
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        let mode = match arguments.first().map(String::as_str) {
            Some("snapshot") => Mode::Snapshot,
            Some("events") => Mode::Events,
            _ => return Err("require snapshot or events mode; use --help".into()),
        };
        let mut values = BTreeMap::new();
        for argument in &arguments[1..] {
            let (key, value) = argument.split_once('=').ok_or("require --name=value")?;
            if !["--path", "--epoch", "--capabilities"].contains(&key)
                || value.is_empty()
                || values.insert(key, value).is_some()
            {
                return Err("unknown, empty or duplicate argument".into());
            }
        }
        let path = PathBuf::from(*values.get("--path").ok_or("missing --path")?);
        let epoch = values
            .get("--epoch")
            .ok_or("missing --epoch")?
            .parse::<u64>()
            .map_err(|_| "invalid epoch")?;
        if epoch == 0 {
            return Err("epoch must be nonzero".into());
        }
        let mask = values
            .get("--capabilities")
            .ok_or("missing --capabilities")?
            .strip_prefix("0x")
            .ok_or("capabilities require 0x hexadecimal mask")?;
        let capabilities = u64::from_str_radix(mask, 16).map_err(|_| "invalid capability mask")?;
        Ok(Self {
            mode,
            path,
            epoch,
            capabilities,
        })
    }
}

pub fn read_bounded(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(WM_FILE_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read capture: {e}"))?;
    if bytes.len() > WM_FILE_MAX_BYTES {
        return Err("capture exceeds 1 MiB window bound".into());
    }
    Ok(bytes)
}

#[derive(Debug, Eq, PartialEq)]
pub enum Payload {
    Snapshot(Box<WmFileSnapshot>),
    Negotiated(u64),
    Submitted(WmFileSubmitted),
    Profile(PolicyProfileCommand),
    ConfigurationOutcome(WmFileConfigurationOutcome),
    Cycle(WmFileCycle),
    ProjectionOutcome(WmFileProjectionOutcome),
    SessionOperationOutcome(WmFileSessionOperationOutcome),
    PresentationReceipt(WmFilePresentationReceipt),
}

#[derive(Debug, Eq, PartialEq)]
pub struct InspectedRecord {
    pub offset: usize,
    pub header: WmFileHeader,
    pub payload: Payload,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Capture {
    pub mode: Mode,
    pub epoch: u64,
    pub capabilities: u64,
    pub records: Vec<InspectedRecord>,
}

fn decoded<T, E: std::fmt::Debug>(result: Result<T, E>, offset: usize) -> Result<T, String> {
    result.map_err(|error| format!("record at byte {offset}: {error:?}"))
}

fn record(
    bytes: &[u8],
    mode: Mode,
    epoch: u64,
    caps: u64,
    offset: usize,
) -> Result<InspectedRecord, String> {
    let class = match mode {
        Mode::Snapshot => WmFileClass::Object,
        Mode::Events => WmFileClass::Event,
    };
    let header = decoded(decode_wm_file_record(bytes, class), offset)?.header;
    if header.connection_epoch != epoch {
        return Err(format!("record at byte {offset}: epoch mismatch"));
    }
    let payload = match header.kind {
        WmFileKind::Snapshot if mode == Mode::Snapshot => Payload::Snapshot(Box::new(decoded(
            decode_wm_file_snapshot(bytes, caps),
            offset,
        )?)),
        WmFileKind::Negotiated => {
            Payload::Negotiated(decoded(decode_wm_file_negotiated(bytes), offset)?)
        }
        WmFileKind::Submitted => {
            Payload::Submitted(decoded(decode_wm_file_submitted(bytes), offset)?)
        }
        WmFileKind::ProfilePrepare | WmFileKind::ProfileActivate | WmFileKind::ProfileRollback => {
            Payload::Profile(decoded(
                decode_wm_file_profile_command(bytes, header.kind, caps),
                offset,
            )?)
        }
        WmFileKind::ConfigurationOutcome => Payload::ConfigurationOutcome(decoded(
            decode_wm_file_configuration_outcome(bytes, caps),
            offset,
        )?),
        WmFileKind::Cycle => Payload::Cycle(decoded(decode_wm_file_cycle(bytes, caps), offset)?),
        WmFileKind::ProjectionOutcome => Payload::ProjectionOutcome(decoded(
            decode_wm_file_projection_outcome(bytes, caps),
            offset,
        )?),
        WmFileKind::SessionOperationOutcome => Payload::SessionOperationOutcome(decoded(
            decode_wm_file_session_operation_outcome(bytes, caps),
            offset,
        )?),
        WmFileKind::PresentationReceipt => Payload::PresentationReceipt(decoded(
            decode_wm_file_presentation_receipt(bytes, caps),
            offset,
        )?),
        _ => {
            return Err(format!(
                "record at byte {offset}: unsupported captured kind"
            ));
        }
    };
    Ok(InspectedRecord {
        offset,
        header,
        payload,
    })
}

pub fn inspect(bytes: &[u8], mode: Mode, epoch: u64, capabilities: u64) -> Result<Capture, String> {
    if epoch == 0 || bytes.len() > WM_FILE_MAX_BYTES {
        return Err("invalid context epoch or capture exceeds 1 MiB".into());
    }
    let mut capture = Capture {
        mode,
        epoch,
        capabilities,
        records: Vec::new(),
    };
    if mode == Mode::Snapshot {
        capture
            .records
            .push(record(bytes, mode, epoch, capabilities, 0)?);
        return Ok(capture);
    }
    let mut offset = 0usize;
    let mut previous: Option<u64> = None;
    while offset < bytes.len() {
        if capture.records.len() == usize::from(WM_FILE_MAX_JOURNAL_RECORDS) {
            return Err("capture exceeds 64 journal records".into());
        }
        let remaining = &bytes[offset..];
        let length_bytes: [u8; 4] = remaining
            .get(..4)
            .ok_or_else(|| format!("truncated length at byte {offset}"))?
            .try_into()
            .map_err(|_| "invalid length field")?;
        let length =
            usize::try_from(u32::from_le_bytes(length_bytes)).map_err(|_| "length overflow")?;
        if !(WM_FILE_HEADER_BYTES..=WM_FILE_MAX_BYTES).contains(&length) || length > remaining.len()
        {
            return Err(format!(
                "invalid or truncated record length at byte {offset}"
            ));
        }
        let value = record(&remaining[..length], mode, epoch, capabilities, offset)?;
        if previous.is_some_and(|last| last.checked_add(1) != Some(value.header.sequence)) {
            return Err(format!("noncontiguous captured sequence at byte {offset}"));
        }
        previous = Some(value.header.sequence);
        capture.records.push(value);
        offset = offset.checked_add(length).ok_or("offset overflow")?;
    }
    Ok(capture)
}

impl Capture {
    pub fn text(&self) -> String {
        let mut text = format!(
            "captured_window=true authenticated=false mode={:?} expected_epoch={} supplied_capabilities=0x{:016x} records={} completeness=not_inferred phase=not_inferred\n",
            self.mode,
            self.epoch,
            self.capabilities,
            self.records.len()
        );
        for (index, record) in self.records.iter().enumerate() {
            let h = record.header;
            writeln!(
                text,
                "record={index} byte_offset={} class={:?} kind={:?} epoch={} sequence={}",
                record.offset,
                wm_file_class(h.kind),
                h.kind,
                h.connection_epoch,
                h.sequence
            )
            .unwrap();
            // Debug escapes all strings in typed records, including control
            // characters. Never print unvalidated body bytes as terminal text.
            match &record.payload {
                Payload::Snapshot(v) => writeln!(text, "transaction={} snapshot={:?}", v.transaction.raw(), v.snapshot),
                Payload::Negotiated(caps) => writeln!(text, "reported_selected_capabilities=0x{caps:016x} admission=not_authenticated"),
                Payload::Submitted(v) => writeln!(text, "accepted_submission_id={} candidate_kind={:?} custody=accepted semantic_outcome=not_implied", v.submission_id, v.candidate_kind),
                Payload::Profile(v) => writeln!(text, "transaction={} profile_command={v:?}", v.transaction.raw()),
                Payload::ConfigurationOutcome(v) => writeln!(text, "transaction={} reported_semantic_outcome={v:?}", v.transaction.raw()),
                Payload::Cycle(v) => writeln!(text, "snapshot_transaction={} request_transaction={} request={:?}", v.snapshot_transaction.raw(), v.request_transaction.raw(), v.request),
                Payload::ProjectionOutcome(v) => writeln!(text, "transaction={} reported_semantic_outcome={v:?}", v.transaction.raw()),
                Payload::SessionOperationOutcome(v) => writeln!(text, "transaction={} reported_semantic_outcome={v:?}", v.transaction.raw()),
                Payload::PresentationReceipt(v) => writeln!(text, "transaction={} reported_receipt={:?} physical_completion_observed_by_tool=false", v.transaction.raw(), v.receipt),
            }.unwrap();
        }
        text
    }
}
