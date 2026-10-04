use super::config::Config;

fn parse(args: &[&str]) -> super::Result<Config> {
    Config::from_args(
        ["--output=unused.json"]
            .into_iter()
            .chain(args.iter().copied())
            .map(str::to_owned),
    )
}

#[test]
fn a_head_sized_workload_requires_one_window() {
    assert!(parse(&["--size=head"]).is_err());
    assert!(parse(&["--size=head", "--clients=2"]).is_err());
    for damage in ["absent", "full", "patch"] {
        let arg = format!("--damage={damage}");
        let config = parse(&["--size=head", "--clients=1", &arg]).unwrap();
        assert_eq!(config.json()["size"], "head");
        assert_eq!(config.json()["damage"], damage);
        assert_eq!(config.json()["pattern"], "fixed_patch_v1");
    }
}

#[test]
fn malformed_damage_or_size_cannot_silently_select_a_different_workload() {
    assert!(parse(&["--damage=unknown"]).is_err());
    assert!(parse(&["--size=0"]).is_err());
    let defaults = parse(&[]).unwrap();
    assert_eq!(defaults.size, "small");
    assert_eq!(defaults.damage, "absent");
}
