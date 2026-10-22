use anyhow::{Context, Result, ensure};
use clap::{Args, Parser, Subcommand, ValueEnum};
use hell_hound::{
    archive::{self, Manifest, Rule},
    discovery, live, recipe, tui,
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
        }
    }
    fn live(self) -> bool {
        matches!(self, Self::Model | Self::Animation)
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
    /// List detected game processes; --wait listens until a matching game starts.
    Games {
        #[command(flatten)]
        game: discovery::Options,
        #[arg(long)]
        json: bool,
    },
    /// Inspect loaded WaW model/animation names; choose the game automatically.
    LiveList {
        #[command(flatten)]
        game: discovery::Options,
        #[arg(long, value_enum)]
        mode: Option<live::Mode>,
        #[arg(long,value_parser=parse_address)]
        image_base: Option<u64>,
        #[arg(long)]
        name: Vec<String>,
        #[arg(long = "match")]
        patterns: Vec<String>,
        #[arg(long, value_enum)]
        kind: Option<LiveKind>,
        #[arg(long)]
        json: bool,
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

#[derive(Clone, Copy, ValueEnum)]
enum LiveKind {
    Model,
    Animation,
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
    mode: Option<live::Mode>,
    base: Option<u64>,
    names: &'a [String],
    patterns: &'a [String],
    exclude: &'a [String],
    kind: Option<&'a str>,
    json: bool,
}

fn live_list(game: &discovery::Options, query: LiveQuery<'_>) -> Result<()> {
    let LiveQuery {
        mode,
        base,
        names,
        patterns,
        exclude,
        kind,
        json,
    } = query;
    let selected = discovery::choose(game, false)?;
    let inferred = selected.waw_mode()?;
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
        mode.unwrap_or(inferred),
        base.or(selected.image_base).unwrap_or(0x400000),
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
        Command::Games { game, json } => {
            discovery::validate_options(&game)?;
            let games = if game.wait || game.timeout > 0 {
                discovery::wait_for_games(&game, true)?
            } else {
                discovery::matching(discovery::discover()?, &game)
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&games)?);
            } else {
                for found in &games {
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        found.pid,
                        found.id,
                        found.executable,
                        found.directory.as_ref().map_or_else(
                            || "directory unknown".into(),
                            |p| p.display().to_string()
                        ),
                        found.capability()
                    );
                }
                if games.is_empty() {
                    eprintln!(
                        "No supported-name CoD processes detected. Start a game, or use games --wait."
                    );
                }
            }
        }
        Command::LiveList {
            game,
            mode,
            image_base,
            name,
            patterns,
            kind,
            json,
        } => live_list(
            &game,
            LiveQuery {
                mode,
                base: image_base,
                names: &name,
                patterns: &patterns,
                exclude: &[],
                kind: kind.map(|k| match k {
                    LiveKind::Model => "model",
                    LiveKind::Animation => "animation",
                }),
                json,
            },
        )?,
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
            if selection.kind.live() {
                live_list(
                    &selection.game,
                    LiveQuery {
                        mode: None,
                        base: None,
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
            ensure!(
                !selection.kind.live(),
                "Use weapon --model NAME --animation NAME --output weapon.glb for decoded model/animation extraction"
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
