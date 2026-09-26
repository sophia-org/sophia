//! Wire-neutral typed model for the shell v1 handshake.
pub const SOPHIA_SHELL_INTERFACE_REVISION: u16 = 1;
pub const SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER: u64 = 1 << 0;
pub const SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION: u64 = 1 << 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellV1ClientHello {
    pub minimum_revision: u16,
    pub maximum_revision: u16,
    pub required_capabilities: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShellV1ServerWelcome {
    pub selected_revision: u16,
    pub connection_epoch: u64,
    pub capabilities: u64,
    pub max_descriptors: u16,
    pub max_label_bytes: u16,
    pub max_pending_activations: u16,
}
