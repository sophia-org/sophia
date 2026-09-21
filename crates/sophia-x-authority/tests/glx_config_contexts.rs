use sophia_protocol::{NamespaceId, TransactionId};
use sophia_x_authority::*;

struct Client {
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
}

impl Client {
    fn new(pixmap_textures: bool) -> Self {
        let mut runtime = XAuthorityRuntime::new();
        runtime.set_pixmap_textures_supported(pixmap_textures);
        Self {
            runtime,
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            sequence: 0,
        }
    }

    fn send(&mut self, request: XWireRequest) -> Vec<XClientOutput> {
        self.send_with_opcode(X_GLX_MAJOR_OPCODE, request)
    }

    fn send_with_opcode(&mut self, major_opcode: u8, request: XWireRequest) -> Vec<XClientOutput> {
        self.sequence += 1;
        dispatch_x11_wire_request(
            XDispatchContext {
                byte_order: XByteOrder::LittleEndian,
                namespace: NamespaceId::from_raw(71),
                transaction: TransactionId::from_raw(u64::from(self.sequence)),
                sequence: self.sequence,
                major_opcode,
                client_id: 1,
                injection: XTestAdmission::Absent,
                server_time: 4_242,
            },
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        )
        .outputs
    }

    fn create_pixmap(&mut self, pixmap: XResourceId, depth: u8) {
        let outputs = self.send_with_opcode(
            53,
            XWireRequest::CreatePixmap {
                depth,
                pixmap,
                drawable: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
                width: 3,
                height: 2,
            },
        );
        assert!(
            outputs.is_empty(),
            "core pixmap creation failed: {outputs:?}"
        );
    }
}

#[test]
fn every_advertised_configuration_can_create_and_bind_a_direct_context() {
    for supported in [false, true] {
        let mut client = Client::new(supported);
        let configurations = match client
            .send(XWireRequest::GlxGetFbConfigs { screen: 0 })
            .remove(0)
        {
            XClientOutput::Reply(XClientReply::GlxFbConfigs { configs, .. }) => configs,
            other => panic!("unexpected configuration reply: {other:?}"),
        };
        for attributes in configurations {
            let config = attributes
                .iter()
                .find(|(attribute, _)| *attribute == X_GLX_FBCONFIG_ID_ATTRIBUTE)
                .unwrap()
                .1;
            let context = XResourceId::new(u64::from(config) * 10, 1);
            let pbuffer = XResourceId::new(u64::from(config) * 10 + 1, 1);
            assert!(
                client
                    .send(XWireRequest::GlxCreateContext {
                        context,
                        config: XGlxContextConfig::FbConfig(config),
                        screen: 0,
                        share: None,
                        direct: true,
                    })
                    .is_empty(),
                "advertised config {config} rejected its context"
            );
            assert!(
                client
                    .send(XWireRequest::GlxCreatePbuffer {
                        screen: 0,
                        fbconfig: config,
                        pbuffer,
                        width: 3,
                        height: 2,
                        largest: false,
                    })
                    .is_empty()
            );
            assert!(matches!(
                client
                    .send(XWireRequest::GlxMakeCurrent {
                        drawable: Some(pbuffer),
                        context: Some(context),
                        old_context_tag: 0,
                    })
                    .as_slice(),
                [XClientOutput::Reply(XClientReply::GlxMakeCurrent {
                    context_tag: 1,
                    ..
                })]
            ));
        }
    }
}

#[test]
fn hidden_stencil_configurations_cannot_be_used_without_the_provider() {
    let mut client = Client::new(false);
    for config in 4..=6 {
        let context = XResourceId::new(u64::from(config) * 10, 1);
        assert!(matches!(
            client
                .send(XWireRequest::GlxCreateContext {
                    context,
                    config: XGlxContextConfig::FbConfig(config),
                    screen: 0,
                    share: None,
                    direct: true,
                })
                .as_slice(),
            [XClientOutput::Error(_)]
        ));
        assert!(
            client
                .runtime
                .glx_context(NamespaceId::from_raw(71), context)
                .is_err()
        );
    }
}

#[test]
fn a_glx_pixmap_cannot_take_an_existing_graphics_context_id() {
    let mut client = Client::new(true);
    let pixmap = XResourceId::new(0x1001, 1);
    let gc = XResourceId::new(0x1002, 1);
    client.create_pixmap(pixmap, 32);
    let values = XGraphicsContextValues {
        foreground: 0x12345678,
        ..Default::default()
    };
    assert!(
        client
            .send_with_opcode(
                55,
                XWireRequest::CreateGraphicsContext {
                    gc,
                    drawable: pixmap,
                    values: values.clone(),
                }
            )
            .is_empty()
    );

    let outputs = client.send(XWireRequest::GlxCreatePixmap {
        screen: 0,
        fbconfig: 2,
        pixmap,
        glx_pixmap: gc,
        target: Some(X_GLX_TEXTURE_2D_VALUE),
        format: Some(X_GLX_TEXTURE_FORMAT_RGBA_VALUE),
        mipmap: Some(false),
    });
    assert!(
        matches!(outputs.as_slice(), [XClientOutput::Error(_)]),
        "a GLX pixmap reused the live GC id: {outputs:?}"
    );
    assert_eq!(
        client
            .runtime
            .graphics_context_values(NamespaceId::from_raw(71), gc)
            .unwrap(),
        values
    );
    assert!(
        client
            .runtime
            .glx_pixmap(NamespaceId::from_raw(71), gc)
            .is_err(),
        "refusing the collision must not create a GLX alias"
    );
}

fn check_make_context_current(pixmap: bool, direct: bool) {
    let mut client = Client::new(true);
    let backing = XResourceId::new(0x2001, 1);
    let drawable = XResourceId::new(0x2002, 1);
    let context = XResourceId::new(0x2003, 1);
    if pixmap {
        client.create_pixmap(backing, 32);
        let outputs = client.send(XWireRequest::GlxCreatePixmap {
            screen: 0,
            fbconfig: 2,
            pixmap: backing,
            glx_pixmap: drawable,
            target: Some(X_GLX_TEXTURE_2D_VALUE),
            format: Some(X_GLX_TEXTURE_FORMAT_RGBA_VALUE),
            mipmap: Some(false),
        });
        assert!(
            outputs.is_empty(),
            "GLX pixmap creation failed: {outputs:?}"
        );
    } else {
        assert!(
            client
                .send(XWireRequest::GlxCreatePbuffer {
                    screen: 0,
                    fbconfig: 2,
                    pbuffer: drawable,
                    width: 3,
                    height: 2,
                    largest: false,
                })
                .is_empty()
        );
    }
    let created = client.send(XWireRequest::GlxCreateContext {
        context,
        config: XGlxContextConfig::FbConfig(2),
        screen: 0,
        share: None,
        direct,
    });
    assert!(created.is_empty(), "context creation failed: {created:?}");
    assert!(
        matches!(client.send(XWireRequest::GlxIsDirect { context }).as_slice(),
        [XClientOutput::Reply(XClientReply::GlxIsDirect { direct: observed, .. })] if *observed == direct)
    );

    let outputs = client.send(XWireRequest::GlxMakeContextCurrent {
        drawable,
        read_drawable: drawable,
        context: Some(context),
    });
    if direct {
        assert!(
            matches!(
                outputs.as_slice(),
                [XClientOutput::Reply(XClientReply::GlxMakeCurrent {
                    context_tag: 1,
                    ..
                })]
            ),
            "direct context was not bound to pixmap={pixmap}: {outputs:?}"
        );
    } else {
        assert!(
            matches!(outputs.as_slice(), [XClientOutput::Error(_)]),
            "indirect rendering was accepted for pixmap={pixmap}: {outputs:?}"
        );
    }
}

#[test]
fn make_context_current_binds_a_direct_pixmap() {
    check_make_context_current(true, true);
}

#[test]
fn make_context_current_binds_a_direct_pbuffer() {
    check_make_context_current(false, true);
}

#[test]
fn make_context_current_refuses_an_indirect_pixmap() {
    check_make_context_current(true, false);
}

#[test]
fn make_context_current_refuses_an_indirect_pbuffer() {
    check_make_context_current(false, false);
}

fn config_attribute(attributes: &[(u32, u32)], name: u32) -> u32 {
    attributes
        .iter()
        .find(|(attribute, _)| *attribute == name)
        .unwrap_or_else(|| panic!("configuration omitted {name:#x}: {attributes:?}"))
        .1
}

#[test]
fn advertised_rgb_and_rgba_bindings_agree_with_pixmap_creation() {
    let mut client = Client::new(true);
    let configurations = match client
        .send(XWireRequest::GlxGetFbConfigs { screen: 0 })
        .remove(0)
    {
        XClientOutput::Reply(XClientReply::GlxFbConfigs { configs, .. }) => configs,
        other => panic!("unexpected configuration reply: {other:?}"),
    };
    assert!(!configurations.is_empty());
    let mut opaque_rgba_cases = 0;
    for attributes in configurations {
        let config = config_attribute(&attributes, X_GLX_FBCONFIG_ID_ATTRIBUTE);
        let visual = config_attribute(&attributes, X_GLX_VISUAL_ID_ATTRIBUTE);
        let depth = if visual == X_SETUP_DEFAULT_VISUAL {
            24
        } else {
            32
        };
        for (index, capability, format) in [
            (
                0,
                X_GLX_BIND_TO_TEXTURE_RGB_ATTRIBUTE,
                X_GLX_TEXTURE_FORMAT_RGB_VALUE,
            ),
            (
                1,
                X_GLX_BIND_TO_TEXTURE_RGBA_ATTRIBUTE,
                X_GLX_TEXTURE_FORMAT_RGBA_VALUE,
            ),
        ] {
            let advertised = config_attribute(&attributes, capability);
            assert!(advertised <= 1, "binding capability must be boolean");
            if depth == 24 && format == X_GLX_TEXTURE_FORMAT_RGBA_VALUE && advertised == 1 {
                opaque_rgba_cases += 1;
            }
            let pixmap = XResourceId::new(0x3000 + u64::from(config) * 10 + index * 2, 1);
            let glx_pixmap = XResourceId::new(pixmap.local.raw() + 1, 1);
            client.create_pixmap(pixmap, depth);
            let outputs = client.send(XWireRequest::GlxCreatePixmap {
                screen: 0,
                fbconfig: config,
                pixmap,
                glx_pixmap,
                target: Some(X_GLX_TEXTURE_2D_VALUE),
                format: Some(format),
                mipmap: Some(false),
            });
            if advertised == 1 {
                assert!(
                    outputs.is_empty(),
                    "config {config} advertises format {format:#x} but rejects it: {outputs:?}"
                );
                assert_eq!(
                    client
                        .runtime
                        .glx_pixmap(NamespaceId::from_raw(71), glx_pixmap)
                        .unwrap(),
                    (pixmap, config)
                );
                let (_, _, texture) = client
                    .runtime
                    .glx_pixmap_attributes(NamespaceId::from_raw(71), glx_pixmap)
                    .unwrap();
                assert_eq!(
                    texture.format, format,
                    "the requested format must survive creation"
                );
            } else {
                assert!(
                    matches!(outputs.as_slice(), [XClientOutput::Error(_)]),
                    "config {config} accepts unadvertised format {format:#x}: {outputs:?}"
                );
                assert!(
                    client
                        .runtime
                        .glx_pixmap(NamespaceId::from_raw(71), glx_pixmap)
                        .is_err()
                );
            }
        }
    }
    assert!(
        opaque_rgba_cases > 0,
        "opaque RGBA must exercise the advertised compatibility row"
    );
}

fn advertised_configs(client: &mut Client) -> Vec<Vec<(u32, u32)>> {
    match client
        .send(XWireRequest::GlxGetFbConfigs { screen: 0 })
        .as_slice()
    {
        [XClientOutput::Reply(XClientReply::GlxFbConfigs { configs, .. })] => configs.clone(),
        other => panic!("unexpected configuration reply: {other:?}"),
    }
}

fn assert_pixmap_geometry(client: &mut Client, drawable: XResourceId, depth: u8) {
    let outputs = client.send_with_opcode(14, XWireRequest::GetGeometry { drawable });
    assert!(
        matches!(outputs.as_slice(),
            [XClientOutput::Reply(XClientReply::GetGeometry { depth: actual, geometry, .. })]
            if *actual == depth && geometry.width == 3 && geometry.height == 2),
        "drawable must report its actual backing depth {depth} and extent: {outputs:?}"
    );
}

#[test]
fn default_visual_stays_depth24_while_its_first_glx_config_has_rgba8() {
    for supported in [false, true] {
        let mut client = Client::new(supported);
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let geometry = client.send_with_opcode(14, XWireRequest::GetGeometry { drawable: root });
        assert!(matches!(
            geometry.as_slice(),
            [XClientOutput::Reply(XClientReply::GetGeometry {
                depth: 24,
                ..
            })]
        ));
        let attributes =
            client.send_with_opcode(3, XWireRequest::GetWindowAttributes { window: root });
        assert!(matches!(attributes.as_slice(),
            [XClientOutput::Reply(XClientReply::GetWindowAttributes { visual, .. })]
            if *visual == X_SETUP_DEFAULT_VISUAL));

        let configs = advertised_configs(&mut client);
        let first = configs
            .iter()
            .find(|attributes| {
                config_attribute(attributes, X_GLX_VISUAL_ID_ATTRIBUTE) == X_SETUP_DEFAULT_VISUAL
            })
            .expect("default visual must have a GLX configuration");
        assert_eq!(config_attribute(first, X_GLX_FBCONFIG_ID_ATTRIBUTE), 1);
        for attribute in [
            X_GLX_RED_SIZE_ATTRIBUTE,
            X_GLX_GREEN_SIZE_ATTRIBUTE,
            X_GLX_BLUE_SIZE_ATTRIBUTE,
            X_GLX_ALPHA_SIZE_ATTRIBUTE,
        ] {
            assert_eq!(config_attribute(first, attribute), 8);
        }
        assert_eq!(config_attribute(first, X_GLX_BUFFER_SIZE_ATTRIBUTE), 32);
    }
}

#[test]
fn legacy_visual_configs_agree_with_the_first_modern_config_for_each_visual() {
    for supported in [false, true] {
        let mut client = Client::new(supported);
        let modern = advertised_configs(&mut client);
        let legacy = match client
            .send(XWireRequest::GlxGetVisualConfigs { screen: 0 })
            .as_slice()
        {
            [XClientOutput::Reply(XClientReply::GlxVisualConfigs { configs, .. })] => {
                configs.clone()
            }
            other => panic!("unexpected legacy configuration reply: {other:?}"),
        };
        assert_eq!(legacy.len(), 2);
        for visual in [X_SETUP_DEFAULT_VISUAL, X_SETUP_ARGB_VISUAL] {
            let old = legacy
                .iter()
                .find(|row| row[0] == visual)
                .expect("legacy visual");
            let current = modern
                .iter()
                .find(|row| config_attribute(row, X_GLX_VISUAL_ID_ATTRIBUTE) == visual)
                .expect("modern visual");
            for (slot, attribute) in [
                (3, X_GLX_RED_SIZE_ATTRIBUTE),
                (4, X_GLX_GREEN_SIZE_ATTRIBUTE),
                (5, X_GLX_BLUE_SIZE_ATTRIBUTE),
                (6, X_GLX_ALPHA_SIZE_ATTRIBUTE),
                (11, X_GLX_DOUBLEBUFFER_ATTRIBUTE),
                (13, X_GLX_BUFFER_SIZE_ATTRIBUTE),
                (14, X_GLX_DEPTH_SIZE_ATTRIBUTE),
                (15, X_GLX_STENCIL_SIZE_ATTRIBUTE),
            ] {
                assert_eq!(
                    old[slot],
                    config_attribute(current, attribute),
                    "visual {visual:#x}, legacy slot {slot}, attribute {attribute:#x}"
                );
            }
        }
    }
}

#[test]
fn modern_default_visual_pixmaps_accept_rgb24_and_argb32_and_retain_actual_depth() {
    let mut client = Client::new(true);
    let configs = advertised_configs(&mut client)
        .into_iter()
        .filter(|row| config_attribute(row, X_GLX_VISUAL_ID_ATTRIBUTE) == X_SETUP_DEFAULT_VISUAL)
        .collect::<Vec<_>>();
    assert_eq!(configs.len(), 2);
    for row in configs {
        let fbconfig = config_attribute(&row, X_GLX_FBCONFIG_ID_ATTRIBUTE);
        for depth in [24, 32] {
            let pixmap =
                XResourceId::new(0x5000 + u64::from(fbconfig) * 100 + u64::from(depth) * 2, 1);
            let glx_pixmap = XResourceId::new(pixmap.local.raw() + 1, 1);
            client.create_pixmap(pixmap, depth);
            let outputs = client.send(XWireRequest::GlxCreatePixmap {
                screen: 0,
                fbconfig,
                pixmap,
                glx_pixmap,
                target: Some(X_GLX_TEXTURE_2D_VALUE),
                format: Some(X_GLX_TEXTURE_FORMAT_RGBA_VALUE),
                mipmap: Some(false),
            });
            assert!(
                outputs.is_empty(),
                "config {fbconfig}, depth {depth}: {outputs:?}"
            );
            assert_pixmap_geometry(&mut client, glx_pixmap, depth);
            assert!(
                client
                    .send_with_opcode(54, XWireRequest::FreePixmap { pixmap })
                    .is_empty()
            );
            assert_pixmap_geometry(&mut client, glx_pixmap, depth);
            client.create_pixmap(pixmap, if depth == 24 { 32 } else { 24 });
            assert_pixmap_geometry(&mut client, glx_pixmap, depth);
        }
    }
}

#[test]
fn legacy_default_visual_pixmaps_require_the_native_depth() {
    for depth in [24, 32] {
        let mut client = Client::new(true);
        let pixmap = XResourceId::new(0x6000, 1);
        let glx_pixmap = XResourceId::new(0x6001, 1);
        client.create_pixmap(pixmap, depth);
        let outputs = client.send(XWireRequest::GlxCreateGlxPixmap {
            screen: 0,
            visual: X_SETUP_DEFAULT_VISUAL,
            pixmap,
            glx_pixmap,
        });
        if depth == 24 {
            assert!(
                outputs.is_empty(),
                "legacy native-depth pixmap: {outputs:?}"
            );
            assert_pixmap_geometry(&mut client, glx_pixmap, depth);
        } else {
            assert!(
                matches!(outputs.as_slice(), [XClientOutput::Error(_)]),
                "legacy visual constructor accepted non-native depth: {outputs:?}"
            );
            assert!(
                client
                    .runtime
                    .glx_pixmap(NamespaceId::from_raw(71), glx_pixmap)
                    .is_err()
            );
            assert_pixmap_geometry(&mut client, pixmap, depth);
        }
    }
}

#[test]
fn glx_pixmaps_refuse_unsupported_core_depths_without_creating_an_alias() {
    for depth in [1, 8, 16] {
        for legacy in [false, true] {
            let mut client = Client::new(true);
            let pixmap = XResourceId::new(0x7000, 1);
            let glx_pixmap = XResourceId::new(0x7001, 1);
            client.create_pixmap(pixmap, depth);
            let request = if legacy {
                XWireRequest::GlxCreateGlxPixmap {
                    screen: 0,
                    visual: X_SETUP_DEFAULT_VISUAL,
                    pixmap,
                    glx_pixmap,
                }
            } else {
                XWireRequest::GlxCreatePixmap {
                    screen: 0,
                    fbconfig: 1,
                    pixmap,
                    glx_pixmap,
                    target: Some(X_GLX_TEXTURE_2D_VALUE),
                    format: Some(X_GLX_TEXTURE_FORMAT_RGBA_VALUE),
                    mipmap: Some(false),
                }
            };
            let outputs = client.send(request);
            assert!(
                matches!(outputs.as_slice(), [XClientOutput::Error(_)]),
                "legacy={legacy} accepted unsupported depth {depth}: {outputs:?}"
            );
            assert!(
                client
                    .runtime
                    .glx_pixmap(NamespaceId::from_raw(71), glx_pixmap)
                    .is_err()
            );
            assert_pixmap_geometry(&mut client, pixmap, depth);
        }
    }
}
