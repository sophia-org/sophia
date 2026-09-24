#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XResolvedKeyboardInput {
    client: XServerFrontendClientId,
    target_window: Option<XResourceId>,
    event: XAuthorityInputEvent,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XKeyboardRouteResolution {
    Routed(XResolvedKeyboardInput),
    Rejected,
}

#[cfg(unix)]
fn resolve_x_keyboard_input(
    route: &XAuthorityRoutedInput,
    surface_route: XServerFrontendSurfaceRoute,
    input_authority: &Arc<Mutex<crate::XInputAuthorityState>>,
    xkb_worker: &XkbKeyboardWorker,
    pointer_state: u16,
    time_msec: u32,
) -> Result<XKeyboardRouteResolution, XServerFrontendRouteError> {
    let InputEventKind::Key { keycode, pressed } = route.request.kind else {
        return Ok(XKeyboardRouteResolution::Rejected);
    };
    let repeated = route.mode == XAuthorityRoutedInputMode::Repeat;
    if repeated && !pressed {
        return Ok(XKeyboardRouteResolution::Rejected);
    }

    let mut client = surface_route.client;
    let mut target_window = Some(surface_route.window);
    if let Some(grab) = input_authority
        .lock()
        .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
        .keyboard_grab(surface_route.namespace)
    {
        client = XServerFrontendClientId(grab.owner);
        target_window = Some(if grab.owner_events && client == surface_route.client {
            surface_route.window
        } else {
            grab.window
        });
    }

    let mapped = if repeated {
        keycode
            .checked_add(8)
            .and_then(|keycode| u8::try_from(keycode).ok())
            .zip(
                xkb_worker
                    .request(XkbWorkerCommand::Modifiers {
                        seat: route.request.seat,
                    })?
                    .map(|(_, state, modifiers_after)| (state, modifiers_after)),
            )
            .map(|(keycode, (state, modifiers_after))| (keycode, state, modifiers_after))
    } else {
        xkb_worker.request(XkbWorkerCommand::Key {
            seat: route.request.seat,
            keycode,
            pressed,
        })?
    };
    let Some((keycode, state, modifiers_after)) = mapped else {
        return Ok(XKeyboardRouteResolution::Rejected);
    };
    // What QueryKeymap reports: the keys down, as this routing saw them go
    // down and up (t166). A repeat is not a transition.
    if !repeated {
        input_authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .observe_pressed_key(surface_route.namespace, keycode, pressed);
    }

    let passive = if pressed && !repeated {
        input_authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .activate_key(surface_route.namespace, keycode, state & 0xff)
    } else {
        None
    };
    if let Some(grab) = passive {
        client = XServerFrontendClientId(grab.owner);
        target_window = Some(if grab.owner_events && client == surface_route.client {
            surface_route.window
        } else {
            grab.window
        });
    }
    if !pressed && !repeated {
        input_authority
            .lock()
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
            .release_key(surface_route.namespace, keycode);
    }

    Ok(XKeyboardRouteResolution::Routed(
        XResolvedKeyboardInput {
            client,
            target_window,
            event: XAuthorityInputEvent::Key(XAuthorityKeyEvent {
                keycode,
                pressed,
                state: state | pointer_state,
                modifiers_after: modifiers_after as u8,
                time_msec,
            }),
        },
    ))
}
