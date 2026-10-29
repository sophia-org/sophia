use super::*;

#[test]
fn private_capture_argv_cannot_fall_back_to_inherited_output() {
    let mut f = Fixture::new();
    let mut request = f.request();
    request.launch.args.push("--diagnostic-stream".into());
    assert!(
        f.service
            .start(
                request,
                &mut f.auth.coordinator,
                f.wake.notifier(),
                Instant::now()
            )
            .is_err()
    );
    assert_eq!(f.service.occupied(), 0);
}

#[cfg(feature = "diagnostic-capture-delivery")]
#[test]
fn private_capture_selection_checks_every_owner_supplied_input() {
    use crate::diagnostics::{
        application::{LaunchContext, LaunchSource},
        pixel_delivery::CaptureStore,
    };
    let mut f = Fixture::new();
    f.service = NamespaceLaunchService::new(f.service.registry.clone(), f.root.clone())
        .with_capture_delivery(CaptureStore::create(&f.root.join("captures")).unwrap());
    for mode in 0..13 {
        let mut request = f.request();
        request.launch.program = std::env::current_exe().unwrap().into();
        request.launch.args = vec!["capture".into(), "7".into(), "--diagnostic-stream".into()];
        request.capture_context = Some(LaunchContext {
            source: LaunchSource::Developer,
            transaction: Some(8),
        });
        let mut policy = request.recipe.policy().clone();
        policy.capture_outputs = vec!["device/connector-A".into()];
        match mode {
            0 => request.capture_context = None,
            1 => request.capture_context.as_mut().unwrap().source = LaunchSource::Startup,
            2 => request.capture_context.as_mut().unwrap().transaction = None,
            3 => request.launch.program = "/usr/bin/true".into(),
            4 => request.launch.protection_domain = None,
            5 => request.launch.args[1] = "07".into(),
            6 => request.launch.args[1] = "0".into(),
            7 => request.launch.args.push("extra".into()),
            8 => policy.capture_outputs.clear(),
            9 => policy.capture_outputs.push("connector-B".into()),
            10 => policy.developer_launch = false,
            11 => policy.profile = NamespaceRecipeProfile::SharedX,
            12 => policy.capture_outputs = vec!["connector-A".into()],
            _ => unreachable!(),
        }
        request.recipe = LoadedNamespaceRecipe::load(&policy, &f.root)
            .unwrap()
            .into();
        assert!(
            f.service
                .prepare_capture(&request, 1, f.wake.notifier())
                .is_err(),
            "mode {mode}"
        );
    }
}
