//! Small terminal workflow, not a GUI/TUI framework; batch commands remain usable.
use crate::{
    archive::{self, Rule},
    discovery::{self, Game, Options},
    live, weapon,
};
use anyhow::{Context, Result, ensure};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

fn ask(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    io::stderr().flush()?;
    let mut line = String::new();
    ensure!(
        io::stdin().read_line(&mut line)? > 0,
        "Input closed; session cancelled"
    );
    Ok(line.trim().to_owned())
}

pub fn pattern(query: &str) -> String {
    if query.contains(['*', '?', '[']) {
        query.to_owned()
    } else {
        format!("*{query}*")
    }
}

pub fn indices(value: &str, count: usize) -> Result<Vec<usize>> {
    if value.trim() == "all" {
        return Ok((0..count).collect());
    }
    ensure!(!value.trim().is_empty(), "No assets selected");
    let mut result = std::collections::BTreeSet::new();
    for item in value.split(',') {
        let item = item.trim();
        if let Some((start, end)) = item.split_once('-') {
            let start: usize = start.trim().parse().context("Invalid range start")?;
            let end: usize = end.trim().parse().context("Invalid range end")?;
            ensure!(
                start >= 1 && start <= end && end <= count,
                "Asset range outside 1..={count}"
            );
            result.extend(start - 1..end);
        } else {
            let number: usize = item.parse().context("Use numbers, ranges (1-3), or all")?;
            ensure!(
                (1..=count).contains(&number),
                "Asset number outside 1..={count}"
            );
            result.insert(number - 1);
        }
    }
    Ok(result.into_iter().collect())
}

fn inspect(game: &Game, kind: &str) -> Result<()> {
    let game = discovery::current(game)?;
    let query = ask("Name substring or glob (Enter=all): ")?;
    let filter = glob::Pattern::new(&pattern(&query))?;
    let memory = live::Process::open(game.pid)?;
    let assets = live::list(
        &memory,
        game.waw_mode()?,
        game.image_base.unwrap_or(0x400000),
    )?;
    let mut count = 0;
    for asset in assets
        .iter()
        .filter(|a| a.kind == kind && filter.matches(&a.name))
    {
        count += 1;
        eprintln!(
            "  {count}. {} (bones={}, frames={:?}, fps={:?})",
            asset.name, asset.bones, asset.frames, asset.fps
        );
    }
    eprintln!(
        "{count} {kind} assets. Use 'Export weapon' to decode selected models/animations into GLB."
    );
    Ok(())
}

fn archive_browser(game: &Game, kind: &str) -> Result<()> {
    let sources = game.archives()?;
    let query = ask("Name substring or glob (Enter=all): ")?;
    let rule = Rule {
        include: vec![pattern(&query)],
        kind: kind.into(),
        ..Rule::default()
    };
    let plan = archive::select(
        &archive::catalog(&sources)?,
        &[rule],
        archive::DEFAULT_LIMIT,
    )?;
    for (index, entry) in plan.iter().enumerate() {
        eprintln!(
            "  {}. {} ({} bytes)",
            index + 1,
            entry.asset.member,
            entry.asset.size
        );
    }
    let answer = ask("Extract numbers/ranges (e.g. 1,3-5), all, or Enter to go back: ")?;
    if answer.is_empty() {
        return Ok(());
    }
    let selected = indices(&answer, plan.len())?;
    let default = format!("output/{}/{}", game.id, kind);
    let destination = ask(&format!("Output folder [{default}]: "))?;
    let destination = PathBuf::from(if destination.is_empty() {
        default
    } else {
        destination
    });
    let selected: Vec<_> = selected.into_iter().map(|i| plan[i].clone()).collect();
    archive::extract(
        &selected,
        &destination,
        archive::DEFAULT_LIMIT,
        false,
        false,
        None,
    )?;
    eprintln!(
        "Extracted {} assets into {}",
        selected.len(),
        destination.display()
    );
    Ok(())
}

fn pick_assets(assets: &[live::LiveAsset], kind: &str, query: &str) -> Result<Vec<String>> {
    let filter = glob::Pattern::new(&pattern(query))?;
    let found: Vec<_> = assets
        .iter()
        .filter(|a| a.kind == kind && filter.matches(&a.name))
        .collect();
    ensure!(
        !found.is_empty(),
        "No loaded {kind} names matched {query:?}"
    );
    for (index, asset) in found.iter().enumerate() {
        eprintln!(
            "  {}. {} (bones={}, frames={:?}, fps={:?})",
            index + 1,
            asset.name,
            asset.bones,
            asset.frames,
            asset.fps
        );
    }
    let answer = ask("Select numbers/ranges, all, or Enter to cancel: ")?;
    if answer.is_empty() {
        return Ok(vec![]);
    }
    Ok(indices(&answer, found.len())?
        .into_iter()
        .map(|i| found[i].name.clone())
        .collect())
}

fn export_browser(game: &Game) -> Result<()> {
    eprintln!(
        "Experimental WaW export. Search and select the weapon model; add hands if the clips need their bones."
    );
    discovery::current(game)?;
    let mode = game.waw_mode()?;
    let memory = live::Process::open(game.pid)?;
    let assets = live::list(&memory, mode, game.image_base.unwrap_or(0x400000))?;
    let mut models = vec![];
    loop {
        let query = ask("Model name substring/glob (e.g. colt or *hands*): ")?;
        for name in pick_assets(&assets, "model", &query)? {
            if !models.contains(&name) {
                models.push(name);
            }
        }
        if !ask("Search/add another model (e.g. hands)? [y/N]: ")?.eq_ignore_ascii_case("y") {
            break;
        }
    }
    if models.is_empty() {
        return Ok(());
    }
    let query = ask("Animation name substring/glob (Enter=model only): ")?;
    let animations = if query.is_empty() {
        vec![]
    } else {
        pick_assets(&assets, "animation", &query)?
    };
    let default = format!("output/{}/weapon.glb", game.id);
    let output = ask(&format!("Output GLB [{default}]: "))?;
    let no_textures = ask("Skip textures? [y/N]: ")?.eq_ignore_ascii_case("y");
    discovery::current(game)?;
    let sources = if no_textures {
        vec![]
    } else {
        game.archives()?
    };
    let options = weapon::Options {
        models,
        animations,
        no_textures,
        ..weapon::Options::default()
    };
    let output = PathBuf::from(if output.is_empty() { default } else { output });
    weapon::export(
        &memory,
        mode,
        game.image_base.unwrap_or(0x400000),
        &sources,
        &output,
        &options,
    )?;
    eprintln!(
        "Wrote {}. In Blender use File → Import → glTF 2.0. Live output still needs visual validation.",
        output.display()
    );
    Ok(())
}

pub fn open(options: &Options, game_dir: Option<PathBuf>) -> Result<()> {
    ensure!(
        io::stdin().is_terminal(),
        "Interactive mode requires a terminal. Use games, list/extract --game ID, or weapon for batch use"
    );
    loop {
        let mut game = discovery::choose(options, true)?;
        if let Some(directory) = &game_dir {
            game.directory = Some(directory.canonicalize().context("Cannot open --game-dir")?);
        }
        eprintln!(
            "Opened {} (PID {}). {}",
            game.title,
            game.pid,
            game.capability()
        );
        if let Some(root) = &game.directory {
            eprintln!("Game directory: {}", root.display());
        }
        loop {
            eprintln!(
                "\n1 Images/textures   2 Sounds   3 Other archive files\n4 Loaded models    5 Loaded animations\n6 Export weapon GLB (experimental)\nr Choose/refresh game   q Quit"
            );
            let action = ask("Action: ")?;
            let result = match action.as_str() {
                "1" => archive_browser(&game, "image"),
                "2" => archive_browser(&game, "sound"),
                "3" => archive_browser(&game, "file"),
                "4" => inspect(&game, "model"),
                "5" => inspect(&game, "animation"),
                "6" => export_browser(&game),
                "r" => break,
                "q" | "quit" => return Ok(()),
                _ => {
                    eprintln!("Choose one of the listed actions");
                    continue;
                }
            };
            if let Err(error) = result {
                eprintln!("{error:#}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selections_are_bounded_and_deduplicated() {
        assert_eq!(indices("1,3-5,3", 5).unwrap(), vec![0, 2, 3, 4]);
        assert_eq!(indices("all", 3).unwrap(), vec![0, 1, 2]);
        for input in ["0", "2-1", "1-99", "6", "", "-1"] {
            assert!(indices(input, 5).is_err());
        }
    }
    #[test]
    fn plain_queries_are_substrings_but_globs_stay_globs() {
        assert_eq!(pattern("colt"), "*colt*");
        assert_eq!(pattern("images/*colt*"), "images/*colt*");
    }
}
