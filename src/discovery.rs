//! Read-only game discovery. A recognized process is not a promise of a decoder.

use crate::live::Mode;
use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use serde::Serialize;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Game {
    pub pid: u32,
    pub id: String,
    pub title: String,
    pub executable: String,
    pub directory: Option<PathBuf>,
    pub image_base: Option<u64>,
    pub live_inspection: bool,
}

#[derive(Debug, Clone, Default, Args)]
pub struct Options {
    /// Select a detected game by stable ID, e.g. waw, cod4, mw2, black-ops.
    #[arg(long)]
    pub game: Option<String>,
    /// Select the OS game PID; normally the interactive picker handles this.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub pid: Option<u32>,
    /// Wait for a matching game to start (Ctrl+C cancels).
    #[arg(long, alias = "watch")]
    pub wait: bool,
    /// Maximum wait in seconds; 0 means no timeout. Implies --wait.
    #[arg(long, default_value_t = 0)]
    pub timeout: u64,
}

pub fn identify(executable: &str) -> Option<(&'static str, &'static str)> {
    let basename = executable
        .rsplit(['/', '\\'])
        .next()?
        .trim_matches('"')
        .to_ascii_lowercase();
    match basename.as_str() {
        "codwaw.exe" | "codwawmp.exe" => Some(("waw", "World at War")),
        "codsp.exe" | "codmp.exe" | "coduosp.exe" | "coduomp.exe" => {
            Some(("cod", "Call of Duty / United Offensive"))
        }
        "cod2sp_s.exe" | "cod2mp_s.exe" | "cod2sp.exe" | "cod2mp.exe" => {
            Some(("cod2", "Call of Duty 2"))
        }
        "iw3sp.exe" | "iw3mp.exe" => Some(("cod4", "Call of Duty 4: Modern Warfare")),
        "iw4sp.exe" | "iw4mp.exe" => Some(("mw2", "Modern Warfare 2")),
        "iw5sp.exe" | "iw5mp.exe" => Some(("mw3", "Modern Warfare 3")),
        "blackops.exe" | "blackopsmp.exe" => Some(("black-ops", "Black Ops")),
        "t6sp.exe" | "t6mp.exe" | "t6zm.exe" => Some(("black-ops-2", "Black Ops II")),
        _ => None,
    }
}

pub fn known_id(id: &str) -> bool {
    [
        "waw",
        "cod",
        "cod2",
        "cod4",
        "mw2",
        "mw3",
        "black-ops",
        "black-ops-2",
    ]
    .contains(&id)
}

impl Game {
    pub fn waw_mode(&self) -> Result<Mode> {
        ensure!(
            self.id == "waw",
            "{} is detected, but its live model/animation decoder is not implemented. Archive extraction is available only if IWDs exist",
            self.title
        );
        if self.executable.eq_ignore_ascii_case("codwawmp.exe") {
            Ok(Mode::Mp)
        } else {
            Ok(Mode::Sp)
        }
    }

    pub fn archives(&self) -> Result<Vec<PathBuf>> {
        let root = self.directory.as_ref().context("Could not discover the game directory. Supply --game-dir DIRECTORY (not an IWD filename), or use advanced --source")?;
        archives_in(root)
    }

    pub fn capability(&self) -> &'static str {
        if self.live_inspection {
            "IWD extraction + experimental skinned/animated GLB export (real-game verification pending)"
        } else {
            "detection only; IWD extraction if present, no live decoder"
        }
    }
}

pub fn archives_in(root: &Path) -> Result<Vec<PathBuf>> {
    let mut result = vec![];
    // Main carries stock IWDs. The root fallback handles older/simple layouts;
    // never recursively scan drives, guess mod order, or silently treat FF as ZIP.
    for directory in [root.to_path_buf(), root.join("main")] {
        if !directory.is_dir() {
            continue;
        }
        let mut paths = vec![];
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("iwd"))
            {
                paths.push(path.canonicalize()?);
            }
        }
        paths.sort();
        for path in paths {
            if !result.contains(&path) {
                result.push(path);
            }
        }
    }
    ensure!(
        !result.is_empty(),
        "No IWD archives found in {} or main/. This game's other package formats are not implemented",
        root.display()
    );
    Ok(result)
}

fn make_game(
    pid: u32,
    executable: &str,
    directory: Option<PathBuf>,
    image_base: Option<u64>,
) -> Option<Game> {
    let (id, title) = identify(executable)?;
    Some(Game {
        pid,
        id: id.into(),
        title: title.into(),
        executable: executable.into(),
        directory,
        image_base,
        live_inspection: id == "waw",
    })
}

/// Only convert a Wine Z: mapping, which has an unambiguous Unix equivalent.
/// Other drive letters require prefix mappings and must not be guessed.
pub fn unix_executable(argument: &str) -> Option<PathBuf> {
    let value = argument.trim_matches('"');
    if value.starts_with('/') {
        Some(PathBuf::from(value))
    } else if value
        .get(..3)
        .is_some_and(|p| p.eq_ignore_ascii_case("z:\\") || p.eq_ignore_ascii_case("z:/"))
    {
        Some(PathBuf::from(format!("/{}", value[3..].replace('\\', "/"))))
    } else {
        None
    }
}

/// Parse proc maps without losing spaces in filesystem paths.
pub fn mapped_executable(line: &str, executable: &str) -> Option<(PathBuf, u64)> {
    let mut rest = line;
    let mut fields = vec![];
    for _ in 0..5 {
        rest = rest.trim_start();
        let end = rest.find(char::is_whitespace)?;
        fields.push(&rest[..end]);
        rest = &rest[end..];
    }
    let path = PathBuf::from(
        rest.trim_start()
            .strip_suffix(" (deleted)")
            .unwrap_or(rest.trim_start()),
    );
    if !path.file_name()?.to_str()?.eq_ignore_ascii_case(executable) {
        return None;
    }
    if !path.is_absolute() {
        return None;
    }
    let start = u64::from_str_radix(fields[0].split('-').next()?, 16).ok()?;
    let offset = u64::from_str_radix(fields[2], 16).ok()?;
    Some((path, start.checked_sub(offset)?))
}

#[cfg(target_os = "linux")]
pub fn discover() -> Result<Vec<Game>> {
    discover_from_proc(Path::new("/proc"))
}

/// Injectable proc root for deterministic discovery tests; normal discovery uses /proc.
#[cfg(target_os = "linux")]
pub fn discover_from_proc(root: &Path) -> Result<Vec<Game>> {
    let mut result = vec![];
    for entry in fs::read_dir(root)? {
        let Ok(entry) = entry else {
            continue;
        };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(comm) = fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        let comm = comm.trim();
        let recognized = identify(comm).is_some();
        let wine =
            comm.to_ascii_lowercase().contains("wine") || comm.eq_ignore_ascii_case("mainthrd");
        if !recognized && !wine {
            continue;
        }
        let commandline = fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let args: Vec<_> = commandline
            .split(|b| *b == 0)
            .filter_map(|s| std::str::from_utf8(s).ok())
            .collect();
        let executable_argument = args.iter().find(|s| identify(s).is_some()).copied();
        let executable = if recognized {
            comm.to_owned()
        } else if let Some(arg) = executable_argument {
            arg.rsplit(['/', '\\']).next().unwrap_or(arg).to_owned()
        } else {
            continue;
        };
        let mut directory = executable_argument
            .and_then(unix_executable)
            .filter(|p| p.is_file())
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let mut image_base = None;
        if let Ok(maps) = fs::read_to_string(entry.path().join("maps")) {
            for line in maps.lines() {
                if let Some((path, base)) = mapped_executable(line, &executable) {
                    if path.is_file() {
                        directory = path.parent().map(Path::to_path_buf);
                    }
                    image_base = Some(image_base.map_or(base, |old: u64| old.min(base)));
                }
            }
        }
        if directory.is_none()
            && let Ok(cwd) = fs::read_link(entry.path().join("cwd"))
            && (cwd.join(&executable).is_file() || cwd.join("main").is_dir())
        {
            directory = Some(cwd);
        }
        if let Some(game) = make_game(pid, &executable, directory, image_base) {
            result.push(game);
        }
    }
    result.sort_by_key(|game| game.pid);
    Ok(result)
}

#[cfg(target_os = "windows")]
pub fn discover() -> Result<Vec<Game>> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    };
    struct Handle(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    // SAFETY: native process enumeration only; each successful handle is owned.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    ensure!(
        raw != INVALID_HANDLE_VALUE,
        "Cannot enumerate processes: {}",
        std::io::Error::last_os_error()
    );
    let snapshot = Handle(raw);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut valid = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
    let mut result = vec![];
    while valid {
        let length = entry
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(entry.szExeFile.len());
        let executable = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if identify(&executable).is_some() {
            let mut directory = None;
            let raw =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
            if !raw.is_null() {
                let process = Handle(raw);
                let mut buffer = vec![0u16; 32768];
                let mut length = buffer.len() as u32;
                if unsafe {
                    QueryFullProcessImageNameW(process.0, 0, buffer.as_mut_ptr(), &mut length)
                } != 0
                {
                    let path =
                        PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]));
                    directory = path.parent().map(Path::to_path_buf);
                }
            }
            if let Some(game) = make_game(entry.th32ProcessID, &executable, directory, None) {
                result.push(game);
            }
        }
        valid = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
    }
    result.sort_by_key(|game| game.pid);
    Ok(result)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn discover() -> Result<Vec<Game>> {
    bail!("Game discovery supports Windows and Linux")
}

pub fn matching(games: Vec<Game>, options: &Options) -> Vec<Game> {
    games
        .into_iter()
        .filter(|game| {
            options.pid.is_none_or(|pid| pid == game.pid)
                && options.game.as_ref().is_none_or(|id| id == &game.id)
        })
        .collect()
}

pub fn validate_options(options: &Options) -> Result<()> {
    if let Some(id) = &options.game {
        ensure!(
            known_id(id),
            "Unknown game ID {id:?}; open the interface to see detected games"
        );
    }
    Ok(())
}

pub fn wait_for_games(options: &Options, wait: bool) -> Result<Vec<Game>> {
    validate_options(options)?;
    let started = Instant::now();
    let mut announced = false;
    loop {
        let games = matching(discover()?, options);
        if !games.is_empty() {
            return Ok(games);
        }
        if !(wait || options.wait || options.timeout > 0) {
            bail!(
                "No matching CoD game is running. Start one, use --wait, or supply --game-dir DIRECTORY"
            );
        }
        if options.timeout > 0 && started.elapsed() >= Duration::from_secs(options.timeout) {
            bail!("Timed out waiting for a matching CoD game");
        }
        if !announced {
            eprintln!("Waiting for a CoD game to start (Ctrl+C cancels)...");
            announced = true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

pub fn choose_index(input: &str, count: usize) -> Result<usize> {
    let index: usize = input
        .trim()
        .parse()
        .context("Enter a game number, r to refresh or q to quit")?;
    ensure!(
        (1..=count).contains(&index),
        "Select a number between 1 and {count}"
    );
    Ok(index - 1)
}

pub fn choose(options: &Options, wait: bool) -> Result<Game> {
    loop {
        let games = wait_for_games(options, wait)?;
        if options.pid.is_some() || (!io::stdin().is_terminal() && games.len() == 1) {
            return Ok(games[0].clone());
        }
        ensure!(
            io::stdin().is_terminal(),
            "Multiple games detected; use --pid PID or --game ID to choose without a terminal"
        );
        eprintln!("Running games:");
        for (index, game) in games.iter().enumerate() {
            eprintln!(
                "  {}. {} ({}, PID {}) — {}",
                index + 1,
                game.title,
                game.executable,
                game.pid,
                game.capability()
            );
        }
        eprint!(
            "Open game [1-{}, Enter=1, r=refresh, q=quit]: ",
            games.len()
        );
        io::stderr().flush()?;
        let mut line = String::new();
        ensure!(
            io::stdin().read_line(&mut line)? > 0,
            "Input closed; no game opened"
        );
        match line.trim() {
            "q" | "quit" => bail!("Cancelled; no game opened"),
            "r" | "refresh" => continue,
            "" => return Ok(games[0].clone()),
            _ => match choose_index(&line, games.len()) {
                Ok(index) => return Ok(games[index].clone()),
                Err(error) => eprintln!("{error}"),
            },
        }
    }
}

pub fn current(game: &Game) -> Result<Game> {
    discover()?
        .into_iter()
        .find(|found| {
            found.pid == game.pid && found.executable.eq_ignore_ascii_case(&game.executable)
        })
        .context("Selected game exited or changed. Refresh the game picker before attaching again")
}
