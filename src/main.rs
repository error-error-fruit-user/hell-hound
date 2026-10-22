use anyhow::{Context, Result, ensure};
use clap::{Args, Parser, Subcommand, ValueEnum};
use hell_hound::{
    archive::{self, Manifest, Rule},
    discovery, live, recipe, sound, tui,
    weapon::{self, Problem},
};
use serde::Serialize;
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(
    version,
    about = "Discover running CoD games, browse assets, and export WaW weapons (experimental)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum Kind {
    #[default]
    Any,
    Image,
    Sound,
    File,
    Model,
    Animation,
    /// A loaded sound alias, as weapon files and notetracks name sounds (WaW zombies).
    Alias,
    /// A loaded weapon definition (WaW zombies).
    Weapon,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Image => "image",
            Self::Sound => "sound",
            Self::File => "file",
            Self::Model => "model",
            Self::Animation => "animation",
            Self::Alias => "alias",
            Self::Weapon => "weapon",
        }
    }
    fn live(self) -> bool {
        matches!(
            self,
            Self::Model | Self::Animation | Self::Alias | Self::Weapon
        )
    }
}

#[derive(Args)]
struct Selection {
    /// Advanced override: archive path/glob; normally sources are discovered from the game.
    #[arg(long)]
    source: Vec<String>,
    /// Override the detected installation directory; individual IWD filenames are unnecessary.
    #[arg(long)]
    game_dir: Option<PathBuf>,
    #[command(flatten)]
    game: discovery::Options,
    #[arg(long)]
    name: Vec<String>,
    /// Case-sensitive glob; quote it to prevent shell expansion.
    #[arg(long = "match")]
    patterns: Vec<String>,
    #[arg(long)]
    exclude: Vec<String>,
    #[arg(long, value_enum, default_value = "any")]
    kind: Kind,
}
impl Selection {
    fn sources(&self) -> Result<Vec<PathBuf>> {
        if !self.source.is_empty() {
            return archive::sources_from(&self.source, &std::env::current_dir()?);
        }
        if let Some(root) = &self.game_dir {
            return discovery::archives_in(root);
        }
        let game = discovery::choose(&self.game, false)?;
        eprintln!("Using {} (PID {})", game.title, game.pid);
        game.archives()
    }
    fn rule(self, into: String, list: bool) -> Rule {
        let include = if list && self.name.is_empty() && self.patterns.is_empty() {
            vec!["*".into()]
        } else {
            self.patterns
        };
        Rule {
            names: self.name,
            include,
            exclude: self.exclude,
            kind: self.kind.name().into(),
            into,
            required: true,
        }
    }
}

#[derive(Args)]
struct WriteOptions {
    #[arg(long)]
    overwrite: bool,
    #[arg(long)]
    dry_run: bool,
    /// Require the same names, paths, source archive hashes and extracted bytes.
    #[arg(long)]
    lock: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Open the interface on the running games (also what no command does).
    Open {
        #[command(flatten)]
        game: discovery::Options,
        #[arg(long)]
        game_dir: Option<PathBuf>,
    },
    /// Decode selected WaW model(s), skin and animation clips into a Blender-importable GLB.
    #[command(alias = "export-weapon")]
    Weapon {
        #[command(flatten)]
        game: discovery::Options,
        /// Repeat for weapon and hands; shared bind poses must agree.
        #[arg(long, required = true)]
        model: Vec<String>,
        #[arg(long)]
        animation: Vec<String>,
        #[arg(long = "anim-match")]
        animation_patterns: Vec<String>,
        #[arg(long, default_value = "output/weapon.glb")]
        output: PathBuf,
        #[arg(long)]
        game_dir: Option<PathBuf>,
        #[arg(long,value_parser=parse_address)]
        image_base: Option<u64>,
        #[arg(long)]
        no_textures: bool,
        /// Explicitly omit animation bones absent from the selected models.
        #[arg(long)]
        allow_missing_bones: bool,
        #[arg(long)]
        overwrite: bool,
    },
    List {
        #[command(flatten)]
        selection: Selection,
        #[arg(long)]
        json: bool,
        #[arg(long,default_value_t=archive::DEFAULT_LIMIT)]
        max_bytes: u64,
    },
    Extract {
        #[command(flatten)]
        selection: Selection,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "")]
        into: String,
        #[command(flatten)]
        write: WriteOptions,
        #[arg(long,default_value_t=archive::DEFAULT_LIMIT)]
        max_bytes: u64,
    },
    Run {
        recipe: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        write: WriteOptions,
        #[arg(long,default_value_t=archive::DEFAULT_LIMIT)]
        max_bytes: u64,
    },
}

fn parse_address(value: &str) -> std::result::Result<u64, String> {
    let result = if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16)
    } else {
        value.parse()
    };
    result.map_err(|error| error.to_string())
}

fn write(
    sources: Vec<PathBuf>,
    rules: Vec<Rule>,
    output: PathBuf,
    options: WriteOptions,
    limit: u64,
) -> Result<()> {
    let plan = archive::select(&archive::catalog(&sources)?, &rules, limit)?;
    let lock: Option<Manifest> = options
        .lock
        .as_ref()
        .map(|p| -> Result<_> {
            Ok(serde_json::from_slice(&fs::read(p).with_context(
                || format!("Cannot read lock {}", p.display()),
            )?)?)
        })
        .transpose()?;
    let manifest = archive::extract(
        &plan,
        &output,
        limit,
        options.overwrite,
        options.dry_run,
        lock.as_ref(),
    )?;
    for record in &manifest.assets {
        println!("{} -> {}", record.name, record.path);
    }
    println!(
        "{} {} assets to {}",
        if options.dry_run {
            "Would extract"
        } else {
            "Extracted"
        },
        plan.len(),
        output.display()
    );
    Ok(())
}

struct LiveQuery<'a> {
    names: &'a [String],
    patterns: &'a [String],
    exclude: &'a [String],
    kind: Option<&'a str>,
    json: bool,
}

fn live_list(game: &discovery::Options, query: LiveQuery<'_>) -> Result<()> {
    let LiveQuery {
        names,
        patterns,
        exclude,
        kind,
        json,
    } = query;
    let selected = discovery::choose(game, false)?;
    let patterns: Vec<_> = patterns
        .iter()
        .map(|p| glob::Pattern::new(p))
        .collect::<std::result::Result<_, _>>()?;
    let excluded: Vec<_> = exclude
        .iter()
        .map(|p| glob::Pattern::new(p))
        .collect::<std::result::Result<_, _>>()?;
    let memory = live::Process::open(selected.pid)?;
    let all = live::list(
        &memory,
        selected.waw_mode()?,
        selected.image_base.unwrap_or(0x400000),
    )?;
    let rows: Vec<_> = all
        .into_iter()
        .filter(|asset| {
            let matches = |p: &glob::Pattern| p.matches(&asset.name);
            ((names.is_empty() && patterns.is_empty())
                || names.contains(&asset.name)
                || patterns.iter().any(matches))
                && !excluded.iter().any(matches)
                && kind.is_none_or(|k| asset.kind == k)
        })
        .collect();
    for name in names {
        ensure!(
            rows.iter().any(|a| &a.name == name),
            "Loaded asset not found: {name}"
        );
    }
    ensure!(!rows.is_empty(), "No loaded assets matched the selection");
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for asset in rows {
            println!(
                "{}\t{}\t{:#x}\tbones={}\tframes={:?}\tfps={:?}",
                asset.kind, asset.name, asset.address, asset.bones, asset.frames, asset.fps
            );
        }
    }
    Ok(())
}

/// Whether `name` is selected by exact names and globs (everything when there are neither),
/// and not excluded.
struct NameFilter {
    names: Vec<String>,
    patterns: Vec<glob::Pattern>,
    excluded: Vec<glob::Pattern>,
}
impl NameFilter {
    fn new(selection: &Selection) -> Result<Self> {
        let compile = |patterns: &[String]| -> Result<Vec<glob::Pattern>> {
            Ok(patterns
                .iter()
                .map(|p| glob::Pattern::new(p))
                .collect::<std::result::Result<_, _>>()?)
        };
        Ok(Self {
            names: selection.name.clone(),
            patterns: compile(&selection.patterns)?,
            excluded: compile(&selection.exclude)?,
        })
    }
    fn matches(&self, name: &str) -> bool {
        ((self.names.is_empty() && self.patterns.is_empty())
            || self.names.iter().any(|n| n == name)
            || self.patterns.iter().any(|p| p.matches(name)))
            && !self.excluded.iter().any(|p| p.matches(name))
    }
    /// Fails unless every exact name and something at all was found.
    fn check<'a>(&self, found: impl Iterator<Item = &'a str> + Clone, kind: &str) -> Result<()> {
        for name in &self.names {
            ensure!(
                found.clone().any(|f| f == name),
                "Loaded {kind} not found: {name}"
            );
        }
        ensure!(found.count() > 0, "No loaded {kind}s matched the selection");
        Ok(())
    }
}

/// The selected live sound aliases or weapon definitions of the chosen game.
fn live_aliases(selection: &Selection) -> Result<(live::Process, Vec<sound::Alias>)> {
    let selected = discovery::choose(&selection.game, false)?;
    let memory = live::Process::open(selected.pid)?;
    let filter = NameFilter::new(selection)?;
    let aliases: Vec<_> = sound::aliases(
        &memory,
        selected.waw_mode()?,
        selected.image_base.unwrap_or(0x400000),
    )?
    .into_iter()
    .filter(|a| filter.matches(&a.name))
    .collect();
    filter.check(aliases.iter().map(|a| a.name.as_str()), "sound alias")?;
    Ok((memory, aliases))
}

fn live_weapons(selection: &Selection) -> Result<Vec<weapon::def::WeaponDef>> {
    let selected = discovery::choose(&selection.game, false)?;
    let memory = live::Process::open(selected.pid)?;
    let filter = NameFilter::new(selection)?;
    let weapons: Vec<_> = weapon::def::list(
        &memory,
        selected.waw_mode()?,
        selected.image_base.unwrap_or(0x400000),
    )?
    .into_iter()
    .filter(|w| filter.matches(&w.name))
    .collect();
    filter.check(weapons.iter().map(|w| w.name.as_str()), "weapon")?;
    Ok(weapons)
}

/// Writes each selected alias's sounds as `<alias>.wav`, or `<alias>_<n>.wav` for an alias with
/// several variants (`.xwm` for xWMA ones); streamed ones come from the IWDs.
fn extract_aliases(
    selection: &Selection,
    output: &std::path::Path,
    options: &WriteOptions,
) -> Result<()> {
    let (memory, aliases) = live_aliases(selection)?;
    let archives = archive::catalog(&selection.sources()?)?;
    let mut written = 0;
    for alias in &aliases {
        for (index, file) in alias.files.iter().enumerate() {
            let bytes = sound::read(&memory, file, &archives)
                .with_context(|| format!("Sound alias {}", alias.name))?;
            let extension = sound::extension(&bytes).unwrap_or("wav");
            let name = if alias.files.len() == 1 {
                format!("{}.{extension}", alias.name)
            } else {
                format!("{}_{index}.{extension}", alias.name)
            };
            let path = output.join(&name);
            if !options.dry_run {
                ensure!(
                    options.overwrite || !path.exists(),
                    "{} already exists; use --overwrite",
                    path.display()
                );
                archive::atomic_write(&path, &bytes, options.overwrite)?;
            }
            println!("{} ({}) -> {}", alias.name, file.name(), path.display());
            written += 1;
        }
    }
    println!(
        "{} {written} sounds to {}",
        if options.dry_run {
            "Would extract"
        } else {
            "Extracted"
        },
        output.display()
    );
    Ok(())
}

/// The batch commands' answer to an export problem: what their flags allow, or an error saying
/// which flag would.
fn answer(problem: &Problem, allow_missing_bones: bool, overwrite: bool) -> Result<bool> {
    let (allowed, hint) = match problem {
        Problem::MissingBones { .. } => (
            allow_missing_bones,
            "add the hands model, or use --allow-missing-bones",
        ),
        Problem::OutputExists(_) => (overwrite, "choose another file, or use --overwrite"),
        Problem::MissingTexture(_) => (false, "load its IWDs, or use --no-textures"),
        Problem::NoTextureArchives => (false, "supply --game-dir, or use --no-textures"),
    };
    ensure!(allowed, "{problem}; {hint}");
    Ok(true)
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let Some(command) = cli.command else {
        return tui::run(discovery::Options::default(), None);
    };
    match command {
        Command::Open { game, game_dir } => tui::run(game, game_dir)?,
        Command::Weapon {
            game,
            model,
            animation,
            animation_patterns,
            output,
            game_dir,
            image_base,
            no_textures,
            allow_missing_bones,
            overwrite,
        } => {
            let selected = discovery::choose(&game, false)?;
            let mode = selected.waw_mode()?;
            let sources = if no_textures {
                vec![]
            } else if let Some(root) = game_dir {
                discovery::archives_in(&root)?
            } else {
                selected.archives()?
            };
            let memory = live::Process::open(selected.pid)?;
            weapon::export(
                &memory,
                mode,
                image_base.or(selected.image_base).unwrap_or(0x400000),
                &sources,
                &output,
                &weapon::Options {
                    models: model,
                    animations: animation,
                    animation_patterns,
                    no_textures,
                    dry_run: false,
                },
                &mut |problem| answer(problem, allow_missing_bones, overwrite),
            )?;
            println!(
                "Exported {} — import with Blender's glTF 2.0 importer. Experimental live output requires visual verification.",
                output.display()
            );
        }
        Command::List {
            selection,
            json,
            max_bytes,
        } => {
            if matches!(selection.kind, Kind::Alias) {
                let (_, aliases) = live_aliases(&selection)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&aliases)?);
                } else {
                    for alias in aliases {
                        let files: Vec<_> = alias.files.iter().map(|f| f.name()).collect();
                        println!("alias\t{}\t{}", alias.name, files.join(" | "));
                    }
                }
            } else if matches!(selection.kind, Kind::Weapon) {
                let weapons = live_weapons(&selection)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&weapons)?);
                } else {
                    for weapon in weapons {
                        println!(
                            "weapon\t{}\t{}\t{}\t{}\t{}",
                            weapon.name,
                            weapon.display_name,
                            weapon.weapon_type,
                            weapon.fire_type,
                            weapon.gun_model.as_deref().unwrap_or("-")
                        );
                    }
                }
            } else if selection.kind.live() {
                live_list(
                    &selection.game,
                    LiveQuery {
                        names: &selection.name,
                        patterns: &selection.patterns,
                        exclude: &selection.exclude,
                        kind: Some(selection.kind.name()),
                        json,
                    },
                )?;
            } else {
                let sources = selection.sources()?;
                let rule = selection.rule(String::new(), true);
                let plan = archive::select(&archive::catalog(&sources)?, &[rule], max_bytes)?;
                #[derive(Serialize)]
                struct Row {
                    name: String,
                    kind: &'static str,
                    size: u64,
                    source: PathBuf,
                }
                let rows: Vec<_> = plan
                    .into_iter()
                    .map(|p| Row {
                        kind: archive::kind_of(&p.asset.member),
                        name: p.asset.member,
                        size: p.asset.size,
                        source: p.asset.source,
                    })
                    .collect();
                if json {
                    println!("{}", serde_json::to_string_pretty(&rows)?);
                } else {
                    for row in rows {
                        println!(
                            "{}\t{}\t{}\t{}",
                            row.kind,
                            row.size,
                            row.name,
                            row.source.display()
                        );
                    }
                }
            }
        }
        Command::Extract {
            selection,
            output,
            into,
            write: options,
            max_bytes,
        } => {
            if matches!(selection.kind, Kind::Alias) {
                ensure!(
                    into.is_empty() && options.lock.is_none(),
                    "--into and --lock are for archive files, not sound aliases"
                );
                return extract_aliases(&selection, &output, &options);
            }
            ensure!(
                !selection.kind.live(),
                "Use weapon --model NAME --animation NAME --output weapon.glb for decoded model/animation extraction; list --kind weapon --json shows weapon definitions"
            );
            let sources = selection.sources()?;
            write(
                sources,
                vec![selection.rule(into, false)],
                output,
                options,
                max_bytes,
            )?;
        }
        Command::Run {
            recipe: path,
            output,
            write: options,
            max_bytes,
        } => {
            if let Some(job) = recipe::weapon_job(&path)? {
                ensure!(
                    options.lock.is_none(),
                    "Weapon recipes do not yet support archive JSON locks; use exact model/animation names in the recipe"
                );
                let selected = discovery::choose(
                    &discovery::Options {
                        game: Some(job.game),
                        ..Default::default()
                    },
                    false,
                )?;
                let mode = selected.waw_mode()?;
                let sources = if job.weapon.no_textures {
                    vec![]
                } else if let Some(directory) = job.game_dir {
                    discovery::archives_in(&directory)?
                } else {
                    selected.archives()?
                };
                let destination = output.unwrap_or(job.output);
                let memory = live::Process::open(selected.pid)?;
                weapon::export(
                    &memory,
                    mode,
                    selected.image_base.unwrap_or(0x400000),
                    &sources,
                    &destination,
                    &weapon::Options {
                        models: job.weapon.models,
                        animations: job.weapon.animations,
                        animation_patterns: job.weapon.animation_patterns,
                        no_textures: job.weapon.no_textures,
                        dry_run: options.dry_run,
                    },
                    &mut |problem| {
                        answer(problem, job.weapon.allow_missing_bones, options.overwrite)
                    },
                )?;
                println!(
                    "{} {}",
                    if options.dry_run {
                        "Would export"
                    } else {
                        "Exported"
                    },
                    destination.display()
                );
            } else {
                let (sources, rules, destination) = recipe::load(&path)?;
                write(
                    sources,
                    rules,
                    output.unwrap_or(destination),
                    options,
                    max_bytes,
                )?;
            }
        }
    }
    Ok(())
}

fn main() {
    match run() {
        Ok(()) => {}
        // Ctrl+C in the interface.
        Err(error) if error.is::<tui::Interrupted>() => std::process::exit(130),
        Err(error) => {
            eprintln!("hell-hound: {error:#}");
            std::process::exit(1);
        }
    }
}
