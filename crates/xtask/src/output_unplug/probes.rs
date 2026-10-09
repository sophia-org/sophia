//! The two-probe fixture of the t306 qualification runs (opt-in, `probes=2` on
//! the running record): one static DRI3 probe declared per connector, each with
//! its own geometry and so its own expected frame. The proof is per head.
//!
//! - Owner maps come only from `sophia_live_native_owner_head`, emitted for an
//!   opened owner under that owner's own epoch. A map with an `unavailable`
//!   record, a duplicate head or connector, or a record for another owner is
//!   incomplete or refused; neighbouring ready lines are never joined.
//! - Each probe's baseline is its first presented region before the first
//!   removal, on the head the startup owner maps its connector to, and must be
//!   the probe's own frame; every earlier region of that probe must agree.
//!   Each probe is judged alone, so one probe's good pixels cannot hide the
//!   other's unstable baseline.
//! - After the return, the accepted owner is the one whose lifetime holds the
//!   return's publication and settlement (or, for the keyboard-only run, the
//!   one owner of the run). A fixture repaint (one committed hold-shift action
//!   after the return, with no client Present) moves both windows; each probe
//!   must then be presented at the shifted crop, with its own frame, on the head
//!   the accepted owner maps its connector to, in a frame of that owner queued
//!   after the action settled. Reused output or head numbers of an older owner
//!   never satisfy it.

use super::{Record, presented_frame, probe_frame_checksum, region_size};

const OWNER: &str = "sophia_live_native_owner";
const OWNER_HEAD: &str = "sophia_live_native_owner_head";
const PROBE: &str = "sophia_qemu_probe";
const REGION: &str = "sophia_native_composition_region_frame";

/// One declared probe: identity, size `WxH` and the connector its output is.
#[derive(Clone, Debug)]
pub(super) struct Probe {
    pub(super) id: String,
    pub(super) size: String,
    pub(super) connector: String,
    pub(super) checksum: String,
}

/// An opened owner: its epoch, lifetime and, if complete, connector -> (output, head).
#[derive(Debug)]
pub(super) struct Owner {
    pub(super) epoch: String,
    pub(super) opened: usize,
    pub(super) closed: usize,
    pub(super) heads: Option<Vec<(String, String, String)>>,
}

impl Owner {
    pub(super) fn head(&self, connector: &str) -> Result<(&str, &str), String> {
        let heads = self
            .heads
            .as_ref()
            .ok_or_else(|| format!("owner {} has no complete head map", self.epoch))?;
        heads
            .iter()
            .find(|(name, _, _)| name == connector)
            .map(|(_, output, head)| (output.as_str(), head.as_str()))
            .ok_or_else(|| {
                format!(
                    "owner {} maps no head for connector {connector}",
                    self.epoch
                )
            })
    }

    pub(super) fn holds(&self, at: usize) -> bool {
        self.opened < at && at < self.closed
    }
}

/// The declared probes, exactly two, each with its own geometry and connector.
pub(super) fn probes(records: &[Record], before: usize) -> Result<Vec<Probe>, String> {
    let mut probes = Vec::new();
    for (at, record) in records.iter().enumerate() {
        if record.name != PROBE {
            continue;
        }
        if at >= before
            || record.get("status") != Some("declared")
            || !exact(
                record,
                &["schema", "status", "probe", "geometry", "connector"],
            )
        {
            return Err(
                "probes: a probe record is not one declaration before the removal".to_owned(),
            );
        }
        let size = record.get("geometry").unwrap_or("");
        let (width, height) = size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
            .filter(|&(w, h)| (1..=4096).contains(&w) && (1..=4096).contains(&h))
            .ok_or_else(|| format!("probes: geometry {size} is not a probe size"))?;
        probes.push(Probe {
            id: record.get("probe").unwrap_or("").to_owned(),
            size: size.to_owned(),
            connector: record.get("connector").unwrap_or("").to_owned(),
            checksum: probe_frame_checksum(width, height).to_string(),
        });
    }
    let distinct = |key: fn(&Probe) -> &str| {
        probes.len() == 2
            && key(&probes[0]) != key(&probes[1])
            && !key(&probes[0]).is_empty()
            && !key(&probes[1]).is_empty()
    };
    if !(distinct(|p| &p.id) && distinct(|p| &p.size) && distinct(|p| &p.connector)) {
        return Err(
            "probes: expected two probes with distinct identities, geometries and connectors"
                .to_owned(),
        );
    }
    Ok(probes)
}

/// Every opened owner with its explicit head map.
pub(super) fn owners(records: &[Record]) -> Result<Vec<Owner>, String> {
    let mut owners: Vec<Owner> = Vec::new();
    for (at, record) in records.iter().enumerate() {
        if record.is(OWNER, "opened") {
            if owners.last().is_some_and(|owner| owner.closed > at) {
                return Err("owners: an owner opened before the previous one closed".to_owned());
            }
            let epoch = record.get("epoch").unwrap_or("").to_owned();
            if owners.iter().any(|owner| owner.epoch == epoch) {
                return Err(format!("owners: epoch {epoch} opened twice"));
            }
            owners.push(Owner {
                epoch,
                opened: at,
                closed: usize::MAX,
                heads: Some(Vec::new()),
            });
        } else if record.is(OWNER, "closed") {
            let owner = owners
                .last_mut()
                .filter(|owner| {
                    owner.closed == usize::MAX && record.get("epoch") == Some(owner.epoch.as_str())
                })
                .ok_or("owners: a close names no open owner")?;
            owner.closed = at;
        } else if record.name == OWNER_HEAD {
            let owner = owners
                .last_mut()
                .filter(|owner| {
                    owner.closed == usize::MAX && record.get("epoch") == Some(owner.epoch.as_str())
                })
                .ok_or("owners: a head map record names no open owner")?;
            match record.get("status") {
                Some("mapped")
                    if exact(
                        record,
                        &[
                            "schema",
                            "status",
                            "epoch",
                            "output",
                            "head",
                            "connector",
                            "connector_id",
                        ],
                    ) =>
                {
                    if let Some(heads) = owner.heads.as_mut() {
                        let (output, head, connector) = (
                            record.get("output").unwrap_or("").to_owned(),
                            record.get("head").unwrap_or("").to_owned(),
                            record.get("connector").unwrap_or("").to_owned(),
                        );
                        if heads
                            .iter()
                            .any(|(c, o, h)| c == &connector || (o == &output && h == &head))
                        {
                            return Err(format!(
                                "owners: owner {} maps a head or connector twice",
                                owner.epoch
                            ));
                        }
                        heads.push((connector, output, head));
                    }
                }
                Some("unavailable") if exact(record, &["schema", "status", "epoch"]) => {
                    owner.heads = None
                }
                _ => return Err("owners: a head map record is malformed".to_owned()),
            }
        }
    }
    for owner in &mut owners {
        if owner.heads.as_ref().is_some_and(Vec::is_empty) {
            owner.heads = None;
        }
    }
    Ok(owners)
}

/// The owner whose lifetime holds `at`.
pub(super) fn owner_at(owners: &[Owner], at: usize) -> Result<&Owner, String> {
    owners
        .iter()
        .find(|owner| owner.holds(at))
        .ok_or_else(|| "owners: no owner holds the anchor".to_owned())
}

fn is_probe_region(record: &Record, probe: &Probe) -> bool {
    record.is(REGION, "read")
        && record.get("source_stage") == Some("renderer_image")
        && region_size(record) == Some(probe.size.as_str())
}

/// Each probe's baseline on its startup head: the first presented region before
/// the removal is its own frame and every earlier region of it agrees.
pub(super) fn baselines(
    records: &[Record],
    probes: &[Probe],
    owners: &[Owner],
    first_off: usize,
) -> Result<String, String> {
    let startup = owner_at(owners, first_off)?;
    let mut lines = Vec::new();
    for probe in probes {
        let (output, head) = startup.head(&probe.connector)?;
        let reference = (startup.opened..first_off)
            .find(|&at| {
                let r = &records[at];
                is_probe_region(r, probe)
                    && r.get("output") == Some(output)
                    && r.get("head") == Some(head)
                    && presented_frame(records, at).is_some()
            })
            .ok_or_else(|| {
                format!(
                    "probe {}: no presented region on head {head} before the removal",
                    probe.id
                )
            })?;
        if records[reference].get("checksum") != Some(probe.checksum.as_str()) {
            return Err(format!(
                "probe {}: its first presented region is not its frame",
                probe.id
            ));
        }
        if let Some(unstable) = (0..first_off).find(|&at| {
            is_probe_region(&records[at], probe)
                && records[at].get("checksum") != Some(probe.checksum.as_str())
        }) {
            return Err(format!(
                "probe {}: unstable_baseline: a region before the removal shows checksum={}",
                probe.id,
                records[unstable].get("checksum").unwrap_or("?")
            ));
        }
        lines.push(format!("{}@{}", probe.id, probe.connector));
    }
    Ok(lines.join(","))
}

/// The one committed fixture repaint after `anchor`: (action settled index, offset_x, scene generation).
pub(super) fn repaint(records: &[Record], anchor: usize) -> Result<(usize, u64, u64), String> {
    let sending = (anchor..records.len())
        .find(|&at| records[at].is("sophia_qemu_unplug", "repaint_sending"))
        .ok_or("repaint: no fixture repaint after the return")?;
    let committed = (sending..records.len())
        .filter(|&at| {
            records[at].name == "sophia_shell_action_policy"
                && records[at].get("action") == Some("1")
                && records[at].get("outcome") == Some("Committed")
        })
        .collect::<Vec<_>>();
    let [policy] = committed.as_slice() else {
        return Err(format!(
            "repaint: expected exactly one committed hold-shift action, found {}",
            committed.len()
        ));
    };
    let transaction = records[*policy]
        .get("transaction")
        .ok_or("repaint: action without transaction")?;
    let proposed = (sending..records.len())
        .find(|&at| {
            records[at].is("sophia_qemu_wm_hold", "proposed")
                && records[at].get("transaction") == Some(transaction)
        })
        .ok_or("repaint: the WM proposed no shift for the action")?;
    let offset = records[proposed]
        .get("offset_x")
        .and_then(|x| x.parse::<u64>().ok())
        .ok_or("repaint: the shift names no offset")?;
    let settled = (*policy..records.len())
        .find(|&at| {
            records[at].is("sophia_live_wm_chrome", "settled")
                && records[at].get("transaction") == Some(transaction)
                && records[at].get("outcome") == Some("Committed")
        })
        .ok_or("repaint: Session settled no shift for the action")?;
    let generation = records[settled]
        .get("scene_generation")
        .and_then(|g| g.parse::<u64>().ok())
        .ok_or("repaint: the settlement names no scene generation")?;
    Ok((settled, offset, generation))
}

/// Each probe presented with its own frame at the shifted crop on the head
/// `owner` maps its connector to, in a frame of `owner` after the repaint.
pub(super) fn returned_heads(
    records: &[Record],
    probes: &[Probe],
    owner: &Owner,
    repaint: (usize, u64, u64),
) -> Result<String, String> {
    let (settled, offset, generation) = repaint;
    if !owner.holds(settled) {
        return Err("returned heads: the repaint settled outside the accepted owner".to_owned());
    }
    let mut lines = Vec::new();
    for probe in probes {
        let (output, head) = owner.head(&probe.connector)?;
        let target = format!("{}_{offset}_0", probe.size);
        let sample = (settled + 1..owner.closed.min(records.len()))
            .find(|&at| {
                let r = &records[at];
                is_probe_region(r, probe)
                    && r.get("output") == Some(output)
                    && r.get("head") == Some(head)
                    && r.get("target") == Some(target.as_str())
                    && r.get("scene_generation").and_then(|g| g.parse::<u64>().ok()).is_some_and(|g| g >= generation)
                    && presented_frame(records, at).is_some()
            })
            .ok_or_else(|| format!("probe {}: not presented at {target} on returned head {head} of owner {} after the repaint", probe.id, owner.epoch))?;
        if records[sample].get("checksum") != Some(probe.checksum.as_str()) {
            return Err(format!(
                "probe {}: returned head {head} shows checksum={} after the repaint, not its frame",
                probe.id,
                records[sample].get("checksum").unwrap_or("?")
            ));
        }
        lines.push(format!("{}@{}={head}", probe.id, probe.connector));
    }
    Ok(lines.join(","))
}

/// No client Present after the two probes' own.
pub(super) fn static_clients(records: &[Record], barrier: usize) -> Result<(), String> {
    let presents = records
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.name == "sophia_live_session_present" && r.get("status") == Some("retired")
        })
        .map(|(at, _)| at)
        .collect::<Vec<_>>();
    if presents.len() != 2 || presents.iter().any(|&at| at > barrier) {
        return Err(format!(
            "probes: expected exactly two client Presents before the barrier, found {}",
            presents.len()
        ));
    }
    Ok(())
}

fn exact(record: &Record, names: &[&str]) -> bool {
    record.fields.len() == names.len()
        && names
            .iter()
            .all(|name| record.fields.iter().filter(|(key, _)| key == name).count() == 1)
}

/// A probe's own client line `dri3_layout probe=ID stage=...`, as (probe, rest).
pub(super) fn client_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("dri3_layout probe=")?;
    rest.split_once(' ')
}

/// The probe that holds the focus at `at`: its last focus line before `at` is
/// `state=in`, and no other probe's last focus line is.
pub(super) fn focused(plain: &[String], at: usize) -> Result<String, String> {
    let mut last: Vec<(String, bool)> = Vec::new();
    for line in &plain[..at] {
        if let Some((probe, rest)) = client_line(line)
            && let Some(state) = rest.strip_prefix("stage=focus state=")
        {
            let focused = state.starts_with("in");
            match last.iter_mut().find(|(id, _)| id == probe) {
                Some(entry) => entry.1 = focused,
                None => last.push((probe.to_owned(), focused)),
            }
        }
    }
    let holders = last
        .iter()
        .filter(|(_, focused)| *focused)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    match holders.as_slice() {
        [one] => Ok(one.clone()),
        other => Err(format!(
            "input: expected one focused probe, found {}",
            other.len()
        )),
    }
}

/// The post-return key: the host's one `key_sending phase=returned` after
/// `after`, Session's key from a keyboard it admitted and still holds, the
/// focused probe's keycode, no key at the other probe, and the host's completed
/// send. Returns the summary line and the index of the last link.
pub(super) fn returned_key(
    records: &[Record],
    plain: &[String],
    after: usize,
    keycode: &str,
) -> Result<(String, usize), String> {
    let sending = (after..records.len())
        .filter(|&at| {
            records[at].is("sophia_qemu_unplug", "key_sending")
                && records[at].get("phase") == Some("returned")
        })
        .collect::<Vec<_>>();
    let [sending] = sending.as_slice() else {
        return Err(format!(
            "input: expected one returned-phase key after the return, found {}",
            sending.len()
        ));
    };
    let target = focused(plain, *sending)?;
    let observed = (*sending..records.len())
        .find(|&at| records[at].is("sophia_live_session_input_device", "key_observed"))
        .ok_or("input: Session observed no key after the returned-phase send")?;
    let device = records[observed].get("device").unwrap_or("");
    let added = records[..observed].iter().rposition(|r| {
        r.is("sophia_live_session_input_device", "added")
            && r.get("device") == Some(device)
            && r.get("keyboard") == Some("true")
    });
    let removed = records[..observed].iter().rposition(|r| {
        r.is("sophia_live_session_input_device", "removed") && r.get("device") == Some(device)
    });
    if added.is_none() || removed.is_some_and(|removed| Some(removed) > added) {
        return Err(format!(
            "input: the returned key came from device {device}, not an admitted keyboard"
        ));
    }
    let mut routed = None;
    for (at, line) in plain.iter().enumerate().skip(*sending) {
        if let Some((probe, rest)) = client_line(line)
            && rest.starts_with("stage=key ")
        {
            if probe != target {
                return Err(format!(
                    "input: probe {probe} received a key; probe {target} holds the focus"
                ));
            }
            if rest != format!("stage=key keycode={keycode} synthetic=0") || routed.is_some() {
                return Err(format!(
                    "input: probe {target} reported {rest:?}, expected one keycode {keycode}"
                ));
            }
            routed = Some(at);
        }
    }
    let routed =
        routed.ok_or_else(|| format!("input: probe {target} reported no key after the return"))?;
    let sent = (*sending..records.len())
        .find(|&at| {
            records[at].is("sophia_qemu_unplug", "key_sent")
                && records[at].get("phase") == Some("returned")
        })
        .filter(|&at| records[at].get("result") == Some("completed"))
        .ok_or("input: the returned-phase key was not sent")?;
    Ok((
        format!(
            "sophia_qemu_output_unplug_verdict schema=1 status=returned_input_routed device={device} probe={target} keycode={keycode}"
        ),
        observed.max(routed).max(sent),
    ))
}

/// The return's publication with every head and its settlement, after the first return attempt.
fn return_publication(
    records: &[Record],
    first_on: usize,
    heads: u32,
) -> Result<(usize, usize), String> {
    let outputs = heads.to_string();
    let published = (first_on..records.len())
        .find(|&at| {
            records[at].is(super::TOPOLOGY, "published")
                && records[at].get("outputs") == Some(outputs.as_str())
        })
        .ok_or_else(|| format!("return: never published {heads} outputs after the return"))?;
    let transition = records[published]
        .get("transition")
        .ok_or("return: publication names no transition")?;
    let settled = (published..records.len())
        .find(|&at| {
            records[at].is(super::TOPOLOGY, "settled")
                && records[at].get("transition") == Some(transition)
        })
        .ok_or("return: the return publication never settled")?;
    Ok((published, settled))
}

/// The two-probe proof for a return mode. Returns its summary lines and the
/// index of its last link.
pub(super) fn verify(
    records: &[Record],
    plain: &[String],
    mode: super::Mode,
    heads: u32,
    first_off: usize,
    first_on: Option<usize>,
    last_on: Option<usize>,
) -> Result<(Vec<String>, usize), String> {
    let barrier = records
        .iter()
        .position(|r| r.is("sophia_qemu_unplug", "static_barrier"))
        .filter(|&at| at < first_off)
        .ok_or("probes: no static barrier before the removal")?;
    let probes = probes(records, first_off)?;
    static_clients(records, barrier)?;
    let owners = owners(records)?;
    let mut lines = vec![format!(
        "sophia_qemu_output_unplug_verdict schema=1 status=probe_baselines probes={}",
        baselines(records, &probes, &owners, first_off)?
    )];
    let (owner, anchor) = if mode == super::Mode::InputReturn {
        // The keyboard-only run keeps its one owner; its routing chain reads
        // the focused probe's lines, and a key at the other probe refuses.
        if owners.len() != 1 {
            return Err(format!(
                "input return: the outputs were to stay, but {} owners opened",
                owners.len()
            ));
        }
        let first_key = (0..records.len())
            .find(|&at| records[at].is("sophia_qemu_unplug", "key_sending"))
            .ok_or("input return: no key sent")?;
        let target = focused(plain, first_key)?;
        let mut rewritten = Vec::with_capacity(plain.len());
        for line in plain {
            rewritten.push(match client_line(line) {
                Some((probe, rest)) if probe == target => format!("dri3_layout {rest}"),
                Some((probe, rest)) if rest.starts_with("stage=key ") => {
                    return Err(format!(
                        "input return: probe {probe} received a key; probe {target} holds the focus"
                    ));
                }
                Some(_) => String::new(),
                None => line.clone(),
            });
        }
        let (line, routed) = super::input_return::verify_input_return(records, &rewritten)?;
        lines.push(line);
        (&owners[0], routed)
    } else {
        let first_on = first_on.ok_or("return: no return attempted")?;
        let (published, settled) = return_publication(records, first_on, heads)?;
        let owner = owner_at(&owners, published)?;
        if !owner.holds(settled) {
            return Err(
                "return: the return settled outside the owner that published it".to_owned(),
            );
        }
        if owner.opened < first_off {
            return Err("return: the publication belongs to the pre-loss owner".to_owned());
        }
        let anchor = if mode == super::Mode::LockedReturn {
            let (line, unlocked) = lock_cover(
                records, plain, barrier, first_off, published, settled, heads,
            )?;
            lines.push(line);
            unlocked
        } else {
            settled
        };
        (owner, anchor)
    };
    let painted = repaint(records, anchor)?;
    lines.push(format!(
        "sophia_qemu_output_unplug_verdict schema=1 status=returned_heads_presented owner={} probes={}",
        owner.epoch,
        returned_heads(records, &probes, owner, painted)?
    ));
    let mut last = painted.0;
    if mode != super::Mode::InputReturn {
        let (line, at) = returned_key(records, plain, painted.0, "56")?;
        if mode == super::Mode::CombinedReturn {
            let returned = keyboard_cycle(
                records,
                first_off,
                last_on.ok_or("return: no return attempted")?,
            )?;
            if !line.contains(&format!(" device={returned} ")) {
                return Err(format!(
                    "combined: the returned key did not come from the returned keyboard {returned}"
                ));
            }
        }
        lines.push(line);
        last = at;
    }
    Ok((lines, last))
}

fn one_unplug(records: &[Record], status: &str, key: &str, value: &str) -> Result<usize, String> {
    let found = (0..records.len())
        .filter(|&at| {
            records[at].is("sophia_qemu_unplug", status) && records[at].get(key) == Some(value)
        })
        .collect::<Vec<_>>();
    match found.as_slice() {
        [at] => Ok(*at),
        other => Err(format!(
            "expected exactly one {status} {key}={value}, found {}",
            other.len()
        )),
    }
}

/// The combined run's keyboard: removed with the heads and returned after them.
/// Session removes the admitted virtual keyboard K0 after the host's removal and
/// admits a new virtual keyboard K1 after its return. Returns K1.
pub(super) fn keyboard_cycle(
    records: &[Record],
    first_off: usize,
    last_on: usize,
) -> Result<String, String> {
    let off = one_unplug(records, "keyboard_sending", "action", "off")
        .map_err(|e| format!("combined: {e}"))?;
    let off_sent = one_unplug(records, "keyboard_sent", "action", "off")
        .map_err(|e| format!("combined: {e}"))?;
    let on = one_unplug(records, "keyboard_sending", "action", "on")
        .map_err(|e| format!("combined: {e}"))?;
    let on_sent = one_unplug(records, "keyboard_sent", "action", "on")
        .map_err(|e| format!("combined: {e}"))?;
    if !(off < off_sent && off_sent < first_off && last_on < on && on < on_sent) {
        return Err(
            "combined: the keyboard did not leave before the heads and return after them"
                .to_owned(),
        );
    }
    let device = |at: usize| records[at].get("device").unwrap_or("").to_owned();
    let virtual_keyboard =
        |r: &Record| r.get("keyboard") == Some("true") && r.get("virtual") == Some("true");
    let k0 = (0..off)
        .rev()
        .find(|&at| {
            records[at].is("sophia_live_session_input_device", "added")
                && virtual_keyboard(&records[at])
        })
        .map(device)
        .ok_or("combined: no virtual keyboard admitted before the removal")?;
    (off..on)
        .find(|&at| {
            records[at].is("sophia_live_session_input_device", "removed")
                && records[at].get("device") == Some(k0.as_str())
        })
        .ok_or_else(|| format!("combined: keyboard {k0} was not removed"))?;
    let added = (on..records.len())
        .filter(|&at| {
            records[at].is("sophia_live_session_input_device", "added")
                && virtual_keyboard(&records[at])
        })
        .collect::<Vec<_>>();
    let [added] = added.as_slice() else {
        return Err(format!(
            "combined: expected one returned virtual keyboard, found {}",
            added.len()
        ));
    };
    let k1 = device(*added);
    if k1 == k0 {
        return Err("combined: the returned keyboard kept the removed identity".to_owned());
    }
    Ok(k1)
}

/// The locked run's cover and exclusion. The session locks once after the
/// barrier and before the removal; after the return settles, Session's covered
/// record names that lock and the return's published topology epoch with every
/// head; then the host sends one locked-phase key, and Session reports that
/// device held by that lock for the first time in that interval, while neither
/// probe reports a key until the unlock; then the host's right secret and
/// Session's unlock. Returns the summary line and the unlock's index.
pub(super) fn lock_cover(
    records: &[Record],
    plain: &[String],
    barrier: usize,
    first_off: usize,
    published: usize,
    settled: usize,
    heads: u32,
) -> Result<(String, usize), String> {
    let lock = "sophia_live_session_lock";
    let locked = (0..records.len())
        .filter(|&at| records[at].is(lock, "locked"))
        .collect::<Vec<_>>();
    let [locked] = locked.as_slice() else {
        return Err(format!("locked: expected one lock, found {}", locked.len()));
    };
    if !(barrier < *locked && *locked < first_off) {
        return Err(
            "locked: the session did not lock after the barrier and before the removal".to_owned(),
        );
    }
    let epoch = records[*locked]
        .get("epoch")
        .ok_or("locked: lock names no epoch")?;
    let unlocked = (settled..records.len())
        .find(|&at| records[at].is(lock, "unlocked") && records[at].get("epoch") == Some(epoch))
        .ok_or("locked: the lock was not unlocked after the return")?;
    let topology = records[published]
        .get("topology_epoch")
        .ok_or("locked: the return publication names no topology epoch")?;
    let all = heads.to_string();
    (settled..unlocked)
        .find(|&at| {
            let r = &records[at];
            r.is(lock, "covered") && r.get("epoch") == Some(epoch) && r.get("topology_epoch") == Some(topology) && r.get("heads") == Some(all.as_str())
        })
        .ok_or_else(|| format!("locked: no cover for lock {epoch} over the returned topology {topology} with {heads} heads before the unlock"))?;
    let covered = (settled..unlocked)
        .find(|&at| {
            records[at].is(lock, "covered") && records[at].get("topology_epoch") == Some(topology)
        })
        .unwrap_or(settled);
    let sending = one_unplug(records, "key_sending", "phase", "locked")
        .map_err(|e| format!("locked: {e}"))?;
    if !(covered < sending && sending < unlocked) {
        return Err(
            "locked: the locked-phase key was not sent between the cover and the unlock".to_owned(),
        );
    }
    let held = (sending..unlocked)
        .find(|&at| records[at].is(lock, "key_held") && records[at].get("epoch") == Some(epoch))
        .ok_or("locked: the lock held no key after the locked-phase send")?;
    let device = records[held].get("device").unwrap_or("");
    if (0..sending).any(|at| {
        records[at].is(lock, "key_held")
            && records[at].get("epoch") == Some(epoch)
            && records[at].get("device") == Some(device)
    }) {
        return Err(format!(
            "locked: device {device} was already held by lock {epoch} before the locked-phase send"
        ));
    }
    if let Some(line) = plain[*locked..unlocked]
        .iter()
        .find(|line| client_line(line).is_some_and(|(_, rest)| rest.starts_with("stage=key ")))
    {
        return Err(format!(
            "locked: a probe received a key while locked: {line}"
        ));
    }
    let sent =
        one_unplug(records, "key_sent", "phase", "locked").map_err(|e| format!("locked: {e}"))?;
    if !(sending < sent && sent < unlocked) || records[sent].get("result") != Some("completed") {
        return Err("locked: the locked-phase key was not sent before the unlock".to_owned());
    }
    (sent..unlocked)
        .find(|&at| {
            records[at].is("sophia_qemu_lock_input", "sent")
                && records[at].get("secret") == Some("right")
        })
        .ok_or("locked: no right secret sent before the unlock")?;
    Ok((
        format!(
            "sophia_qemu_output_unplug_verdict schema=1 status=lock_covered epoch={epoch} topology_epoch={topology} heads={heads} held_device={device}"
        ),
        unlocked,
    ))
}
