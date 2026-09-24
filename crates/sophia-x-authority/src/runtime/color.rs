impl XAuthorityRuntime {
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
        if colormap.local.raw() == u64::from(crate::X_SETUP_DEFAULT_COLORMAP) {
            if namespace.is_valid() {
                return Ok(());
            }
            return Err(crate::XColormapError::Access(
                crate::XAuthorityAccessError::InvalidNamespace,
            ));
        }
        self.colormap_visual(namespace, colormap)?;
        self.resources.remove(colormap);
        self.colormaps.remove(&colormap);
        Ok(())
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
    pub(crate) fn release_window_colormaps(&mut self, colormap: crate::XResourceId) -> Vec<crate::XResourceId> {
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
