use crate::archive::{Rule, sources_from};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub version: u32,
    #[serde(default)]
    pub sources: Vec<String>,
    pub game: Option<String>,
    pub game_dir: Option<PathBuf>,
    pub output: PathBuf,
    pub extract: Vec<Rule>,
}

pub fn load(path: &Path) -> Result<(Vec<PathBuf>, Vec<Rule>, PathBuf)> {
    let path = path
        .canonicalize()
        .with_context(|| format!("Cannot read recipe {}", path.display()))?;
    let recipe: Recipe = toml::from_str(&fs::read_to_string(&path)?)?;
    ensure!(recipe.version == 1, "Recipe needs version = 1");
    ensure!(
        !recipe.output.as_os_str().is_empty(),
        "Recipe output must be nonempty"
    );
    ensure!(
        !recipe.extract.is_empty(),
        "Recipe needs one or more [[extract]] rules"
    );
    for rule in &recipe.extract {
        rule.validate()?;
    }
    let base = path.parent().context("Recipe needs a parent directory")?;
    let output = if recipe.output.is_absolute() {
        recipe.output
    } else {
        base.join(recipe.output)
    };
    ensure!(
        usize::from(!recipe.sources.is_empty())
            + usize::from(recipe.game.is_some())
            + usize::from(recipe.game_dir.is_some())
            == 1,
        "Recipe needs exactly one of sources, game or game_dir"
    );
    let sources = if let Some(game) = recipe.game {
        crate::discovery::choose(
            &crate::discovery::Options {
                game: Some(game),
                ..Default::default()
            },
            false,
        )?
        .archives()?
    } else if let Some(directory) = recipe.game_dir {
        crate::discovery::archives_in(&if directory.is_absolute() {
            directory
        } else {
            base.join(directory)
        })?
    } else {
        sources_from(&recipe.sources, base)?
    };
    Ok((sources, recipe.extract, output))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponRecipe {
    pub version: u32,
    pub game: String,
    pub game_dir: Option<PathBuf>,
    pub output: PathBuf,
    pub weapon: WeaponSelection,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponSelection {
    pub models: Vec<String>,
    #[serde(default)]
    pub animations: Vec<String>,
    #[serde(default)]
    pub animation_patterns: Vec<String>,
    #[serde(default)]
    pub no_textures: bool,
    #[serde(default)]
    pub allow_missing_bones: bool,
}

pub fn weapon_job(path: &Path) -> Result<Option<WeaponRecipe>> {
    let path = path.canonicalize()?;
    let text = fs::read_to_string(&path)?;
    let data: toml::Value = toml::from_str(&text)?;
    if data.get("weapon").is_none() {
        return Ok(None);
    }
    let mut job: WeaponRecipe = toml::from_str(&text)?;
    ensure!(
        job.version == 1 && job.game == "waw",
        "Weapon recipes currently require version = 1 and game = 'waw'"
    );
    ensure!(
        !job.weapon.models.is_empty(),
        "Weapon recipe needs at least one exact model name"
    );
    ensure!(
        !job.output.as_os_str().is_empty(),
        "Weapon recipe output must be nonempty"
    );
    let base = path.parent().context("Recipe has no parent")?;
    if !job.output.is_absolute() {
        job.output = base.join(job.output);
    }
    if let Some(directory) = &mut job.game_dir {
        if !directory.is_absolute() {
            *directory = base.join(&*directory);
        }
    }
    Ok(Some(job))
}
