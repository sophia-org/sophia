use xkbcommon::xkb;

/// Engine-owned text composition. Only committed text while the overlay owns
/// input is exposed to its shell; application frontend state is independent.
pub struct LauncherKeyboard {
    state: xkb::State,
    compose: Option<xkb::compose::State>,
}
impl LauncherKeyboard {
    pub fn new(
        rules: &str,
        model: &str,
        layout: &str,
        variant: &str,
        options: &str,
        locale: &std::ffi::OsStr,
    ) -> Result<Self, &'static str> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            rules,
            model,
            layout,
            variant,
            Some(options.to_owned()),
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or("launcher keymap unavailable")?;
        let compose =
            xkb::compose::Table::new_from_locale(&context, locale, xkb::compose::COMPILE_NO_FLAGS)
                .ok()
                .map(|table| xkb::compose::State::new(&table, xkb::compose::STATE_NO_FLAGS));
        Ok(Self {
            state: xkb::State::new(&keymap),
            compose,
        })
    }
    /// Keep compose state within one opening, including candidate/focus refresh.
    /// Grant/output/opening replacement must not commit a previous dead key.
    pub fn synchronize_native_focus(
        &mut self,
        capture: &mut super::LauncherCapture,
        binding: Option<sophia_protocol::NativeLauncherBinding>,
    ) {
        let owner = |b: sophia_protocol::NativeLauncherBinding| (b.grant, b.output, b.opening);
        if capture.native_binding().map(owner) != binding.map(owner)
            && let Some(compose) = self.compose.as_mut()
        {
            compose.reset();
        }
        capture.present_native(binding);
    }

    pub fn command_modifier_active(&self) -> bool {
        [xkb::MOD_NAME_CTRL, xkb::MOD_NAME_ALT, xkb::MOD_NAME_LOGO]
            .iter()
            .any(|name| {
                self.state
                    .mod_name_is_active(name, xkb::STATE_MODS_EFFECTIVE)
            })
    }
    pub fn observe(&mut self, keycode: u32, pressed: bool, active: bool) -> (Option<String>, bool) {
        let key = xkb::Keycode::new(keycode.saturating_add(8));
        self.state.update_key(
            key,
            if pressed {
                xkb::KeyDirection::Down
            } else {
                xkb::KeyDirection::Up
            },
        );
        if !active || !pressed {
            if !active && let Some(compose) = self.compose.as_mut() {
                compose.reset();
            }
            return (None, false);
        }
        let control = self
            .state
            .mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
        let alt = self
            .state
            .mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE);
        let logo = self
            .state
            .mod_name_is_active(xkb::MOD_NAME_LOGO, xkb::STATE_MODS_EFFECTIVE);
        if keycode == 1 {
            if let Some(compose) = self.compose.as_mut() {
                compose.reset();
            }
            return (None, false);
        }
        if control || alt || logo {
            return (None, control && !alt && !logo && keycode == 22);
        }
        if let Some(compose) = self.compose.as_mut() {
            compose.feed(self.state.key_get_one_sym(key));
            match compose.status() {
                xkb::compose::Status::Composing => return (None, false),
                xkb::compose::Status::Composed => {
                    let text = compose.utf8();
                    compose.reset();
                    return (text, false);
                }
                xkb::compose::Status::Cancelled => {
                    compose.reset();
                    return (None, false);
                }
                xkb::compose::Status::Nothing => {}
            }
        }
        let text = self.state.key_get_utf8(key);
        ((!text.is_empty()).then_some(text), false)
    }
}
