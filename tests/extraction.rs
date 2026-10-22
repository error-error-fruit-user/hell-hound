use hell_hound::{
    archive::{self, DEFAULT_LIMIT, MANIFEST, Rule},
    recipe,
};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use zip::{ZipWriter, write::SimpleFileOptions};

struct Fixture {
    root: TempDir,
    source: PathBuf,
    output: PathBuf,
}

fn archive(path: &Path, entries: &[(&str, &[u8])]) {
    let mut writer = ZipWriter::new(File::create(path).unwrap());
    for (name, data) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap();
}

impl Fixture {
    fn new() -> Self {
        let root = if Path::new("/tmp/opencode").is_dir() {
            tempfile::tempdir_in("/tmp/opencode").unwrap()
        } else {
            tempfile::tempdir().unwrap()
        };
        let source = root.path().join("base.iwd");
        archive(
            &source,
            &[
                ("images/colt.iwi", b"texture"),
                ("images/colt_n.iwi", b"normal"),
                ("sound/weapons/colt/fire.wav", b"RIFFaudio"),
                ("config/test.cfg", b"original"),
            ],
        );
        let source = source.canonicalize().unwrap();
        let output = root.path().join("output");
        Self {
            root,
            source,
            output,
        }
    }

    fn plan(&self, rule: Rule) -> Vec<archive::Planned> {
        archive::select(
            &archive::catalog(std::slice::from_ref(&self.source)).unwrap(),
            &[rule],
            DEFAULT_LIMIT,
        )
        .unwrap()
    }
}

fn all() -> Rule {
    Rule {
        include: vec!["*".into()],
        ..Rule::default()
    }
}
fn exact(name: &str) -> Rule {
    Rule {
        names: vec![name.into()],
        ..Rule::default()
    }
}

#[test]
fn exact_selection_routing_and_bytes() {
    let f = Fixture::new();
    let rule = Rule {
        into: "weapons/colt".into(),
        ..exact("images/colt.iwi")
    };
    let manifest =
        archive::extract(&f.plan(rule), &f.output, DEFAULT_LIMIT, false, false, None).unwrap();
    assert_eq!(
        fs::read(f.output.join("weapons/colt/images/colt.iwi")).unwrap(),
        b"texture"
    );
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(
        manifest.assets[0].sha256,
        archive::file_hash(&f.output.join("weapons/colt/images/colt.iwi")).unwrap()
    );
    let saved: archive::Manifest =
        serde_json::from_slice(&fs::read(f.output.join(MANIFEST)).unwrap()).unwrap();
    assert_eq!(saved, manifest);
}

#[test]
fn globs_exclusions_and_kind() {
    let f = Fixture::new();
    let plan = f.plan(Rule {
        include: vec!["*colt*".into()],
        exclude: vec!["*_n.iwi".into()],
        kind: "image".into(),
        ..Rule::default()
    });
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].asset.member, "images/colt.iwi");
}

#[test]
fn selection_is_case_sensitive_and_required_names_are_strict() {
    let f = Fixture::new();
    let assets = archive::catalog(&[f.source]).unwrap();
    for rule in [
        exact("images/COLT.iwi"),
        Rule {
            names: vec!["images/colt.iwi".into(), "missing".into()],
            ..Rule::default()
        },
        Rule::default(),
    ] {
        assert!(archive::select(&assets, &[rule], DEFAULT_LIMIT).is_err());
    }
}

#[test]
fn optional_empty_rule_and_invalid_glob() {
    let f = Fixture::new();
    assert!(
        f.plan(Rule {
            required: false,
            ..exact("missing")
        })
        .is_empty()
    );
    assert!(
        Rule {
            include: vec!["[".into()],
            ..Rule::default()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn source_overlay_last_wins() {
    let f = Fixture::new();
    let patch = f.root.path().join("patch.iwd");
    archive(&patch, &[("images/colt.iwi", b"patched")]);
    let assets = archive::catalog(&[f.source, patch.canonicalize().unwrap()]).unwrap();
    let plan = archive::select(&assets, &[exact("images/colt.iwi")], DEFAULT_LIMIT).unwrap();
    archive::extract(&plan, &f.output, DEFAULT_LIMIT, false, false, None).unwrap();
    assert_eq!(
        fs::read(f.output.join("images/colt.iwi")).unwrap(),
        b"patched"
    );
}

#[test]
fn dry_run_does_not_create_output() {
    let f = Fixture::new();
    let manifest =
        archive::extract(&f.plan(all()), &f.output, DEFAULT_LIMIT, false, true, None).unwrap();
    assert_eq!(manifest.assets.len(), 4);
    assert!(!f.output.exists());
}

#[test]
fn repeated_extraction_and_locked_replay_are_identical() {
    let f = Fixture::new();
    let plan = f.plan(all());
    let first = archive::extract(&plan, &f.output, DEFAULT_LIMIT, false, false, None).unwrap();
    let bytes = fs::read(f.output.join(MANIFEST)).unwrap();
    let second =
        archive::extract(&plan, &f.output, DEFAULT_LIMIT, false, false, Some(&first)).unwrap();
    assert_eq!(first, second);
    assert_eq!(bytes, fs::read(f.output.join(MANIFEST)).unwrap());
}

#[test]
fn lock_mismatch_fails_before_writes() {
    let f = Fixture::new();
    let plan = f.plan(all());
    let mut lock = archive::extract(&plan, &f.output, DEFAULT_LIMIT, false, true, None).unwrap();
    lock.assets[0].sha256 = "wrong".into();
    assert!(archive::extract(&plan, &f.output, DEFAULT_LIMIT, false, false, Some(&lock)).is_err());
    assert!(!f.output.exists());
}

#[test]
fn archive_change_invalidates_lock_even_when_selected_bytes_match() {
    let f = Fixture::new();
    let plan = f.plan(exact("images/colt.iwi"));
    let lock = archive::extract(&plan, &f.output, DEFAULT_LIMIT, false, true, None).unwrap();
    archive(
        &f.source,
        &[("images/colt.iwi", b"texture"), ("extra", b"new")],
    );
    let plan = f.plan(exact("images/colt.iwi"));
    assert!(archive::extract(&plan, &f.output, DEFAULT_LIMIT, false, false, Some(&lock)).is_err());
    assert!(!f.output.exists());
}

#[test]
fn existing_edits_are_not_overwritten_and_preflight_prevents_partial_writes() {
    let f = Fixture::new();
    fs::create_dir_all(f.output.join("images")).unwrap();
    fs::write(f.output.join("images/colt.iwi"), b"my edit").unwrap();
    let plan = f.plan(all());
    assert!(archive::extract(&plan, &f.output, DEFAULT_LIMIT, false, false, None).is_err());
    assert!(!f.output.join("config/test.cfg").exists());
    assert_eq!(
        fs::read(f.output.join("images/colt.iwi")).unwrap(),
        b"my edit"
    );
    archive::extract(&plan, &f.output, DEFAULT_LIMIT, true, false, None).unwrap();
    assert_eq!(
        fs::read(f.output.join("images/colt.iwi")).unwrap(),
        b"texture"
    );
}

#[test]
fn unrelated_manifest_is_never_adopted() {
    let f = Fixture::new();
    fs::create_dir(&f.output).unwrap();
    fs::write(f.output.join(MANIFEST), br#"{"mine":true}"#).unwrap();
    assert!(archive::extract(&f.plan(all()), &f.output, DEFAULT_LIMIT, true, false, None).is_err());
    assert!(!f.output.join("images").exists());
}

#[test]
fn case_and_file_directory_collisions_fail() {
    let f = Fixture::new();
    for entries in [
        vec![("images/A.iwi", &b"a"[..]), ("images/a.iwi", &b"b"[..])],
        vec![("images", &b"a"[..]), ("images/a.iwi", &b"b"[..])],
        vec![("hell-hound-manifest.json/subfile", &b"a"[..])],
    ] {
        archive(&f.source, &entries);
        assert!(
            archive::catalog(std::slice::from_ref(&f.source))
                .and_then(|assets| archive::select(&assets, &[all()], DEFAULT_LIMIT))
                .is_err()
        );
    }
}

#[test]
fn later_archives_override_ignoring_case() {
    let f = Fixture::new();
    let patch = f.source.with_file_name("patch.iwd");
    archive(&f.source, &[("sound/Stream/a.wav", &b"old"[..])]);
    archive(&patch, &[("sound/stream/A.wav", &b"new"[..])]);
    let plan = archive::select(
        &archive::catalog(&[f.source.clone(), patch.clone()]).unwrap(),
        &[all()],
        DEFAULT_LIMIT,
    )
    .unwrap();
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].asset.member, "sound/stream/A.wav");
    assert_eq!(plan[0].asset.source, patch);
}

#[test]
fn selected_size_limit_and_zero_limit_fail() {
    let f = Fixture::new();
    let assets = archive::catalog(&[f.source]).unwrap();
    assert!(archive::select(&assets, &[all()], 1).is_err());
    assert!(archive::select(&assets, &[all()], 0).is_err());
}

#[test]
fn portable_paths_reject_traversal_drives_devices_and_aliases() {
    for path in [
        "../escape",
        "/absolute",
        "C:/bad",
        "a\\bad",
        "a//b",
        "a/./b",
        "CON.txt",
        "nul",
        "a/LPT1.iwi",
        "a/trailing.",
        "a/trailing ",
        "a/*",
        "a/\0",
    ] {
        assert!(archive::safe_relative(path, false).is_err(), "{path:?}");
    }
    assert!(archive::safe_relative("images/~colt_s-rgb&g.iwi", false).is_ok());
}

#[test]
fn malicious_archive_paths_are_rejected() {
    let f = Fixture::new();
    archive(&f.source, &[("../escape", b"no")]);
    assert!(archive::catalog(&[f.source]).is_err());
}

#[cfg(unix)]
#[test]
fn output_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    fs::create_dir(&f.output).unwrap();
    let elsewhere = f.root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, f.output.join("images")).unwrap();
    assert!(
        archive::extract(&f.plan(all()), &f.output, DEFAULT_LIMIT, false, false, None).is_err()
    );
    assert_eq!(fs::read_dir(elsewhere).unwrap().count(), 0);
}

#[test]
fn source_output_overlap_is_rejected() {
    let f = Fixture::new();
    archive(&f.source, &[("base.iwd", b"do not overwrite archive")]);
    assert!(
        archive::extract(
            &f.plan(all()),
            f.root.path(),
            DEFAULT_LIMIT,
            true,
            false,
            None
        )
        .is_err()
    );
    assert!(archive::catalog(&[f.source]).is_ok());
}

#[test]
fn globs_missing_sources_and_fastfiles() {
    let f = Fixture::new();
    assert_eq!(
        archive::sources_from(&["*.iwd".into()], f.root.path()).unwrap(),
        vec![f.source]
    );
    assert!(archive::sources_from(&["missing*".into()], f.root.path()).is_err());
    fs::write(f.root.path().join("bad.ff"), b"fastfile").unwrap();
    assert!(
        archive::sources_from(&["bad.ff".into()], f.root.path())
            .unwrap_err()
            .to_string()
            .contains("not implemented")
    );
}

#[test]
fn recipe_paths_are_relative_to_recipe_not_cwd() {
    let f = Fixture::new();
    let path = f.root.path().join("recipe.toml");
    fs::write(&path, "version = 1\nsources = ['base.iwd']\noutput = 'destination'\n[[extract]]\ninclude = ['images/*']\ninto = 'textures'\n").unwrap();
    let (sources, rules, output) = recipe::load(&path).unwrap();
    assert_eq!(sources, vec![f.source]);
    assert_eq!(
        output,
        f.root.path().canonicalize().unwrap().join("destination")
    );
    assert_eq!(rules[0].into, "textures");
}

#[test]
fn recipe_schema_is_strict_and_never_executes_commands() {
    let f = Fixture::new();
    let path = f.root.path().join("bad.toml");
    for text in [
        "version = 2",
        "version = true",
        "version = 1\nsources = 3",
        "version = 1\nsources = ['base.iwd']\noutput = 'out'\n[[extract]]\ninclude = ['*']\nrequired = 'false'",
        "version = 1\nsources = ['base.iwd']\noutput = 'out'\n[[extract]]\ninclude = ['*']\ncommand = 'danger'",
    ] {
        fs::write(&path, text).unwrap();
        assert!(recipe::load(&path).is_err());
    }
}

#[test]
fn cli_list_and_dry_run_and_corruption() {
    let f = Fixture::new();
    let binary = env!("CARGO_BIN_EXE_hell-hound");
    let listing = std::process::Command::new(binary)
        .args(["list", "--source"])
        .arg(&f.source)
        .args(["--match", "images/*colt*", "--json"])
        .output()
        .unwrap();
    assert!(
        listing.status.success(),
        "{}",
        String::from_utf8_lossy(&listing.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&listing.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let extraction = std::process::Command::new(binary)
        .args(["extract", "--source"])
        .arg(&f.source)
        .args(["--match", "*", "--output"])
        .arg(&f.output)
        .arg("--dry-run")
        .output()
        .unwrap();
    assert!(extraction.status.success());
    assert!(!f.output.exists());
    fs::write(&f.source, b"corrupt").unwrap();
    assert!(
        !std::process::Command::new(binary)
            .args(["list", "--source"])
            .arg(&f.source)
            .output()
            .unwrap()
            .status
            .success()
    );
}
