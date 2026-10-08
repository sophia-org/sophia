use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TopologyEventSource {
    Kernel,
    Processed,
}

pub(super) fn processed_membership_event(action: udev::EventType, name: &OsStr) -> bool {
    is_device_node(name)
        && matches!(
            action,
            udev::EventType::Add
                | udev::EventType::Bind
                | udev::EventType::Change
                | udev::EventType::Remove
                | udev::EventType::Unbind
        )
}

fn is_device_node(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    ["card", "renderD"].into_iter().any(|prefix| {
        name.strip_prefix(prefix).is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
    })
}

pub(super) fn topology_event_requires_rescan(
    source: TopologyEventSource,
    action: udev::EventType,
    name: &OsStr,
    hotplug: bool,
) -> bool {
    match source {
        TopologyEventSource::Kernel => {
            (action == udev::EventType::Change && hotplug)
                || (is_device_node(name)
                    && matches!(action, udev::EventType::Remove | udev::EventType::Unbind))
        }
        TopologyEventSource::Processed => {
            // The processed monitor is a settled view of the same kernel
            // topology.  Lifecycle notifications are not topology changes:
            // startup and database replay commonly produce Add/Bind bursts,
            // and treating those as rescans can defer native recovery before
            // the first frame.  Only a settled hotplug Change is authoritative
            // for a connector/output rebuild.
            is_device_node(name) && action == udev::EventType::Change && hotplug
        }
    }
}
