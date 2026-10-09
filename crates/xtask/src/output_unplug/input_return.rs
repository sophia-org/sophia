//! The input-return mode's ordered chain: the virtio keyboard Session admitted, a routed key at a
//! focused managed client, the keyboard's unbind and bind by the guest, and a routed key from the
//! keyboard's new identity. The parent module owns record parsing and the shared field checks.

use super::{Record, named_fields};

const INPUT_DEVICE: &str = "sophia_live_session_input_device";
const CLIENT_KEY: &str = "dri3_layout stage=key ";
const CLIENT_FOCUS: &str = "dri3_layout stage=focus state=";
/// The X keycodes (evdev code + 8) of the keys the host types: KEY_A at the
/// baseline, KEY_B after the return. Distinct, so a buffered baseline key
/// cannot stand for the return.
const BASELINE_KEYCODE: &str = "38";
const RETURN_KEYCODE: &str = "56";

/// The input mode's ordered chain (tools/qemu_guest_init.sh, input_return_drive
/// and unplug_keyboard; tools/qemu_session_harness.sh, input_key). K0 is the one
/// virtio keyboard (the kernel's virtual bus) Session admitted before its
/// readiness; the managed client holds the focus; the host types "a" in the
/// baseline phase and Session's first key from K0 and the client's keycode 38
/// both fall inside it; after the guest names its unbind attempt Session removes
/// K0, and after it names its bind attempt Session admits exactly one new virtio
/// keyboard K1; the host types "b" and Session's first key from K1 and the
/// client's keycode 56 both fall inside that phase. A phase runs from the host's
/// sending line to the guest's phase marker, which the guest prints only after
/// both observations; the host's completed line may follow the observations,
/// since QMP can deliver before the helper returns, but must come before the
/// session's bounded completion. Keys the client reports as SendEvents and the
/// client's own failure lines are refused, and every input record read is
/// validated before any identity is bound. Session's and the client's lines have
/// no order between them. Any other key, focus loss at readiness, keyboard,
/// removal, overflow or failed send is refused. Returns the summary line and the
/// index of the last link.
pub(super) fn verify_input_return(
    records: &[Record],
    plain: &[String],
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
            ("sophia_qemu_unplug", Some("sending" | "sent")) => &["action", "target"],
            _ => continue,
        };
        let numeric = record
            .get("device")
            .is_none_or(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()));
        // A keyboard attempt or completion names off or on and one virtio
        // device; any other action is refused, never ignored.
        let device = record
            .get("target")
            .and_then(|target| target.strip_prefix("virtio"));
        let keyboard_action = !matches!(record.get("status"), Some("sending" | "sent"))
            || (matches!(record.get("action"), Some("off" | "on"))
                && device.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())));
        if !named_fields(record, expected) || !numeric || !keyboard_action {
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
    // The keyboard actions (tools/qemu_guest_init.sh, unplug_keyboard): each
    // attempt is printed before its write and each completion after a
    // successful one, exactly once for off and once for on, all naming the one
    // virtio device that is unbound and bound again.
    if unplug("sending", "", "").len() != 2 || unplug("sent", "", "").len() != 2 {
        return fail("expected exactly two keyboard attempts and two completions".to_owned());
    }
    let action = |status: &str, which: &str| {
        one(
            unplug(status, "action", which),
            &format!("{status} action={which}"),
        )
    };
    let (sending_off, sending_on) = (action("sending", "off")?, action("sending", "on")?);
    let (off, on) = (action("sent", "off")?, action("sent", "on")?);
    let target = records[sending_off].get("target");
    if [sending_on, off, on]
        .iter()
        .any(|&at| records[at].get("target") != target)
    {
        return fail("the keyboard attempts and completions name different devices".to_owned());
    }

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
        ("keyboard off attempted", sending_off),
        ("K0 removed", removed_k0),
        ("removal marked", removed_marker),
        ("keyboard on attempted", sending_on),
        ("K1 admitted", k1_at),
        ("return ready", return_ready),
        ("return key sending", sending_return),
        ("return routed", routed),
    ];
    if let Some(pair) = chain.windows(2).find(|pair| pair[0].1 >= pair[1].1) {
        return fail(format!("{} does not precede {}", pair[0].0, pair[1].0));
    }
    // Session's record of the removal or the return may reach the log on
    // either side of the guest's completion line, since the kernel acts inside
    // the write; the completion itself must follow its attempt and precede the
    // guest's own marker for that phase, which the same shell prints later.
    if !(sending_off < off && off < removed_marker) {
        return fail("the keyboard removal completed outside its attempt window".to_owned());
    }
    if !(sending_on < on && on < return_ready) {
        return fail("the keyboard return completed outside its attempt window".to_owned());
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
