use super::*;
use std::cell::Cell;

fn identity() -> LiveRenderDeviceIdentitySnapshot {
    LiveRenderDeviceIdentitySnapshot {
        node: "/dev/dri/renderD128".into(),
        device: 1,
        inode: 2,
        device_number: rustix::fs::makedev(226, 128),
        physical_device: "/sys/devices/pci0000:00/0000:03:00.0".into(),
    }
}

#[test]
fn instance_open_refuses_admission_and_identity_changes_without_a_fallback() {
    use LiveRenderDeviceInventoryError as E;
    for refusal in 0..5 {
        let identity = identity();
        let validations = Cell::new(0);
        let opens = Cell::new(0);
        let result = open_revalidated(
            &identity,
            || {
                let call = validations.get();
                validations.set(call + 1);
                if (refusal == 0 && call == 0) || (refusal == 3 && call == 1) {
                    Err(E::IdentityChanged)
                } else {
                    Ok(())
                }
            },
            || {
                if refusal == 1 {
                    return Err(E::DiscoveryUnavailable);
                }
                let mut observed = identity.clone();
                if refusal == 2 {
                    observed.inode += 1;
                }
                Ok(observed)
            },
            || {
                opens.set(opens.get() + 1);
                let mut observed = identity.clone();
                if refusal == 4 {
                    observed.device_number += 1;
                }
                Ok(LiveRenderDevice {
                    file: File::open("/dev/null").unwrap(),
                    identity: observed,
                })
            },
        );
        assert!(result.is_err(), "refusal {refusal}");
        assert_eq!(opens.get(), usize::from(refusal >= 3), "refusal {refusal}");
    }
}

#[test]
fn each_instance_uses_the_validated_render_node_and_error_remains_typed() {
    fn typed_error<E: std::error::Error + Send + Sync + 'static>() {}
    typed_error::<LiveRenderDeviceInventoryError>();
    let identity = identity();
    let opens = Cell::new(0);
    for _ in 0..2 {
        let result = open_revalidated(
            &identity,
            || Ok(()),
            || Ok(identity.clone()),
            || {
                opens.set(opens.get() + 1);
                Ok(LiveRenderDevice {
                    file: File::open("/dev/null").unwrap(),
                    identity: identity.clone(),
                })
            },
        )
        .unwrap();
        assert!(result.identity.is_render_node());
        assert_eq!(result.identity.device_number, identity.device_number);
    }
    assert_eq!(opens.get(), 2);
    let mut primary = identity;
    primary.node = "/dev/dri/card0".into();
    assert!(!primary.is_render_node());
}
