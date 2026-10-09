//! `cargo xtask conformance verify output-unplug MODE LOG`: the verdict on
//! the QEMU output-unplug scenario (t306).
//!
//! WHAT IT PROVES. While the session runs on scanned-out heads, the fixture
//! takes away one connector, every connector, or the keyboard, and in the
//! return modes gives it back. The session must live through it: no runtime
//! fatal, a published topology that reflects the loss, after a return one
//! that has every head again with input enabled, then a bounded completion and
//! a clean guest exit: the guest's completion of this scenario and QEMU's exit
//! status 0, in that order after the bounded completion.
//!
//! In the input mode the outputs stay, and the proof is at a client. A key
//! typed before the removal reaches Session from the virtio keyboard's first
//! identity and reaches the focused managed client as its keycode; after the
//! keyboard returns under a new identity, a different key reaches Session from
//! that identity and the client as its own keycode. Udev counts alone only
//! show that the fixture was reached.
//!
//! WHAT IT DOES NOT. virtio-gpu is not the operator's card, and the fixture
//! changes QEMU's display configuration instead of a physical link going down.
//! This proves the owner transition on a connector loss and return; the KVM
//! switch itself stays an attended test.
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
            // A first removal can publish a healthy smaller topology. Neither
            // that transition nor a host command proves that every head went.
            // Bind both independent witnesses to the completed removal phase.
            let all_gone = &records[last_off + 1..loss_end];
            let mut zero_connected = false;
            for sample in all_gone
                .iter()
                .filter(|record| record.is("sophia_qemu_unplug", "connectors"))
            {
                let keys = sample
                    .fields
                    .iter()
                    .map(|(key, _)| key)
                    .collect::<std::collections::BTreeSet<_>>();
                if keys.len() != sample.fields.len() {
                    return Err("connector observation has repeated fields".to_owned());
                }
                if sample.get("schema") != Some("1") {
                    return Err("connector observation has an unsupported schema".to_owned());
                }
                let connectors = number(sample, "connectors")?;
                let connected = number(sample, "connected")?;
                if connectors < heads || connected > connectors {
                    return Err("connector observation has inconsistent counts".to_owned());
                }
                zero_connected |= connected == 0;
            }
            if !zero_connected {
                return Err(
                    "fixture unreached: no zero-connected observation after the last removal and before the first return"
                        .to_owned(),
                );
            }
            if !all_gone
                .iter()
                .any(|record| record.is(TOPOLOGY, "unavailable"))
            {
                return Err(
                    "no unavailable topology after the last removal and before the first return"
                        .to_owned(),
                );
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

    // In the input mode the last action is the client's report of the
    // returned keyboard, and the managed client is the input witness, not a
    // static-content subject.
    let mut last_action = ons.last().copied().unwrap_or(last_off);
    if mode == Mode::InputReturn {
        if records[running].get("wm") != Some("true")
            || records[running].get("client") != Some("dri3")
        {
            return Err(
                "input return: was not run with the WM and the managed DRI3 client".to_owned(),
            );
        }
        let plain = log.lines().map(strip_ansi).collect::<Vec<_>>();
        let (line, routed) =
            verify_input_return(&records, &plain, (offs.as_slice(), ons.as_slice()))?;
        summary.push(line);
        last_action = routed;
    }
    if !records[last_action..].iter().any(|record| {
        record.name == "sophia_live_session" && record.get("status") == Some("bounded_complete")
    }) {
        return Err("has no bounded completion after the last action".to_owned());
    }
    if mode != Mode::InputReturn && records[running].get("client") == Some("dri3") {
        summary.push(verify_static_client(&records, first_off, last_action)?);
    }
    // The clean guest exit: the guest's own completion of this scenario, then
    // the host's record of QEMU's exit status 0, in that order after the
    // session's bounded completion.
    let complete = only("sophia_qemu_guest", "complete")?;
    let exited = only("sophia_qemu_unplug", "guest_exited")?;
    if !exact_fields(
        &records[complete],
        &[
            ("schema", "1"),
            ("status", "complete"),
            ("scenario", "output-unplug"),
        ],
    ) {
        return Err(
            "guest completion is not this scenario's: expected scenario=output-unplug only"
                .to_owned(),
        );
    }
    if !exact_fields(
        &records[exited],
        &[
            ("schema", "1"),
            ("status", "guest_exited"),
            ("qemu_exit", "0"),
        ],
    ) {
        return Err("guest exit is not a clean QEMU exit: expected qemu_exit=0 only".to_owned());
    }
    let bounded = records
        .iter()
        .rposition(|record| record.is("sophia_live_session", "bounded_complete"))
        .unwrap_or(0);
    if !(bounded < complete && complete < exited) {
        return Err(
            "guest completion and exit do not follow the bounded completion in order".to_owned(),
        );
    }
    Ok(summary)
}

/// Whether the record carries exactly these fields, each once, in any order.
fn exact_fields(record: &Record, expected: &[(&str, &str)]) -> bool {
    record.fields.len() == expected.len()
        && expected.iter().all(|&(key, value)| {
            record
                .fields
                .iter()
                .filter(|(name, _)| name.as_str() == key)
                .count()
                == 1
                && record.get(key) == Some(value)
        })
}

/// Whether the record has schema 1, its status, and each of `names` exactly
/// once, and no other field.
fn named_fields(record: &Record, names: &[&str]) -> bool {
    let count = |key: &str| {
        record
            .fields
            .iter()
            .filter(|(name, _)| name.as_str() == key)
            .count()
    };
    record.fields.len() == names.len() + 2
        && record.get("schema") == Some("1")
        && ["schema", "status"]
            .iter()
            .chain(names)
            .all(|&key| count(key) == 1)
}

const INPUT_DEVICE: &str = "sophia_live_session_input_device";
const CLIENT_KEY: &str = "dri3_layout stage=key ";
const CLIENT_FOCUS: &str = "dri3_layout stage=focus state=";
/// The X keycodes (evdev code + 8) of the keys the host types: KEY_A at the
/// baseline, KEY_B after the return. Distinct, so a buffered baseline key
/// cannot stand for the return.
const BASELINE_KEYCODE: &str = "38";
const RETURN_KEYCODE: &str = "56";

/// The input mode's ordered chain (tools/qemu_guest_init.sh,
/// input_return_drive; tools/qemu_session_harness.sh, input_key). K0 is the
/// one virtio keyboard (the kernel's virtual bus) Session admitted before its
/// readiness; the managed client holds the focus; the host types "a" in the
/// baseline phase and Session's first key from K0 and the client's keycode 38
/// both fall inside it; the guest unbinds the keyboard and Session removes K0;
/// the guest binds it again and Session admits exactly one new virtio
/// keyboard K1; the host types "b" and Session's first key from K1 and the
/// client's keycode 56 both fall inside that phase. A phase runs from the
/// host's sending line to the guest's phase marker, which the guest prints
/// only after both observations; the host's completed line may follow the
/// observations, since QMP can deliver before the helper returns, but must
/// come before the session's bounded completion. Keys the client reports as
/// SendEvents and the client's own failure lines are refused, and every input
/// record read is validated before any identity is bound. Session's
/// and the client's lines have no order between them. Any other key, focus
/// loss at readiness, keyboard, removal, overflow or failed send is refused.
/// Returns the summary line and the index of the last link.
fn verify_input_return(
    records: &[Record],
    plain: &[String],
    (offs, ons): (&[usize], &[usize]),
) -> Result<(String, usize), String> {
    let fail =
        |what: String| -> Result<(String, usize), String> { Err(format!("input return: {what}")) };
    let unplug = |status: &str, key: &str, value: &str| -> Vec<usize> {
        records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.is("sophia_qemu_unplug", status)
                    && (key.is_empty() || record.get(key) == Some(value))
            })
            .map(|(index, _)| index)
            .collect()
    };
    let one = |indices: Vec<usize>, what: &str| -> Result<usize, String> {
        match indices.as_slice() {
            [index] => Ok(*index),
            other => Err(format!(
                "input return: expected exactly one {what}, found {}",
                other.len()
            )),
        }
    };
    // Every input record the chain reads is validated before any is bound:
    // schema 1, each field once, and exactly its emitter's fields, with a
    // numeric device.
    for record in records {
        let expected: &[&str] = match (record.name.as_str(), record.get("status")) {
            (INPUT_DEVICE, Some("added")) => &[
                "device", "keyboard", "pointer", "touch", "virtual", "source",
            ],
            (INPUT_DEVICE, Some("removed")) => &["device", "released"],
            (INPUT_DEVICE, Some("key_observed")) => &["device"],
            (
                "sophia_qemu_unplug",
                Some(
                    "input_baseline_ready"
                    | "input_removed"
                    | "input_return_ready"
                    | "input_return_routed",
                ),
            ) => &["device"],
            ("sophia_qemu_unplug", Some("input_baseline")) => &["device", "routed"],
            ("sophia_qemu_unplug", Some("key_sending")) => &["phase", "key"],
            ("sophia_qemu_unplug", Some("key_sent"))
                if record.get("result") == Some("completed") =>
            {
                &["phase", "result"]
            }
            ("sophia_qemu_unplug", Some("key_sent")) => &["phase", "result", "exit"],
            ("sophia_qemu_unplug", Some("sent")) => &["action", "target"],
            _ => continue,
        };
        let numeric = record
            .get("device")
            .is_none_or(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()));
        if !named_fields(record, expected) || !numeric {
            return fail(format!(
                "a malformed {} status={} record",
                record.name,
                record.get("status").unwrap_or("")
            ));
        }
    }
    // The client's own failure lines (tools/probes/dri3_layout.c, fail and
    // x_error) are not Session records; any of them refuses the run, as does
    // a failed finish.
    if let Some(line) = plain.iter().find(|line| {
        line.starts_with("dri3_layout status=failed ")
            || line.starts_with("dri3_layout event=finished result=fail ")
    }) {
        return fail(format!("the client reported a failure: {line}"));
    }
    let virtio_keyboard = |record: &Record| {
        record.is(INPUT_DEVICE, "added")
            && record.get("keyboard") == Some("true")
            && record.get("virtual") == Some("true")
    };
    let keyboards = records
        .iter()
        .enumerate()
        .filter(|(_, record)| virtio_keyboard(record))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let ready = one(
        records
            .iter()
            .enumerate()
            .filter(|(_, record)| record.is("sophia_live_session_startup", "ready"))
            .map(|(index, _)| index)
            .collect(),
        "Session readiness",
    )?;
    let [k0_at, k1_at] = keyboards.as_slice() else {
        return fail(format!(
            "expected one virtio keyboard before readiness and one after the return, found {}",
            keyboards.len()
        ));
    };
    let (k0_at, k1_at) = (*k0_at, *k1_at);
    let device = |index: usize| records[index].get("device").unwrap_or("");
    let (k0, k1) = (device(k0_at), device(k1_at));
    if k0.is_empty() || k1.is_empty() || k0 == k1 {
        return fail(format!(
            "the returned keyboard {k1:?} is not a new identity beside {k0:?}"
        ));
    }
    if records
        .iter()
        .filter(|record| record.is("sophia_qemu_unplug", "records_overflow"))
        .count()
        != 0
        || plain
            .iter()
            .any(|line| line.starts_with("dri3_layout stage=key_overflow "))
    {
        return fail("a record copy or the client's key reports overflowed".to_owned());
    }

    // Each phase marker once in the whole log, then bound to its keyboard.
    let marker = |status: &str, id: &str, which: &str| -> Result<usize, String> {
        let at = one(unplug(status, "", ""), status)?;
        if records[at].get("device") != Some(id) {
            return Err(format!(
                "input return: {status} names another device than {which}"
            ));
        }
        Ok(at)
    };
    let baseline_ready = marker("input_baseline_ready", k0, "K0")?;
    let baseline = marker("input_baseline", k0, "K0")?;
    let removed_marker = marker("input_removed", k0, "K0")?;
    let return_ready = marker("input_return_ready", k1, "K1")?;
    let routed = marker("input_return_routed", k1, "K1")?;
    if records[baseline].get("routed") != Some("yes") {
        return fail("the baseline was not routed".to_owned());
    }
    let sending = |phase: &str| {
        one(
            unplug("key_sending", "phase", phase),
            &format!("key_sending phase={phase}"),
        )
    };
    let completed = |phase: &str| {
        one(
            unplug("key_sent", "phase", phase),
            &format!("key_sent phase={phase}"),
        )
    };
    let (sending_baseline, sending_return) = (sending("baseline")?, sending("return")?);
    let (sent_baseline, sent_return) = (completed("baseline")?, completed("return")?);
    if unplug("key_sending", "", "").len() != 2 || unplug("key_sent", "", "").len() != 2 {
        return fail("the host sent other keys than one per phase".to_owned());
    }
    for (at, key) in [(sending_baseline, "a"), (sending_return, "b")] {
        if records[at].get("key") != Some(key) {
            return fail(format!("the host typed another key than {key:?}"));
        }
    }
    for at in [sent_baseline, sent_return] {
        if records[at].get("result") != Some("completed") {
            return fail("a key send did not complete".to_owned());
        }
    }
    let ([off], [on]) = (offs, ons) else {
        return fail(format!(
            "expected one keyboard removal and one return, found {} and {}",
            offs.len(),
            ons.len()
        ));
    };
    let (off, on) = (*off, *on);

    // Session's own records for the two keyboards: K0's first key, its
    // removal, K1's first key. Nothing from another device.
    let session = |status: &str| -> Vec<usize> {
        records
            .iter()
            .enumerate()
            .filter(|(_, record)| record.is(INPUT_DEVICE, status))
            .map(|(index, _)| index)
            .collect()
    };
    let observed = session("key_observed");
    let removed = session("removed");
    if observed
        .iter()
        .any(|&at| device(at) != k0 && device(at) != k1)
    {
        return fail("a key was observed on another device".to_owned());
    }
    if removed.iter().any(|&at| device(at) != k0) {
        return fail("another input device was removed".to_owned());
    }
    let of = |indices: &[usize], id: &str, what: &str| {
        one(
            indices
                .iter()
                .copied()
                .filter(|&at| device(at) == id)
                .collect(),
            what,
        )
    };
    let observed_k0 = of(&observed, k0, "key_observed for K0")?;
    let observed_k1 = of(&observed, k1, "key_observed for K1")?;
    let removed_k0 = of(&removed, k0, "removal of K0")?;

    // The client's reports: focus held at readiness, then exactly the two keys.
    let holding = plain
        .iter()
        .position(|line| line.starts_with("dri3_layout stage=holding "))
        .ok_or("input return: the client never held")?;
    match plain[..baseline_ready]
        .iter()
        .rev()
        .find(|line| line.starts_with(CLIENT_FOCUS))
    {
        Some(line)
            if line == "dri3_layout stage=focus state=in source=query"
                || (line.starts_with("dri3_layout stage=focus state=in source=event ")
                    && line.ends_with(" synthetic=0")) => {}
        _ => {
            return fail(
                "the client did not hold the focus when the baseline was ready".to_owned(),
            );
        }
    }
    // Each key report names its X keycode and whether it was a SendEvent
    // (synthetic=1). A synthetic key is no routing witness and is refused.
    let mut keys = Vec::new();
    for (index, line) in plain
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with(CLIENT_KEY))
    {
        let Some((keycode, synthetic)) = line
            .strip_prefix("dri3_layout stage=key keycode=")
            .and_then(|rest| rest.split_once(" synthetic="))
        else {
            return fail(format!("a malformed client key report: {line}"));
        };
        match synthetic {
            "0" => keys.push((index, keycode)),
            "1" => return fail(format!("a synthetic key reached the client: {line}")),
            _ => return fail(format!("a malformed client key report: {line}")),
        }
    }
    let [
        (client_baseline, baseline_keycode),
        (client_return, return_keycode),
    ] = keys.as_slice()
    else {
        return fail(format!(
            "expected exactly two client key reports, found {}",
            keys.len()
        ));
    };
    if *baseline_keycode != BASELINE_KEYCODE || *return_keycode != RETURN_KEYCODE {
        return fail(format!(
            "the client reported keycodes {baseline_keycode} then {return_keycode}, expected {BASELINE_KEYCODE} then {RETURN_KEYCODE}"
        ));
    }
    let within = |at: usize, (start, end): (usize, usize)| start < at && at < end;
    let baseline_phase = (sending_baseline, baseline);
    let return_phase = (sending_return, routed);
    if !(within(observed_k0, baseline_phase) && within(*client_baseline, baseline_phase)) {
        return fail(
            "the baseline key was not observed by Session and the client inside its phase"
                .to_owned(),
        );
    }
    if !(within(observed_k1, return_phase) && within(*client_return, return_phase)) {
        return fail("the returned keyboard's key was not observed by Session and the client inside its phase".to_owned());
    }
    let chain = [
        ("K0 admitted", k0_at),
        ("Session readiness", ready),
        ("baseline ready", baseline_ready),
        ("baseline key sending", sending_baseline),
        ("baseline routed", baseline),
        ("keyboard off", off),
        ("K0 removed", removed_k0),
        ("removal marked", removed_marker),
        ("keyboard on", on),
        ("K1 admitted", k1_at),
        ("return ready", return_ready),
        ("return key sending", sending_return),
        ("return routed", routed),
    ];
    if let Some(pair) = chain.windows(2).find(|pair| pair[0].1 >= pair[1].1) {
        return fail(format!("{} does not precede {}", pair[0].0, pair[1].0));
    }
    if holding >= baseline_ready {
        return fail("the baseline was ready before the client held".to_owned());
    }
    let bounded = records
        .iter()
        .position(|record| record.is("sophia_live_session", "bounded_complete"))
        .unwrap_or(records.len());
    if !(sending_baseline < sent_baseline
        && sent_baseline < sending_return
        && sending_return < sent_return
        && sent_return < bounded)
    {
        return fail("a key send completed outside its phase".to_owned());
    }
    Ok((
        format!(
            "sophia_qemu_output_unplug_verdict schema=1 status=input_routed baseline_device={k0} return_device={k1} baseline_keycode={BASELINE_KEYCODE} return_keycode={RETURN_KEYCODE}"
        ),
        routed,
    ))
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
