use super::*;
use crate::diagnostics::{
    application::LaunchSource,
    pixel_delivery::{CaptureLaunch, CaptureStore},
};
use std::{io, os::fd::OwnedFd, path::Path};

impl NamespaceLaunchService {
    pub(crate) fn with_capture_delivery(mut self, store: Arc<CaptureStore>) -> Self {
        self.capture_delivery = Some(store);
        self
    }
    pub(super) fn prepare_capture(
        &self,
        request: &ClientLaunchRequest,
        generation: u64,
        wake: sophia_wake::Notifier,
    ) -> io::Result<(CaptureLaunch, OwnedFd, OwnedFd)> {
        let refuse =
            || io::Error::other("diagnostic stream requires the exact admitted observer launch");
        let store = self.capture_delivery.as_ref().ok_or_else(refuse)?;
        let context = request.capture_context.ok_or_else(refuse)?;
        let policy = request.recipe.policy();
        let args = &request.launch.args;
        if !matches!(context.source, LaunchSource::Developer)
            || policy.profile != NamespaceRecipeProfile::Observer
            || !policy.developer_launch
            || policy.capture_outputs.len() != 1
            || !policy.capture_outputs[0].contains('/')
            || !store.matches_executable(Path::new(&request.launch.program))
            || request.launch.protection_domain.is_none()
            || request.x11_target.is_some()
            || args.len() != 3
            || args[0] != "capture"
            || args[2] != "--diagnostic-stream"
        {
            return Err(refuse());
        }
        let text = args[1].to_str().ok_or_else(refuse)?;
        let output: u64 = text.parse().map_err(|_| refuse())?;
        if output == 0 || output.to_string() != text {
            return Err(refuse());
        }
        store.prepare(
            generation,
            context.transaction.filter(|t| *t > 0).ok_or_else(refuse)?,
            request.namespace,
            sophia_protocol::OutputId::from_raw(output),
            &policy.capture_outputs[0],
            wake,
        )
    }
}
