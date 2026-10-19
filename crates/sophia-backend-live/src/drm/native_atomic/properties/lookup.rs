use crate::prelude::*;

use super::LibdrmNativePropertyHandleSet;

pub trait LibdrmNativePropertyLookupDevice {
    /// Resolve IDs for a primary-plane request. Ordinary devices discover
    /// afresh; a target-scoped device may retain successful ID discovery.
    /// This does not read or cache current property values.
    fn primary_plane_property_handles(
        &self,
        connector: drm::control::connector::Handle,
        crtc: drm::control::crtc::Handle,
        plane: drm::control::plane::Handle,
    ) -> super::LibdrmNativePrimaryPlanePropertyDiscoveryResult {
        super::discovery::discover_primary_plane_properties_uncached(self, connector, crtc, plane)
    }

    fn connector_property_handles(
        &self,
        connector: drm::control::connector::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet>;

    fn crtc_property_handles(
        &self,
        crtc: drm::control::crtc::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet>;

    fn plane_property_handles(
        &self,
        plane: drm::control::plane::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet>;

    fn connector_property_value(
        &self,
        _connector: drm::control::connector::Handle,
        _property: drm::control::property::Handle,
    ) -> io::Result<Option<u64>> {
        Ok(None)
    }
}

impl<D> LibdrmNativePropertyLookupDevice for D
where
    D: drm::control::Device,
{
    fn connector_property_handles(
        &self,
        connector: drm::control::connector::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        Ok(LibdrmNativePropertyHandleSet::from_property_info_map(
            self.get_properties(connector)?.as_hashmap(self)?,
        ))
    }

    fn crtc_property_handles(
        &self,
        crtc: drm::control::crtc::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        Ok(LibdrmNativePropertyHandleSet::from_property_info_map(
            self.get_properties(crtc)?.as_hashmap(self)?,
        ))
    }

    fn plane_property_handles(
        &self,
        plane: drm::control::plane::Handle,
    ) -> io::Result<LibdrmNativePropertyHandleSet> {
        Ok(LibdrmNativePropertyHandleSet::from_property_info_map(
            self.get_properties(plane)?.as_hashmap(self)?,
        ))
    }

    fn connector_property_value(
        &self,
        connector: drm::control::connector::Handle,
        property: drm::control::property::Handle,
    ) -> io::Result<Option<u64>> {
        Ok(self
            .get_properties(connector)?
            .iter()
            .find_map(|(candidate, value)| (*candidate == property).then_some(*value)))
    }
}
