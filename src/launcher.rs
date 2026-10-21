//! Launch through Steam's existing app environment; never use system Wine.

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, ValueEnum};
use std::ffi::OsString;
#[cfg(target_os = "linux")]
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Map {
    Nacht,
    Verruckt,
    ShiNoNuma,
    DerRiese,
}

impl Map {
    pub const ALL: [Self; 4] = [Self::Nacht, Self::Verruckt, Self::ShiNoNuma, Self::DerRiese];

    pub fn name(self) -> &'static str {
        match self {
            Self::Nacht => "nacht",
            Self::Verruckt => "verruckt",
            Self::ShiNoNuma => "shi-no-numa",
            Self::DerRiese => "der-riese",
        }
    }

    pub fn internal(self) -> &'static str {
        match self {
            Self::Nacht => "nazi_zombie_prototype",
            Self::Verruckt => "nazi_zombie_asylum",
            Self::ShiNoNuma => "nazi_zombie_sumpf",
            Self::DerRiese => "nazi_zombie_factory",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    about = "Launch stock WaW solo zombies using Steam's existing setup",
    after_help = "Close WaW first and remove older devmap/connect commands from Steam launch options.\n\
     Startup wait/disconnect is a candidate fix for an already-connected menu bounce, not a verified Proton fix.\n\
     Delays count engine frames, not seconds. God is a toggle: do not retry automatically.\n\
     Game-managed windowed/console settings may persist in your active profile."
)]
pub struct Options {
    #[arg(long, value_enum, default_value = "der-riese")]
    pub map: Map,
    #[arg(long)]
    pub list_maps: bool,
    #[arg(long, conflicts_with_all = ["native", "steam"])]
    pub flatpak: bool,
    /// Native Steam (default).
    #[arg(long)]
    pub native: bool,
    /// Native Steam executable path; useful on Windows.
    #[arg(long)]
    pub steam: Option<PathBuf>,
    #[arg(long)]
    pub keep_display: bool,
    /// Frames before disconnect/devmap, not seconds.
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u32).range(1..=999999))]
    pub startup_frames: u32,
    /// Frames after devmap before the single god request.
    #[arg(long, alias = "god-delay", default_value_t = 300, value_parser = clap::value_parser!(u32).range(1..=999999))]
    pub wait_frames: u32,
    /// Isolate map loading without requesting god mode.
    #[arg(long)]
    pub no_god: bool,
    /// Print a JSON argument array; do not launch or change files.
    #[arg(long)]
    pub dry_run: bool,
}

pub fn arguments(options: &Options) -> Vec<String> {
    let mut args: Vec<String> = [
        "-applaunch",
        "10090",
        "+set",
        "monkeytoy",
        "0",
        "+set",
        "logfile",
        "2",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    if !options.keep_display {
        args.extend(["+set", "r_fullscreen", "0"].map(String::from));
    }
    // Let startup/menu networking settle, then explicitly tear down the local
    // client before creating exactly one solo host. No key presses or injection.
    args.extend([
        "+wait".into(),
        options.startup_frames.to_string(),
        "+disconnect".into(),
        "+wait".into(),
        "60".into(),
        "+devmap".into(),
        options.map.internal().into(),
    ]);
    if !options.no_god {
        args.extend([
            "+wait".into(),
            options.wait_frames.to_string(),
            "+god".into(),
        ]);
    }
    args
}

pub fn game_running() -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        for entry in fs::read_dir("/proc")? {
            let entry = entry?;
            if !entry
                .file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
            {
                continue;
            }
            if let Ok(name) = fs::read_to_string(entry.path().join("comm")) {
                if ["codwaw.exe", "codwawmp.exe"]
                    .contains(&name.trim().to_ascii_lowercase().as_str())
                {
                    return Ok(true);
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .output()?;
        ensure!(
            output.status.success(),
            "Cannot inspect running games with tasklist"
        );
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let name = line
                .split(',')
                .next()
                .unwrap_or("")
                .trim_matches('"')
                .to_ascii_lowercase();
            if ["codwaw.exe", "codwawmp.exe"].contains(&name.as_str()) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn native_steam(explicit: Option<&Path>, dry_run: bool) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.into());
    }
    #[cfg(target_os = "windows")]
    {
        if dry_run {
            return Ok("steam.exe".into());
        }
        if let Ok(output) = Command::new("reg")
            .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamExe"])
            .output()
        {
            if output.status.success() {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if let Some((_, value)) = line.split_once("REG_SZ") {
                        let path = PathBuf::from(value.trim());
                        if path.is_file() {
                            return Ok(path);
                        }
                    }
                }
            }
        }
        for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(root) = std::env::var_os(variable) {
                let path = PathBuf::from(root).join("Steam/steam.exe");
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
        return Ok("steam.exe".into());
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = dry_run;
        Ok("steam".into())
    }
}

pub fn run(options: Options) -> Result<i32> {
    if options.list_maps {
        for map in Map::ALL {
            println!("{}\t{}", map.name(), map.internal());
        }
        return Ok(0);
    }
    if options.flatpak && cfg!(target_os = "windows") {
        bail!("Flatpak Steam is only available on Linux");
    }
    if !options.dry_run {
        ensure!(
            !game_running()?,
            "WaW is already running. Exit it before launching another solo host"
        );
    }
    let mut command: Vec<OsString> = if options.flatpak {
        ["flatpak", "run", "com.valvesoftware.Steam"]
            .map(OsString::from)
            .to_vec()
    } else {
        vec![native_steam(options.steam.as_deref(), options.dry_run)?.into_os_string()]
    };
    command.extend(arguments(&options).into_iter().map(OsString::from));
    let printable: Vec<_> = command.iter().map(|a| a.to_string_lossy()).collect();
    println!("{}", serde_json::to_string(&printable)?);
    if options.dry_run {
        return Ok(0);
    }
    eprintln!("If Steam asks, select single-player. Do not start another host from the menu.");
    eprintln!(
        "God is requested once, not verified. If loading bounces again, keep the exact error and console.log."
    );
    let status = Command::new(&command[0])
        .args(&command[1..])
        .status()
        .context("Cannot launch Steam; check --steam PATH or --flatpak")?;
    Ok(status.code().unwrap_or(1))
}
