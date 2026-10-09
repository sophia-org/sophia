//! `cargo xtask conformance verify output-unplug MODE LOG`: the verdict on
//! the QEMU output-unplug scenario (t306).
//!
//! WHAT IT PROVES. While the session runs on scanned-out heads, the guest
//! takes away one connector, every connector, or the keyboard, and in the
//! return modes gives it back. The session must live through it: no runtime
//! fatal, a published topology that reflects the loss, after a return one
//! that has every head again with input enabled, then a bounded completion and
//! a clean guest exit.
//!
//! WHAT IT DOES NOT. virtio-gpu is not the operator's card, and the guest
//! forces connector state through sysfs instead of a link going down. This
//! proves the owner transition on a connector loss and return; the KVM switch
//! itself stays an attended test.
//!
//! A run in which the guest saw no DRM hotplug uevent (or, in the input mode,
//! no input removal) never asked Sophia anything. It is reported as an
//! unreached fixture, never as a pass and never as a Sophia failure.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// One connector goes and stays gone.
    One,
    /// One connector goes and comes back.
    OneReturn,
    /// Every connector goes and comes back: a single-monitor KVM switch.
    AllReturn,
    /// The keyboard goes and comes back; the outputs stay.
    InputReturn,
}

impl Mode {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "one" => Ok(Self::One),
            "one-return" => Ok(Self::OneReturn),
            "all-return" => Ok(Self::AllReturn),
            "input-return" => Ok(Self::InputReturn),
            other => Err(format!(
                "unknown output-unplug mode {other:?}: one, one-return, all-return or input-return"
            )),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::One => "one",
            Self::OneReturn => "one-return",
            Self::AllReturn => "all-return",
            Self::InputReturn => "input-return",
        }
    }

    fn returns(self) -> bool {
        self != Self::One
    }

    fn display(self) -> bool {
        self != Self::InputReturn
    }
}

const TOPOLOGY: &str = "sophia_live_output_topology";

/// One record: its name and its `key=value` fields in order. Session records
/// written through tracing carry a timestamp, level and target before the
/// name, and possibly colour; the name is the first `sophia_` word that is not
/// a target.
struct Record {
    name: String,
    fields: Vec<(String, String)>,
}

impl Record {
    fn parse(line: &str) -> Self {
        let plain = strip_ansi(line);
        let mut words = plain.split_whitespace().skip_while(|word| {
            !word.starts_with("sophia_") || word.contains("::") || word.ends_with(':')
        });
        let name = words.next().unwrap_or("").to_owned();
        let fields = words
            .filter_map(|word| word.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect();
        Self { name, fields }
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn is(&self, name: &str, status: &str) -> bool {
        self.name == name && self.get("status") == Some(status)
    }
}

/// The line without its terminal escape sequences (ESC, then up to the
/// first letter).
fn strip_ansi(line: &str) -> String {
    let mut plain = String::with_capacity(line.len());
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            for character in characters.by_ref() {
                if character.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            plain.push(character);
        }
    }
    plain
}

fn number(record: &Record, key: &str) -> Result<u32, String> {
    record
        .get(key)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("{} has no numeric {key}", record.name))
}

/// Verifies one run's serial log and returns the summary lines on a pass.
pub fn verify(log: &str, mode: Mode) -> Result<Vec<String>, String> {
    let records = log.lines().map(Record::parse).collect::<Vec<_>>();
    let only = |name: &str, status: &str| -> Result<usize, String> {
        let mut found = records
            .iter()
            .enumerate()
            .filter(|(_, record)| record.is(name, status));
        match (found.next(), found.next()) {
            (Some((index, _)), None) => Ok(index),
            (None, _) => Err(format!("is missing {name} status={status}")),
            (Some(_), Some(_)) => Err(format!("has more than one {name} status={status}")),
        }
    };

    let running = only("sophia_qemu_unplug", "running")?;
    if records[running].get("mode") != Some(mode.name()) {
        return Err(format!("was run in another mode than {}", mode.name()));
    }
    let heads = number(
        &records[only("sophia_qemu_topology", "observed")?],
        "connected",
    )?;
    let required_heads = if mode == Mode::One || mode == Mode::OneReturn {
        2
    } else {
        1
    };
    if heads < required_heads {
        return Err(format!(
            "started with {heads} connected heads; mode {} needs {required_heads}",
            mode.name()
        ));
    }

    // The fixture first: a run that never reached Sophia proves nothing.
    let uevents = &records[only("sophia_qemu_unplug", "uevents")?];
    let hotplug = number(uevents, "drm_hotplug")?;
    let removed = number(uevents, "input_remove")?;
    let added = number(uevents, "input_add")?;
    let phases = if mode.returns() { 2 } else { 1 };
    let reached = if mode.display() {
        hotplug >= phases
    } else {
        removed >= 1 && added >= 1
    };
    if !reached {
        return Err(format!(
            "fixture unreached: drm_hotplug={hotplug} input_remove={removed} input_add={added} for mode {}",
            mode.name()
        ));
    }

    if let Some(fatal) = records
        .iter()
        .position(|record| record.name == "sophia_live_session_runtime_fatal")
    {
        return Err(format!(
            "ended in a runtime fatal: {}",
            log.lines().nth(fatal).unwrap_or("")
        ));
    }
    if let Some(failed) = records.iter().position(|record| {
        record.name.starts_with("sophia_qemu_") && record.get("status") == Some("failed")
    }) {
        return Err(format!(
            "contains a failure marker: {}",
            log.lines().nth(failed).unwrap_or("")
        ));
    }

    let sent = |action: &str| {
        records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.is("sophia_qemu_unplug", "sent") && record.get("action") == Some(action)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
    };
    let offs = sent("off");
    let ons = sent("on");
    let (Some(&first_off), Some(&last_off)) = (offs.first(), offs.last()) else {
        return Err("has no removal sent".to_owned());
    };
    if mode.returns() == ons.is_empty() || ons.first().is_some_and(|&on| on < last_off) {
        return Err("has its removals and returns out of order".to_owned());
    }
    // A removal sent after the session began to stop asked it nothing.
    if records[..first_off].iter().any(|record| {
        record.is("sophia_live_session_quiescence", "started")
            || record.is("sophia_live_session", "bounded_complete")
    }) {
        return Err(
            "fixture unreached: the removal came after the session began to stop".to_owned(),
        );
    }
    let loss_end = ons.first().copied().unwrap_or(records.len());

    let mut summary = vec![format!(
        "sophia_qemu_output_unplug_verdict schema=1 status=passed mode={} heads={heads} drm_hotplug={hotplug} input_remove={removed} input_add={added}",
        mode.name()
    )];
    if mode.display() {
        // The loss must show in what the session publishes, not only in a
        // session that happened to stay up.
        let loss = &records[first_off..loss_end];
        if mode == Mode::AllReturn {
            if !loss.iter().any(|record| record.name == TOPOLOGY) {
                return Err("shows no topology transition after every head went".to_owned());
            }
        } else {
            let (transition, settle) = published_and_settled(loss, heads - 1)
                .ok_or_else(|| format!("never published {} outputs after the loss", heads - 1))?;
            summary.push(format!(
                "sophia_qemu_output_unplug_verdict schema=1 status=loss_settled outputs={} transition={transition} settle={settle}",
                heads - 1
            ));
        }
        if let Some(&first_on) = ons.first() {
            let (transition, settle) = published_and_settled(&records[first_on..], heads)
                .ok_or_else(|| format!("never published {heads} outputs after the return"))?;
            summary.push(format!(
                "sophia_qemu_output_unplug_verdict schema=1 status=return_settled outputs={heads} transition={transition} settle={settle}"
            ));
        }
    }

    let last_action = ons.last().copied().unwrap_or(last_off);
    if !records[last_action..].iter().any(|record| {
        record.name == "sophia_live_session" && record.get("status") == Some("bounded_complete")
    }) {
        return Err("has no bounded completion after the last action".to_owned());
    }
    if records[running].get("client") == Some("dri3") {
        summary.push(verify_static_client(&records, first_off, last_action)?);
    }
    only("sophia_qemu_guest", "complete")?;
    only("sophia_qemu_unplug", "guest_exited")?;
    Ok(summary)
}

/// The first changed publication of `outputs` outputs, then the first record
/// that re-enabled input after it: a settlement, or a presentation timeout,
/// which also re-enables input and is named so a reader sees it. One hotplug
/// can rebuild twice (the kernel's uevent, then udev's), and the later,
/// unchanged rebuild may settle in the earlier one's place, so the settled
/// transition is the one whose publication last preceded the settlement, and
/// that publication must also show `outputs` outputs.
fn published_and_settled(records: &[Record], outputs: u32) -> Option<(&str, &str)> {
    let wanted = outputs.to_string();
    let published = records.iter().position(|record| {
        record.is(TOPOLOGY, "published")
            && record.get("outputs") == Some(wanted.as_str())
            && record.get("changed") == Some("true")
    })?;
    let settled = published
        + records[published..]
            .iter()
            .position(|record| record.name == TOPOLOGY && record.get("input") == Some("enabled"))?;
    let transition = records[settled].get("transition")?;
    let last_published = records[published..settled]
        .iter()
        .rev()
        .find(|record| record.is(TOPOLOGY, "published"))?;
    (last_published.get("transition") == Some(transition)
        && last_published.get("outputs") == Some(wanted.as_str()))
    .then(|| {
        records[settled]
            .get("status")
            .map(|status| (transition, status))
    })
    .flatten()
}

/// The static DMA-BUF client (t306): one Present, captured, promoted and
/// retired before any head was taken away, and none after. Before the loss
/// its window must show that frame, the first presented region of it, in
/// every region read. After the last action the window's content must still
/// be drawn from its retained image: a final composition region of the same
/// size and checksum, read from a renderer image, in a frame the same native
/// owner then presented (a page flip of that frame, or its synchronous first
/// modeset). Region readback proves rendered content and its presentation,
/// not a physical scanout.
fn verify_static_client(
    records: &[Record],
    first_off: usize,
    last_action: usize,
) -> Result<String, String> {
    let presents = records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record.name == "sophia_live_session_present" && record.get("status") == Some("retired")
        })
        .collect::<Vec<_>>();
    let [(present_at, present)] = presents.as_slice() else {
        return Err(format!(
            "static client: expected exactly one retired Present, found {}",
            presents.len()
        ));
    };
    if present.get("schema") != Some("2") {
        return Err(
            "static client: its Present did not retire on the mixed (DMA-BUF) path".to_owned(),
        );
    }
    let barrier = records
        .iter()
        .position(|record| record.is("sophia_qemu_unplug", "static_barrier"))
        .ok_or("static client: no barrier before the removal")?;
    if !(*present_at < barrier && barrier < first_off) {
        return Err("static client: the removal did not follow its retired Present".to_owned());
    }
    if records.iter().any(|record| {
        record.name == "sophia_live_renderer_image_handoff"
            && record.get("status") == Some("discarded")
    }) {
        return Err("static client: a handoff was discarded".to_owned());
    }
    let size = present
        .get("source")
        .ok_or("static client: its Present names no source size")?;
    let expected = size
        .split_once('x')
        .and_then(|(width, height)| Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?)))
        .ok_or("static client: its Present names no source size")?;
    // The probe takes dimensions 1..=4096 (tools/probes/dri3_layout.c,
    // geometry); anything else is not its frame, and is refused before the
    // checksum walks every pixel of it.
    if !(1..=4096).contains(&expected.0) || !(1..=4096).contains(&expected.1) {
        return Err(format!(
            "static client: its Present source {size} is outside the probe's 1..=4096 dimensions"
        ));
    }
    let is_window = |record: &Record| {
        record.is("sophia_native_composition_region_frame", "read")
            && record.get("source_stage") == Some("renderer_image")
            && region_size(record) == Some(size)
    };
    // Presented by the native owner that composed it: the frame this region
    // was queued as, within one owner (owner closings bound it), then either
    // that frame's page flip retiring after the region, or that frame's
    // bootstrap composition followed by the owner's publication, which
    // follows only its synchronous first modeset. Never a frame of another
    // owner, a retirement before the region, or another frame's.
    let closed = |record: &Record| record.is("sophia_live_native_owner", "closed");
    let presented = |at: usize| -> bool {
        let region = &records[at];
        let (Some(output), Some(head), Some(generation)) = (
            region.get("output"),
            region.get("head"),
            region.get("scene_generation"),
        ) else {
            return false;
        };
        let start = records[..at]
            .iter()
            .rposition(closed)
            .map_or(0, |index| index + 1);
        let end = records[at..]
            .iter()
            .position(closed)
            .map_or(records.len(), |index| at + index);
        let Some(frame) = records[start..at]
            .iter()
            .rev()
            .find(|queued| {
                queued.is("sophia_live_head_composition_queue", "queued")
                    && queued.get("output") == Some(output)
                    && queued.get("head") == Some(head)
                    && queued.get("scene_generation") == Some(generation)
            })
            .and_then(|queued| queued.get("frame"))
        else {
            return false;
        };
        let same_frame = |record: &Record| {
            record.get("output") == Some(output)
                && record.get("head") == Some(head)
                && record.get("frame") == Some(frame)
        };
        let after = &records[at + 1..end];
        let flipped = after.iter().any(|record| {
            record.is("sophia_live_native_head_page_flip", "retired") && same_frame(record)
        });
        let bootstrapped = after
            .iter()
            .position(|record| {
                record.is("sophia_live_head_bootstrap", "worker_composed") && same_frame(record)
            })
            .is_some_and(|composed| {
                after[composed..]
                    .iter()
                    .any(|record| record.is(TOPOLOGY, "published"))
            });
        flipped || bootstrapped
    };
    // The reference is the one submitted frame as first presented, before
    // the removal: never a later sample chosen because it matches. Every
    // other region of the window before the removal must show the same
    // pixels; a contradiction fails the run as an unstable baseline, whatever
    // is seen after the loss, and says whether that content was preserved.
    let before = (0..first_off)
        .filter(|&at| is_window(&records[at]))
        .collect::<Vec<_>>();
    let reference = before
        .iter()
        .copied()
        .find(|&at| presented(at))
        .map(|at| &records[at])
        .ok_or("static client: no presented region of its window before the removal")?;
    let checksum = reference
        .get("checksum")
        .ok_or("static client: region without checksum")?;
    // The reference must be the probe's frame itself, judged against what
    // that frame is, never against another sample of the run.
    let pixels = u64::from(expected.0) * u64::from(expected.1);
    let expected_checksum = probe_frame_checksum(expected.0, expected.1).to_string();
    if reference.get("region_pixels") != Some(pixels.to_string().as_str())
        || checksum != expected_checksum
    {
        return Err(format!(
            "static client: its first presented region is not the client's frame: region_pixels={} checksum={checksum}, expected region_pixels={pixels} checksum={expected_checksum}",
            reference.get("region_pixels").unwrap_or("?"),
        ));
    }
    let preserved = (last_action..records.len()).find(|&at| {
        is_window(&records[at]) && records[at].get("checksum") == Some(checksum) && presented(at)
    });
    if let Some(&unstable) = before
        .iter()
        .find(|&&at| records[at].get("checksum") != Some(checksum))
    {
        return Err(format!(
            "static client: unstable_baseline: a region of its window before the removal shows checksum={} nonzero_rgb_pixels={}, not the first presented checksum={checksum}; preserved_content={}",
            records[unstable].get("checksum").unwrap_or("?"),
            records[unstable].get("nonzero_rgb_pixels").unwrap_or("?"),
            if preserved.is_some() {
                "observed"
            } else {
                "not_observed"
            },
        ));
    }
    let after = preserved.map(|at| &records[at]).ok_or(
        "static client: its content was not drawn from a retained image after the last action",
    )?;
    Ok(format!(
        "sophia_qemu_output_unplug_verdict schema=1 status=static_content_retained size={size} checksum={checksum} baseline=stable target_after={}",
        after.get("target").unwrap_or("?")
    ))
}

/// The checksum the composition trace reports for the probe's first frame
/// drawn 1:1 at `width`x`height`: the probe's fill (tools/probes/dri3_layout.c,
/// buffer 0: four quadrant colours, split at `width / 2` and `height / 2`),
/// composed opaque (alpha forced to 1.0), read back by glReadPixels as RGBA
/// bytes from the region's bottom row up, and hashed 64-bit FNV-1a over every
/// byte (sophia-renderer-native-egl pixel_evidence.rs).
pub fn probe_frame_checksum(width: u32, height: u32) -> u64 {
    const COLOURS: [u32; 4] = [0xff26_384a, 0xff4a_3826, 0xff30_4538, 0xff43_344a];
    let mut checksum: u64 = 0xcbf2_9ce4_8422_2325;
    for y in (0..height).rev() {
        for x in 0..width {
            let quadrant = usize::from(x >= width / 2) + 2 * usize::from(y >= height / 2);
            let [_, red, green, blue] = COLOURS[quadrant].to_be_bytes();
            for byte in [red, green, blue, 0xff] {
                checksum ^= u64::from(byte);
                checksum = checksum.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    checksum
}

/// A region record's size, `WxH`, from its `target=WxH_X_Y`.
fn region_size(record: &Record) -> Option<&str> {
    record
        .get("target")
        .and_then(|target| target.split('_').next())
}
