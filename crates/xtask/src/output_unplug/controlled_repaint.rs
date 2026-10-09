//! The controlled-repaint verdict (t307 part 2): whether a static DMA-BUF
//! client's retained image survives repeated recomposition on both heads of
//! one mirrored output, each head composed by its own renderer.
//!
//! WHAT IS RUN. Both virtio heads mirror one logical output. The DRI3 probe
//! presents one frame and holds; after its barrier the host presses F9 once a
//! second twenty times, and the profile binds F9 to the generic WM's
//! hold-shift action, which moves the window 8 pixels further right each
//! time, so each committed shift has a crop of its own: the nth at 8n.
//! QMP carries no sequence the guest sees, so the host's cadence records are
//! cadence evidence only. The authoritative chain is the guest's: Session's
//! action record (activation serial, request, transaction, Committed), the
//! WM's proposal for that transaction with its offset, Session's settlement
//! of the same transaction, then on each head the presented frames, in native
//! frame order, whose window region sits at that offset from the window's
//! first presented position. Equal crops of different shifts would let a
//! stale frame pass for a later shift; distinct ones name the shift.
//!
//! THE VERDICT, declared before any run (plan u9rtb0ml, part 2), one line:
//! - RETAINED, the only pass: every presented region of the window on both
//!   heads is the client's frame, the crop runs of each head follow the
//!   committed shifts, two distinct renderers each imported the image, one
//!   native owner served the run, and coverage met its bounds.
//! - LOST: a presented region of the window, on either head and at any time
//!   after that head's first, differs from the client's frame.
//! - UNREADY: the session never became ready. It reproduces the failure only
//!   in the 153 shape: a first presented region equal to the frame and a
//!   later one that is not (`reproduced=yes`).
//! - INSUFFICIENT: no contradiction, but a head missed committed shifts or
//!   the samples do not cover the window: at least 15 per head, the first by
//!   1.5 s after the Present's retirement, the last from 18.5 s after it, no
//!   gap over 2.5 s (guest clock).
//! - INVALID: the run is not this fixture, its chain is broken, or it ended
//!   badly without a contradiction.
//!
//! Renderer recoveries and fallbacks are counted as context and never turn a
//! contradiction into RETAINED. Region readback proves rendered content and
//! its presentation, not physical scanout.

use super::{
    Record, clean_exit, number, presented_frame, probe_frame_checksum, region_size, strip_ansi,
};

const VERDICT: &str = "sophia_qemu_controlled_repaint_verdict schema=1";
/// The generic WM's one registered action under `--hold-shift`.
const HOLD_SHIFT_ACTION: &str = "1";
const SHIFT_PIXELS: u64 = 8;
const PRESSES: usize = 20;
const WINDOW_US: i64 = 20_000_000;
const FIRST_BY_US: i64 = 1_500_000;
const LAST_FROM_US: i64 = WINDOW_US - 1_500_000;
const MAX_GAP_US: i64 = 2_500_000;
const MIN_SAMPLES: usize = 15;

/// One presented region of the window on a head.
struct Sample {
    at: usize,
    frame: u64,
    x: String,
    y: String,
    checksum: String,
}

pub(super) fn verify(records: &[Record], log: &str) -> Result<Vec<String>, String> {
    let lines = log.lines().map(strip_ansi).collect::<Vec<_>>();
    classify(records, &lines).map(|line| vec![line])
}

fn classify(records: &[Record], lines: &[String]) -> Result<String, String> {
    let invalid = |reason: String| format!("{VERDICT} status=INVALID reason: {reason}");
    fixture(records).map_err(invalid)?;

    // The client's one Present names the frame every region must show.
    let presents = indices(records, |r| {
        r.name == "sophia_live_session_present" && r.get("status") == Some("retired")
    });
    let ready = records
        .iter()
        .any(|r| r.is("sophia_live_session_startup", "ready") && r.get("schema") == Some("2"));
    let present = presents.first().map(|&at| &records[at]);
    let size = present.and_then(|p| p.get("source")).unwrap_or("");
    let expected = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .filter(|&(w, h)| (1..=4096).contains(&w) && (1..=4096).contains(&h));
    let expected_checksum = expected.map(|(w, h)| probe_frame_checksum(w, h).to_string());
    let heads = samples(records, size).map_err(invalid)?;
    let first_contradiction = || {
        heads.iter().find_map(|(head, samples)| {
            let reference = &samples.first()?.checksum;
            samples[1..]
                .iter()
                .find(|s| &s.checksum != reference)
                .map(|s| (head, reference, s))
        })
    };

    if !ready {
        let baseline = heads.iter().all(|(_, samples)| {
            samples.first().map(|s| Some(&s.checksum)) == Some(expected_checksum.as_ref())
        });
        let reproduced = !heads.is_empty() && baseline && first_contradiction().is_some();
        return Err(format!(
            "{VERDICT} status=UNREADY reproduced={} presented_heads={}",
            if reproduced { "yes" } else { "no" },
            heads.len()
        ));
    }
    let ([present_at], Some(present), Some((width, height)), Some(expected_checksum)) =
        (presents.as_slice(), present, expected, expected_checksum)
    else {
        return Err(invalid(format!(
            "expected exactly one retired client Present naming its source size, found {}",
            presents.len()
        )));
    };
    if present.get("schema") != Some("2") {
        return Err(invalid(
            "the client's Present did not retire on the mixed (DMA-BUF) path".to_owned(),
        ));
    }
    let present_at = *present_at;
    if heads.len() != 2 {
        return Err(invalid(format!(
            "the window was presented on {} heads, not the two of one mirrored output",
            heads.len()
        )));
    }
    let pixels = (u64::from(width) * u64::from(height)).to_string();
    for (head, samples) in &heads {
        let reference = &records[samples[0].at];
        if samples[0].checksum != expected_checksum
            || reference.get("region_pixels") != Some(pixels.as_str())
        {
            return Err(invalid(format!(
                "head {head}: its first presented region is not the client's frame: checksum={} region_pixels={}, expected checksum={expected_checksum} region_pixels={pixels}",
                samples[0].checksum,
                reference.get("region_pixels").unwrap_or("?"),
            )));
        }
    }
    let recoveries = recoveries(records);
    if let Some((head, reference, sample)) = first_contradiction() {
        let region = &records[sample.at];
        return Err(format!(
            "{VERDICT} status=LOST head={head} frame={} offset_x={} checksum={} nonzero_rgb_pixels={} expected={reference} recoveries={recoveries}",
            sample.frame,
            sample.x,
            sample.checksum,
            region.get("nonzero_rgb_pixels").unwrap_or("?"),
        ));
    }

    if let Some(fatal) = records
        .iter()
        .position(|r| r.name == "sophia_live_session_runtime_fatal")
    {
        return Err(invalid(format!(
            "ended in a runtime fatal: {}",
            lines[fatal]
        )));
    }
    clean_exit(records).map_err(|error| invalid(format!("guest end {error}")))?;
    let offsets = committed_shifts(records).map_err(invalid)?;
    let renderers = importers(records, &heads).map_err(invalid)?;
    let owners = indices(records, |r| r.name == "sophia_live_native_owner");
    if owners.len() != 1 || !records[owners[0]].is("sophia_live_native_owner", "opened") {
        return Err(invalid(format!(
            "{} native owner records; the run must keep its one startup owner",
            owners.len()
        )));
    }

    // Each head's crop runs, in its own frame order, must be its first
    // position and then each committed offset from it, in commit order. A
    // head that skipped shifts is short of coverage; a crop out of order or
    // outside the committed set contradicts the chain.
    let base = heads
        .values()
        .map(|samples| samples[0].x.parse::<u64>().ok())
        .collect::<Option<std::collections::BTreeSet<_>>>()
        .filter(|bases| bases.len() == 1)
        .and_then(|bases| bases.first().copied())
        .ok_or_else(|| {
            invalid("the heads first show the window at different positions".to_owned())
        })?;
    let mut expected_runs = vec![base];
    for offset in &offsets {
        expected_runs.push(
            base + offset
                .parse::<u64>()
                .map_err(|_| invalid("a non-numeric offset".to_owned()))?,
        );
    }
    for (head, samples) in &heads {
        if samples.iter().any(|s| s.y != samples[0].y) {
            return Err(invalid(format!("head {head}: the window moved vertically")));
        }
        let mut runs: Vec<u64> = Vec::new();
        for sample in samples {
            let x = sample.x.parse::<u64>().map_err(|_| {
                invalid(format!(
                    "head {head}: a window region has no numeric origin"
                ))
            })?;
            if runs.last() != Some(&x) {
                runs.push(x);
            }
        }
        // Each run's place among the committed crops, which must rise.
        let places = runs
            .iter()
            .map(|x| expected_runs.iter().position(|e| e == x))
            .collect::<Option<Vec<_>>>();
        let ordered = places
            .as_ref()
            .is_some_and(|places| places.windows(2).all(|pair| pair[0] < pair[1]));
        if !ordered {
            let show = |runs: &[u64]| {
                runs.iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            };
            return Err(invalid(format!(
                "head {head}: crop runs {} do not follow the committed crops {}",
                show(&runs),
                show(&expected_runs)
            )));
        }
        if runs.len() != expected_runs.len() {
            let missed = (1..expected_runs.len())
                .filter(|place| !runs.contains(&expected_runs[*place]))
                .map(|place| place.to_string())
                .collect::<Vec<_>>();
            return Err(format!(
                "{VERDICT} status=INSUFFICIENT reason: head {head} presented {} of {} committed shifts, missing {}",
                runs.len() - 1,
                offsets.len(),
                missed.join(",")
            ));
        }
    }

    // Coverage on the guest clock, from the Present's retirement.
    let start = timestamp_us(&lines[present_at])
        .ok_or_else(|| invalid("the Present retirement has no guest timestamp".to_owned()))?;
    let mut counts = Vec::new();
    for (head, samples) in &heads {
        let mut times = Vec::new();
        for sample in samples {
            let at = timestamp_us(&lines[sample.at])
                .ok_or_else(|| invalid(format!("head {head}: a region has no guest timestamp")))?;
            if at - start <= WINDOW_US {
                times.push(at - start);
            }
        }
        let gap = times
            .windows(2)
            .map(|w| w[1] - w[0])
            .max()
            .unwrap_or(i64::MAX);
        let (first, last) = (times.first().copied(), times.last().copied());
        if times.len() < MIN_SAMPLES
            || first.is_none_or(|first| first > FIRST_BY_US)
            || last.is_none_or(|last| last < LAST_FROM_US)
            || gap > MAX_GAP_US
        {
            return Err(format!(
                "{VERDICT} status=INSUFFICIENT reason: head {head} samples={} first_us={} last_us={} max_gap_us={gap}",
                times.len(),
                first.map_or("none".to_owned(), |v| v.to_string()),
                last.map_or("none".to_owned(), |v| v.to_string()),
            ));
        }
        counts.push(format!("{head}:{}", times.len()));
    }
    Ok(format!(
        "{VERDICT} status=RETAINED size={size} checksum={expected_checksum} shifts={} samples={} renderers={} recoveries={recoveries}",
        offsets.len(),
        counts.join(","),
        renderers.join(","),
    ))
}

fn indices(records: &[Record], keep: impl Fn(&Record) -> bool) -> Vec<usize> {
    (0..records.len())
        .filter(|&at| keep(&records[at]))
        .collect()
}

/// The fixture is this mode's, it reached the session unchanged, and the
/// host's presses all went out after the barrier.
fn fixture(records: &[Record]) -> Result<(), String> {
    let one = |name: &str, status: &str| -> Result<usize, String> {
        match indices(records, |r| r.is(name, status)).as_slice() {
            [at] => Ok(*at),
            found => Err(format!(
                "expected one {name} status={status}, found {}",
                found.len()
            )),
        }
    };
    let running = &records[one("sophia_qemu_unplug", "running")?];
    if running.get("mode") != Some("controlled-repaint")
        || running.get("wm") != Some("true")
        || running.get("client") != Some("dri3")
    {
        return Err("was not run as controlled-repaint with the WM and the DRI3 client".to_owned());
    }
    if number(
        &records[one("sophia_qemu_topology", "observed")?],
        "connected",
    )? != 2
    {
        return Err("did not start with two connected heads".to_owned());
    }
    if let Some(failed) = records
        .iter()
        .find(|r| r.name.starts_with("sophia_qemu_") && r.get("status") == Some("failed"))
    {
        return Err(format!(
            "contains a failure marker: {} reason={}",
            failed.name,
            failed.get("reason").unwrap_or("?")
        ));
    }
    let ready = records
        .iter()
        .any(|r| r.is("sophia_live_session_startup", "ready") && r.get("schema") == Some("2"));
    if !ready {
        // The host's actions follow readiness; UNREADY is judged by the caller.
        return Ok(());
    }
    let uevents = &records[one("sophia_qemu_unplug", "uevents")?];
    for key in ["drm_hotplug", "input_remove", "input_add"] {
        if number(uevents, key)? != 0 {
            return Err(format!(
                "the fixture changed a device: {key}={}",
                number(uevents, key)?
            ));
        }
    }
    let barrier = one("sophia_qemu_unplug", "static_barrier")?;
    let sending = one("sophia_qemu_unplug", "cadence_sending")?;
    let sent = one("sophia_qemu_unplug", "cadence_sent")?;
    let presses = indices(records, |r| r.is("sophia_qemu_unplug", "key_cadence"));
    let declared = &records[sending];
    if declared.get("key") != Some("f9")
        || declared.get("count") != Some("20")
        || declared.get("period_ms") != Some("1000")
        || records[sent].get("result") != Some("completed")
    {
        return Err(
            "the host cadence is not the declared twenty F9 presses a second apart".to_owned(),
        );
    }
    let in_order = presses.len() == PRESSES
        && presses.iter().enumerate().all(|(index, &at)| {
            records[at].get("index") == Some(index.to_string().as_str())
                && records[at].get("key") == Some("f9")
                && sending < at
                && at < sent
        });
    if !(barrier < sending && in_order) {
        return Err(format!(
            "the host's {} cadence records do not follow the barrier in order",
            presses.len()
        ));
    }
    Ok(())
}

/// Every presented region of the window, per head in native frame order.
fn samples<'a>(
    records: &'a [Record],
    size: &str,
) -> Result<std::collections::BTreeMap<&'a str, Vec<Sample>>, String> {
    let mut heads = std::collections::BTreeMap::<&str, Vec<Sample>>::new();
    let mut output = None;
    for at in 0..records.len() {
        let record = &records[at];
        if !(record.is("sophia_native_composition_region_frame", "read")
            && record.get("source_stage") == Some("renderer_image")
            && !size.is_empty()
            && region_size(record) == Some(size))
        {
            continue;
        }
        let Some(frame) = presented_frame(records, at) else {
            continue;
        };
        let frame = frame
            .parse::<u64>()
            .map_err(|_| "a presented frame has no numeric identity".to_owned())?;
        let (Some(head), Some(target), Some(checksum)) = (
            record.get("head"),
            record.get("target"),
            record.get("checksum"),
        ) else {
            return Err("a window region lacks head, target or checksum".to_owned());
        };
        if *output.get_or_insert(record.get("output")) != record.get("output") {
            return Err("window regions name more than one output".to_owned());
        }
        let mut parts = target.split('_').skip(1);
        let (Some(x), Some(y)) = (parts.next(), parts.next()) else {
            return Err(format!("window region target {target} has no origin"));
        };
        heads.entry(head).or_default().push(Sample {
            at,
            frame,
            x: x.to_owned(),
            y: y.to_owned(),
            checksum: checksum.to_owned(),
        });
    }
    for (head, samples) in &mut heads {
        samples.sort_by_key(|s| s.frame);
        for pair in samples.windows(2) {
            if pair[0].frame == pair[1].frame
                && (pair[0].x != pair[1].x || pair[0].checksum != pair[1].checksum)
            {
                return Err(format!(
                    "head {head}: frame {} has two different regions",
                    pair[0].frame
                ));
            }
        }
        samples.dedup_by_key(|s| s.frame);
    }
    Ok(heads)
}

/// The offsets of the committed hold-shift transactions, in commit order,
/// each bound by its activation serial, request and transaction from
/// Session's action record to the WM's proposal and Session's settlement.
fn committed_shifts(records: &[Record]) -> Result<Vec<String>, String> {
    let mut offsets = Vec::new();
    let mut serial = 0u64;
    for action in records.iter().filter(|r| {
        r.name == "sophia_shell_action_policy" && r.get("action") == Some(HOLD_SHIFT_ACTION)
    }) {
        let (Some(activation), Some(transaction), Some(request)) = (
            action.get("activation_serial"),
            action.get("transaction"),
            action.get("request_id"),
        ) else {
            return Err("a hold-shift action record lacks its identities".to_owned());
        };
        if action.get("outcome") != Some("Committed") {
            return Err(format!(
                "hold-shift transaction {transaction} was not committed"
            ));
        }
        let next = activation
            .parse::<u64>()
            .ok()
            .filter(|&next| next > serial)
            .ok_or_else(|| format!("activation serial {activation} does not increase"))?;
        serial = next;
        let same = |r: &Record| {
            r.get("transaction") == Some(transaction) && r.get("request_id") == Some(request)
        };
        let proposals = indices(records, |r| {
            r.is("sophia_qemu_wm_hold", "proposed") && same(r)
        });
        let [proposal] = proposals.as_slice() else {
            return Err(format!(
                "transaction {transaction}: {} WM proposals, expected one",
                proposals.len()
            ));
        };
        let proposal = &records[*proposal];
        let offset = (SHIFT_PIXELS * (offsets.len() as u64 + 1)).to_string();
        let offset = offset.as_str();
        if proposal.get("cause") != Some("1")
            || proposal.get("activation_serial") != Some(activation)
            || proposal.get("shift") != Some("1")
            || proposal.get("offset_x") != Some(offset)
        {
            return Err(format!(
                "transaction {transaction}: the WM proposal is not shift {} to offset {offset}",
                offsets.len() + 1
            ));
        }
        let settled = indices(records, |r| {
            r.is("sophia_live_wm_chrome", "settled")
                && same(r)
                && r.get("outcome") == Some("Committed")
        });
        if settled.len() != 1 {
            return Err(format!(
                "transaction {transaction}: no single committed settlement"
            ));
        }
        offsets.push(offset.to_owned());
    }
    let proposed = records
        .iter()
        .filter(|r| r.is("sophia_qemu_wm_hold", "proposed") && r.get("shift") == Some("1"))
        .count();
    if proposed != offsets.len() || offsets.len() > PRESSES {
        return Err(format!(
            "{proposed} WM shifts against {} committed hold-shift actions",
            offsets.len()
        ));
    }
    Ok(offsets)
}

/// Each head's renderer: one identity per head, distinct between the heads,
/// one native owner, the window's output, and at least one import each.
fn importers(
    records: &[Record],
    heads: &std::collections::BTreeMap<&str, Vec<Sample>>,
) -> Result<Vec<String>, String> {
    let observed = records
        .iter()
        .filter(|r| r.is("sophia_live_head_renderer_imports", "observed"))
        .collect::<Vec<_>>();
    let output = heads
        .values()
        .next()
        .and_then(|samples| records[samples[0].at].get("output"));
    let owner = observed.first().and_then(|r| r.get("owner"));
    let mut renderers = Vec::new();
    for head in heads.keys() {
        let mine = observed
            .iter()
            .filter(|r| r.get("head") == Some(head))
            .collect::<Vec<_>>();
        let Some(first) = mine.first() else {
            return Err(format!("head {head}: no per-head import record"));
        };
        let renderer = first.get("renderer");
        if mine.iter().any(|r| {
            r.get("renderer") != renderer || r.get("owner") != owner || r.get("output") != output
        }) || owner.is_none_or(|owner| owner == "none")
        {
            return Err(format!(
                "head {head}: import records name another renderer, owner or output"
            ));
        }
        let imports = mine
            .iter()
            .filter_map(|r| r.get("imports")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        if imports == 0 {
            return Err(format!(
                "head {head}: its renderer never imported the image"
            ));
        }
        renderers.push(format!("{head}:{}", renderer.unwrap_or("?")));
    }
    let distinct = observed
        .iter()
        .filter_map(|r| r.get("renderer"))
        .collect::<std::collections::BTreeSet<_>>();
    if distinct.len() != heads.len() {
        return Err(format!(
            "{} renderer identities for {} heads: not one importer per head",
            distinct.len(),
            heads.len()
        ));
    }
    Ok(renderers)
}

/// Renderer recoveries and output fallbacks, context only.
fn recoveries(records: &[Record]) -> usize {
    records
        .iter()
        .filter(|r| {
            (r.name == "sophia_renderer_worker"
                && matches!(r.get("status"), Some("hard_stall" | "stall_recovered")))
                || r.name.contains("fallback")
        })
        .count()
}

/// A tracing line's leading RFC 3339 UTC timestamp, in microseconds.
fn timestamp_us(line: &str) -> Option<i64> {
    let stamp = line.split_whitespace().next()?.strip_suffix('Z')?;
    let (date, time) = stamp.split_once('T')?;
    let mut date = date.splitn(3, '-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);
    let (clock, fraction) = time.split_once('.').unwrap_or((time, "0"));
    let mut clock = clock.splitn(3, ':').map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (clock.next()??, clock.next()??, clock.next()??);
    if fraction.is_empty() || fraction.len() > 9 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let micros = format!("{fraction:0<6}")[..6].parse::<i64>().ok()?;
    // Days from the civil date (proleptic Gregorian), so a run may cross midnight.
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((((days * 24 + hour) * 60 + minute) * 60 + second) * 1_000_000 + micros)
}
