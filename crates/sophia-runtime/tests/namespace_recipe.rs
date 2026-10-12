use sophia_protocol::{NamespaceId, namespace_view::*};
use sophia_runtime::namespace_recipe::{self, Error, Input};
use std::collections::BTreeMap;

fn endpoint(identity: u64) -> GrantedEndpoint {
    GrantedEndpoint {
        service: ServiceReference {
            identity,
            generation: 7,
            protocol: EndpointProtocol::NineP2000L,
            interface: "inspection".into(),
            version: 1,
        },
        scope: EndpointGrantScope {
            namespace: NamespaceId::from_raw(1),
            principal: 9,
            grant: identity,
            generation: 3,
        },
    }
}
fn compile(source: &str, fragments: &BTreeMap<String, String>) -> Result<CompiledNamespace, Error> {
    namespace_recipe::compile(Input {
        revision: 5,
        namespace: NamespaceId::from_raw(1),
        principal: 9,
        source,
        fragments,
        services: &BTreeMap::from([("a".into(), endpoint(11)), ("b".into(), endpoint(12))]),
    })
}
fn run(operations: &str) -> Result<CompiledNamespace, Error> {
    compile(
        &format!("namespace-recipe 1\n{operations}"),
        &BTreeMap::new(),
    )
}

#[test]
fn aliases_capture_identity_and_scope_before_rebinding() {
    let view =
        run("mount a /service\nbind /service /old\nunmount /service\nmount b /service").unwrap();
    let named = |name| view.endpoints.iter().find(|e| e.path == name).unwrap();
    assert_eq!(named("old").endpoint, endpoint(11));
    assert_eq!(named("service").endpoint, endpoint(12));
    assert_eq!(view.revision, 5);
    assert_eq!(named("service").endpoint.service.generation, 7);
}

#[test]
fn a_name_cannot_widen_the_selected_identitys_grants() {
    for foreign_namespace in [false, true] {
        let mut foreign = endpoint(11);
        if foreign_namespace {
            foreign.scope.namespace = NamespaceId::from_raw(2);
        } else {
            foreign.scope.principal = 10;
        }
        let result = namespace_recipe::compile(Input {
            revision: 1,
            namespace: NamespaceId::from_raw(1),
            principal: 9,
            source: "namespace-recipe 1\nmount foreign service",
            fragments: &BTreeMap::new(),
            services: &BTreeMap::from([("foreign".into(), foreign)]),
        });
        assert_eq!(
            result,
            Err(Error::Scope),
            "foreign admission scope must refuse before publication"
        );
    }
}

#[test]
fn private_views_can_share_names_without_sharing_service_identity() {
    let first = run("mount a service").unwrap();
    let second = run("mount b service").unwrap();
    assert_eq!(first.endpoints[0].path, second.endpoints[0].path);
    assert_ne!(
        first.endpoints[0].endpoint.service,
        second.endpoints[0].endpoint.service
    );
    let shared = run("mount a service\nbind service alias").unwrap();
    assert_eq!(shared.endpoints[0].endpoint, shared.endpoints[1].endpoint);
}

#[test]
fn directory_composition_orders_members_and_deduplicates_only_identical_leaves() {
    let source = "mount a left/z\nmount a left/same\nmount b right/a\nmount a right/same\n";
    for (flag, expected) in [
        ("-a", vec!["joined/z", "joined/same", "joined/a"]),
        ("-b", vec!["joined/a", "joined/same", "joined/z"]),
    ] {
        let view = run(&format!(
            "{source}bind left joined\nbind {flag} right joined"
        ))
        .unwrap();
        let names: Vec<_> = view
            .endpoints
            .iter()
            .filter(|e| e.path.starts_with("joined/"))
            .map(|e| e.path.as_str())
            .collect();
        assert_eq!(names, expected);
    }
    assert_eq!(
        run("mount a l/x\nmount b r/x\nbind -a r l"),
        Err(Error::Conflict)
    );
    assert_eq!(
        run("mount a l/x\nmount b r/x/y\nbind -a r l"),
        Err(Error::Conflict)
    );
    assert_eq!(run("mount a leaf\nbind -a leaf leaf"), Err(Error::Kind));
    assert_eq!(run("mount a l/x\nbind -a l absent"), Err(Error::Missing));
}

#[test]
fn explicit_unmount_is_required_and_sources_must_exist() {
    assert!(run("mount a x\nmount a x").is_ok());
    assert_eq!(run("mount a x\nmount b x"), Err(Error::Conflict));
    assert_eq!(run("bind absent x"), Err(Error::Missing));
    assert_eq!(run("unmount absent"), Err(Error::Missing));
    assert_eq!(run("mount a x\nmount b x/y"), Err(Error::Kind));
    assert_eq!(run("mount a /"), Err(Error::Path));
    assert_eq!(run("unmount /"), Err(Error::Path));
}

#[test]
fn overlap_is_a_finite_snapshot_not_a_recursive_alias() {
    let view = run("mount a x/a\nbind x x/child").unwrap();
    assert_eq!(
        view.endpoints
            .iter()
            .map(|e| e.path.as_str())
            .collect::<Vec<_>>(),
        ["x/a", "x/child/a"]
    );
    let view = run("mount a x/a\nbind -a x /").unwrap();
    assert_eq!(view.endpoints.len(), 2);
}

#[test]
fn include_cd_persists_but_includes_are_root_relative_and_bounded() {
    let fragments = BTreeMap::from([
        ("one".into(), "cd /roles\ninclude nested/two".into()),
        ("nested/two".into(), "mount a first".into()),
    ]);
    let view = compile(
        "namespace-recipe 1\ninclude one\nmount b second",
        &fragments,
    )
    .unwrap();
    assert_eq!(
        view.endpoints
            .iter()
            .map(|e| e.path.as_str())
            .collect::<Vec<_>>(),
        ["roles/first", "roles/second"]
    );
    let cycle = BTreeMap::from([
        ("one".into(), "include two".into()),
        ("two".into(), "include ./one".into()),
    ]);
    assert_eq!(
        compile("namespace-recipe 1\ninclude one", &cycle),
        Err(Error::IncludeCycle)
    );
    assert_eq!(run("include ../outside"), Err(Error::Path));
    assert_eq!(run("include /absolute"), Err(Error::Path));
    for depth in [8, 9] {
        let fragments = (0..depth)
            .map(|i| {
                (
                    format!("f{i}"),
                    if i + 1 == depth {
                        "mount a x".into()
                    } else {
                        format!("include f{}", i + 1)
                    },
                )
            })
            .collect();
        let result = compile("namespace-recipe 1\ninclude f0", &fragments);
        if depth == 8 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(Error::Limit));
        }
    }
}

#[test]
fn path_escape_reserved_discovery_and_unknown_grammar_refuse() {
    for operation in [
        "mount a /../escape",
        "cd ../escape",
        "mount a discovery",
        "mount a /discovery/child",
        "mount a /a\0b",
    ] {
        assert!(run(operation).is_err(), "accepted {operation:?}");
    }
    for operation in [
        "mount -a a x",
        "bind -c a x",
        "clear",
        "mount a x extra",
        "namespace-recipe 1",
        "mount\ra x",
    ] {
        assert_eq!(run(operation), Err(Error::Syntax), "accepted {operation:?}");
    }
    assert_eq!(compile("", &BTreeMap::new()), Err(Error::Syntax));
    assert!(run("# comment\ncd /foo/../bar\nmount\ta\t./x").is_ok());
}

#[test]
fn source_operations_endpoint_and_path_limits_include_aliases() {
    let header = "namespace-recipe 1\n#";
    let at = format!(
        "{header}{}",
        "x".repeat(namespace_recipe::MAX_SOURCE_BYTES - header.len())
    );
    assert!(compile(&at, &BTreeMap::new()).is_ok());
    assert_eq!(compile(&(at + "x"), &BTreeMap::new()), Err(Error::Limit));
    assert!(run(&"cd /\n".repeat(1024)).is_ok());
    assert_eq!(run(&"cd /\n".repeat(1025)), Err(Error::Limit));
    let names = (0..64)
        .map(|i| format!("mount a p{i}\n"))
        .collect::<String>();
    assert_eq!(run(&names).unwrap().endpoints.len(), 64);
    assert_eq!(run(&(names + "bind p0 another")), Err(Error::Limit));
    assert!(run(&format!("mount a {}", "x".repeat(255))).is_ok());
    assert_eq!(
        run(&format!("mount a {}", "x".repeat(256))),
        Err(Error::Limit)
    );
    let path = vec!["x".repeat(255); 4].join("/");
    assert!(run(&format!("mount a {path}")).is_ok());
    assert_eq!(run(&format!("mount a {path}/x")), Err(Error::Limit));
    assert_eq!(
        run(&format!(
            "mount a s/{}\nbind s elsewhere",
            vec!["x".repeat(253); 4].join("/")
        )),
        Err(Error::Limit)
    );
}

#[test]
fn distinct_includes_and_repeated_expansion_are_independently_bounded() {
    let fragments = (0..33)
        .map(|i| (format!("f{i}"), "# empty fragment".to_owned()))
        .collect();
    let source = (0..32)
        .map(|i| format!("include f{i}\n"))
        .collect::<String>();
    assert!(compile(&format!("namespace-recipe 1\n{source}"), &fragments).is_ok());
    assert_eq!(
        compile(
            &format!("namespace-recipe 1\n{source}include f32"),
            &fragments
        ),
        Err(Error::Limit)
    );
    let fragments = BTreeMap::from([("large".into(), format!("#{}", "x".repeat(32767)))]);
    assert!(compile("namespace-recipe 1\ninclude large", &fragments).is_ok());
    assert_eq!(
        compile(
            "namespace-recipe 1\ninclude large\ninclude large",
            &fragments
        ),
        Err(Error::Limit)
    );
}

#[test]
fn empty_directory_snapshots_cannot_amplify_without_bound() {
    let mut source = "mount a tree/empty/leaf\nunmount tree/empty/leaf\n".to_owned();
    for i in 0..16 {
        source.push_str(&format!("bind tree tree/c{i}\n"));
    }
    assert_eq!(run(&source), Err(Error::Limit));
}
