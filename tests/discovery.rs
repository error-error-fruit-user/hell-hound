use hell_hound::{
    discovery::{self, Game, Options},
    recipe,
};
use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;

fn game(pid: u32, id: &str, exe: &str) -> Game {
    Game {
        pid,
        id: id.into(),
        title: id.into(),
        executable: exe.into(),
        directory: None,
        image_base: None,
        live_inspection: id == "waw",
    }
}
#[test]
fn known_process_names_are_case_insensitive_without_claiming_decoders() {
    assert_eq!(
        discovery::identify(r"Z:\Games\CoDWaW.exe").unwrap().0,
        "waw"
    );
    assert_eq!(discovery::identify("BLACKOPS.EXE").unwrap().0, "black-ops");
    assert!(discovery::identify("Greyhound.exe").is_none());
    assert!(discovery::identify("steam.exe").is_none());
    assert!(game(1, "cod4", "iw3sp.exe").waw_mode().is_err());
}
#[test]
#[cfg(target_os = "linux")]
fn mapped_paths_preserve_spaces_and_only_z_drive_is_guessed() {
    let entry = "00400000-00500000 r-xp 00000000 08:01 123 /home/user/Steam Games/CoDWaW.exe";
    let (path, base) = discovery::mapped_executable(entry, "codwaw.exe").unwrap();
    assert_eq!(base, 0x400000);
    assert_eq!(path, Path::new("/home/user/Steam Games/CoDWaW.exe"));
    assert_eq!(
        discovery::unix_executable(r"Z:\home\user\CoDWaW.exe").unwrap(),
        Path::new("/home/user/CoDWaW.exe")
    );
    assert!(discovery::unix_executable(r"C:\Games\CoDWaW.exe").is_none());
}
#[test]
fn game_and_pid_filters_and_picker_bounds() {
    let games = vec![game(10, "waw", "CoDWaW.exe"), game(11, "cod4", "iw3sp.exe")];
    assert_eq!(
        discovery::matching(
            games.clone(),
            &Options {
                game: Some("waw".into()),
                ..Default::default()
            }
        )
        .len(),
        1
    );
    assert!(
        discovery::matching(
            games,
            &Options {
                pid: Some(99),
                ..Default::default()
            }
        )
        .is_empty()
    );
    assert_eq!(discovery::choose_index("2", 2).unwrap(), 1);
    for value in ["0", "3", "q", "-1"] {
        assert!(discovery::choose_index(value, 2).is_err());
    }
    assert!(
        discovery::validate_options(&Options {
            game: Some("invented-game".into()),
            ..Default::default()
        })
        .is_err()
    );
}
#[test]
fn discover_iwds_from_directory_without_filenames_and_recipe_supports_game_dir() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("main")).unwrap();
    fs::write(root.path().join("main/iw_01.iwd"), b"fixture").unwrap();
    fs::write(root.path().join("main/iw_02.IWD"), b"fixture").unwrap();
    fs::write(root.path().join("main/test.ff"), b"not supported").unwrap();
    let sources = discovery::archives_in(root.path()).unwrap();
    assert_eq!(sources.len(), 2);
    let recipe_path = root.path().join("extract.toml");
    fs::write(
        &recipe_path,
        "version=1\ngame_dir='.'\noutput='out'\n[[extract]]\ninclude=['*']",
    )
    .unwrap();
    assert_eq!(recipe::load(&recipe_path).unwrap().0, sources);
}
#[test]
fn weapon_recipe_has_strict_exact_selection_and_relative_output() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("weapon.toml");
    fs::write(&path,"version=1\ngame='waw'\noutput='out/colt.glb'\n[weapon]\nmodels=['viewmodel_colt']\nanimations=['viewmodel_colt_fire']").unwrap();
    let job = recipe::weapon_job(&path).unwrap().unwrap();
    assert_eq!(job.weapon.models, vec!["viewmodel_colt"]);
    assert!(job.output.is_absolute());
    fs::write(&path,"version=1\ngame='waw'\noutput='out.glb'\n[weapon]\nmodels=['colt']\ncommand='never execute'").unwrap();
    assert!(recipe::weapon_job(&path).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_scanner_finds_wine_game_directory_and_base_without_reading_game_memory() {
    let root = tempfile::tempdir().unwrap();
    let install = root.path().join("Steam Games/WaW");
    fs::create_dir_all(install.join("main")).unwrap();
    fs::write(install.join("CoDWaW.exe"), b"synthetic name only").unwrap();
    let proc = root.path().join("proc");
    let process = proc.join("123");
    fs::create_dir_all(&process).unwrap();
    fs::write(process.join("comm"), "CoDWaW.exe\n").unwrap();
    fs::write(
        process.join("cmdline"),
        format!("Z:{}\0", install.join("CoDWaW.exe").display()),
    )
    .unwrap();
    fs::write(
        process.join("maps"),
        format!(
            "00400000-00500000 r-xp 00000000 08:01 123 {}\n",
            install.join("CoDWaW.exe").display()
        ),
    )
    .unwrap();
    let games = discovery::discover_from_proc(&proc).unwrap();
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].pid, 123);
    assert_eq!(games[0].directory.as_ref().unwrap(), &install);
    assert_eq!(games[0].image_base, Some(0x400000));
}
