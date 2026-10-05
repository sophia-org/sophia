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
