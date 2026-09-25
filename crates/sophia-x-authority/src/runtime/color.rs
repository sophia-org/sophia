impl XAuthorityRuntime {
    pub(crate) fn installed_colormap(&self, namespace: NamespaceId) -> crate::XResourceId {
        self.installed_colormaps
            .get(&namespace)
            .copied()
            .unwrap_or(crate::XResourceId::new(
                u64::from(crate::X_SETUP_DEFAULT_COLORMAP),
                1,
            ))
    }

    pub(crate) fn colormap_installed(&self, namespace: NamespaceId, colormap: u32) -> bool {
        self.installed_colormap(namespace).local.raw() == u64::from(colormap)
    }

    /// The advertised screen has exactly one installed map. Installation is
    /// namespace-local even though the default map's protocol ID is shared.
    pub(crate) fn install_colormap(
        &mut self,
        namespace: NamespaceId,
        colormap: crate::XResourceId,
    ) -> Vec<XColormapChange> {
        let previous = self.installed_colormap(namespace);
        if previous == colormap {
            return Vec::new();
        }
        if colormap.local.raw() == u64::from(crate::X_SETUP_DEFAULT_COLORMAP) {
            self.installed_colormaps.remove(&namespace);
        } else {
            self.installed_colormaps.insert(namespace, colormap);
        }
        // Loss precedes gain, including on unmapped windows and the root.
        let mut windows = self.windows.ids_for_namespace(namespace);
        let root = crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_ROOT), 1);
        if !windows.contains(&root) {
            windows.insert(0, root);
        }
        let mut changes = Vec::new();
        for (map, installed) in [(previous, false), (colormap, true)] {
            for &window in &windows {
                if !self.window_is_input_only(window) && self.window_visual(window).2 == map {
                    changes.push(XColormapChange {
                        window,
                        colormap: map.local.raw() as u32,
                        new: false,
                        installed,
                    });
                }
            }
        }
        changes
    }

    pub(crate) fn uninstall_colormap(
        &mut self,
        namespace: NamespaceId,
        colormap: crate::XResourceId,
    ) -> Vec<XColormapChange> {
        if self.installed_colormap(namespace) != colormap {
            return Vec::new();
        }
        self.install_colormap(
            namespace,
            crate::XResourceId::new(u64::from(crate::X_SETUP_DEFAULT_COLORMAP), 1),
        )
    }

    /// TrueColor cells are immutable, but each client owns a reference to
    /// each RGB component it allocates, including repeated allocations.
    pub(crate) fn allocate_color(
        &mut self,
        namespace: NamespaceId,
        client: u64,
        colormap: crate::XResourceId,
        pixel: u32,
    ) {
        let channels = self
            .color_allocations
            .entry((namespace, client, colormap))
            .or_default();
        for (channel, shift) in channels.iter_mut().zip([16, 8, 0]) {
            *channel.entry((pixel >> shift) as u8).or_default() += 1;
        }
    }

    pub(crate) fn copy_color_allocations(
        &mut self,
        namespace: NamespaceId,
        client: u64,
        source: crate::XResourceId,
        target: crate::XResourceId,
    ) {
        if let Some(channels) = self.color_allocations.remove(&(namespace, client, source)) {
            self.color_allocations
                .insert((namespace, client, target), channels);
        }
    }

    pub(crate) fn release_client_colors(&mut self, namespace: NamespaceId, client: u64) {
        self.color_allocations
            .retain(|(owner, id, _), _| *owner != namespace || *id != client);
    }

    /// Process every requested component even after an error, as FreeColors
    /// frees the valid entries in a partially invalid request. Enumerating
    /// masks per channel bounds the work to 256 combinations per pixel.
    pub(crate) fn free_colors(
        &mut self,
        namespace: NamespaceId,
        client: u64,
        colormap: crate::XResourceId,
        mask: u32,
        pixels: &[u32],
    ) -> Option<(crate::XErrorCode, u32)> {
        let visual = self
            .colormap_visual(namespace, colormap)
            .ok()
            .and_then(crate::x_true_color_visual);
        let Some(visual) = visual else {
            return Some((
                crate::XErrorCode::BadColor,
                u32::try_from(colormap.local.raw()).unwrap_or(0),
            ));
        };
        let valid_mask = visual.valid_pixel_mask();
        let mut error = None;
        let key = (namespace, client, colormap);
        let mut channels = self.color_allocations.remove(&key).unwrap_or_default();
        for (channel, shift) in channels.iter_mut().zip([16, 8, 0]) {
            let channel_mask = (mask >> shift) as u8;
            for bits in 0..=u8::MAX {
                if bits & !channel_mask != 0 {
                    continue;
                }
                for &pixel in pixels {
                    // ARGB accepts alpha bits but never allocates an alpha
                    // channel: dix's RGBMASK includes ALPHAMASK.
                    if pixel & !valid_mask != 0 {
                        error = Some((
                            crate::XErrorCode::BadValue,
                            pixel | (u32::from(bits) << shift),
                        ));
                        continue;
                    }
                    let component = ((pixel >> shift) as u8) | bits;
                    match channel.get_mut(&component) {
                        Some(count) => {
                            *count -= 1;
                            if *count == 0 {
                                channel.remove(&component);
                            }
                        }
                        None => error = Some((crate::XErrorCode::BadAccess, 0)),
                    }
                }
            }
        }
        if channels.iter().any(|channel| !channel.is_empty()) {
            self.color_allocations.insert(key, channels);
        }
        if mask & !valid_mask != 0
            && let Some(pixel) = pixels.first()
        {
            error = Some((crate::XErrorCode::BadValue, pixel | mask));
        }
        error
    }

    pub fn create_colormap(
        &mut self,
        namespace: NamespaceId,
        colormap: crate::XResourceId,
        visual: u32,
        generation: u64,
    ) -> Result<(), crate::XColormapError> {
        if crate::x_true_color_visual(visual).is_none() {
            return Err(crate::XColormapError::UnknownVisual);
        }
        if self.resources.get(colormap).is_some() {
            return Err(crate::XColormapError::DuplicateId);
        }
        self.resources
            .insert(colormap, XResourceKind::Colormap, namespace, generation)
            .map_err(crate::XColormapError::Access)?;
        self.colormaps.insert(colormap, visual);
        Ok(())
    }

    pub fn colormap_visual(
        &self,
        namespace: NamespaceId,
        colormap: crate::XResourceId,
    ) -> Result<u32, crate::XColormapError> {
        if !namespace.is_valid() {
            return Err(crate::XColormapError::Access(
                crate::XAuthorityAccessError::InvalidNamespace,
            ));
        }
        if colormap.local.raw() == u64::from(crate::X_SETUP_DEFAULT_COLORMAP) {
            return Ok(crate::X_SETUP_DEFAULT_VISUAL);
        }
        self.resources
            .lookup(namespace, colormap, XResourceKind::Colormap)
            .map_err(crate::XColormapError::Access)?;
        self.colormaps
            .get(&colormap)
            .copied()
            .ok_or(crate::XColormapError::Access(
                crate::XAuthorityAccessError::UnknownResource,
            ))
    }

    pub fn free_colormap(
        &mut self,
        namespace: NamespaceId,
        colormap: crate::XResourceId,
    ) -> Result<(), crate::XColormapError> {
        self.free_colormap_with_changes(namespace, colormap)
            .map(|_| ())
    }

    pub(crate) fn free_colormap_with_changes(
        &mut self,
        namespace: NamespaceId,
        colormap: crate::XResourceId,
    ) -> Result<Vec<XColormapChange>, crate::XColormapError> {
        if colormap.local.raw() == u64::from(crate::X_SETUP_DEFAULT_COLORMAP) {
            if namespace.is_valid() {
                return Ok(Vec::new());
            }
            return Err(crate::XColormapError::Access(
                crate::XAuthorityAccessError::InvalidNamespace,
            ));
        }
        self.colormap_visual(namespace, colormap)?;
        let mut changes = self.uninstall_colormap(namespace, colormap);
        changes.extend(
            self.release_window_colormaps(colormap)
                .into_iter()
                .map(|window| XColormapChange {
                    window,
                    colormap: 0,
                    new: true,
                    installed: false,
                }),
        );
        self.resources.remove(colormap);
        self.colormaps.remove(&colormap);
        self.color_allocations
            .retain(|(_, _, map), _| *map != colormap);
        Ok(changes)
    }

    /// Set a window's colormap attribute from ChangeWindowAttributes.
    ///
    /// Zero is CopyFromParent, the parent's colormap. A colormap that does
    /// not exist is a Color error and one of another visual than the window's
    /// a Match error. The colormap is returned when the attribute changed, so
    /// the caller can tell the window's ColormapChange selectors; setting the
    /// one it already has changes nothing (t210).
    pub(crate) fn change_window_colormap(
        &mut self,
        namespace: NamespaceId,
        window: crate::XResourceId,
        raw: u32,
    ) -> Result<Option<u32>, crate::XWindowColormapError> {
        let (depth, visual, current) = self.window_visual(window);
        let colormap = if raw == 0 {
            let parent = self
                .windows
                .get(window)
                .map(|record| record.parent)
                .ok_or(crate::XWindowColormapError::Match)?;
            let (_, parent_visual, parent_colormap) = self.window_visual(parent);
            if parent_visual != visual {
                return Err(crate::XWindowColormapError::Match);
            }
            parent_colormap
        } else {
            let colormap = crate::XResourceId::new(u64::from(raw), 1);
            match self.colormap_visual(namespace, colormap) {
                Err(_) => return Err(crate::XWindowColormapError::Color),
                Ok(colormap_visual) if colormap_visual != visual => {
                    return Err(crate::XWindowColormapError::Match);
                }
                Ok(_) => colormap,
            }
        };
        if colormap == current {
            return Ok(None);
        }
        self.set_window_visual(window, depth, visual, colormap);
        Ok(Some(u32::try_from(colormap.local.raw()).unwrap_or(0)))
    }

    /// Leave every window naming a freed colormap with None, and say which,
    /// so the caller can tell their ColormapChange selectors.
    pub(crate) fn release_window_colormaps(
        &mut self,
        colormap: crate::XResourceId,
    ) -> Vec<crate::XResourceId> {
        let none = crate::XResourceId::new(0, 1);
        let mut released = Vec::new();
        for (window, (_, _, named)) in &mut self.window_visuals {
            if *named == colormap {
                *named = none;
                released.push(*window);
            }
        }
        released
    }
}
