use clap::Parser;
use hell_hound::launcher::{Map, Options, arguments};

#[test]
fn each_map_has_one_deferred_devmap_and_one_god_request() {
    for map in Map::ALL {
        let options = Options::try_parse_from(["waw-zombies", "--map", map.name()]).unwrap();
        let args = arguments(&options);
        assert_eq!(args.iter().filter(|a| *a == "+devmap").count(), 1);
        assert_eq!(args.iter().filter(|a| *a == "+god").count(), 1);
        let devmap = args.iter().position(|a| a == "+devmap").unwrap();
        let disconnect = args.iter().position(|a| a == "+disconnect").unwrap();
        let wait = args.iter().position(|a| a == "+wait").unwrap();
        assert!(wait < disconnect && disconnect < devmap);
        assert_eq!(args[devmap + 1], map.internal());
        assert_eq!(&args[args.len() - 3..], ["+wait", "300", "+god"]);
    }
}

#[test]
fn map_loading_can_be_isolated_without_god_or_display_override() {
    let options = Options::try_parse_from([
        "waw-zombies",
        "--no-god",
        "--keep-display",
        "--startup-frames",
        "1200",
    ])
    .unwrap();
    let args = arguments(&options);
    assert!(!args.contains(&"+god".into()));
    assert!(!args.contains(&"r_fullscreen".into()));
    assert!(args.contains(&"1200".into()));
}

#[test]
fn invalid_options_fail() {
    for args in [
        vec!["--map", "unknown"],
        vec!["--wait-frames", "0"],
        vec!["--startup-frames", "-1"],
        vec!["--wait-frames", "no"],
        vec!["--flatpak", "--steam", "steam"],
        vec!["--flatpak", "--native"],
    ] {
        assert!(Options::try_parse_from(std::iter::once("waw-zombies").chain(args)).is_err());
    }
}

#[test]
fn dry_run_prints_argument_array_without_starting_a_game() {
    let binary = env!("CARGO_BIN_EXE_waw-zombies");
    let output = std::process::Command::new(binary)
        .args(["--map", "verruckt", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let args: Vec<String> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(args.contains(&"nazi_zombie_asylum".into()));
    assert_eq!(args.iter().filter(|a| *a == "+devmap").count(), 1);
}

#[test]
fn list_maps_does_not_require_steam() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_waw-zombies"))
        .arg("--list-maps")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 4);
}
