//! Original ZIP/IWD extraction. No Greyhound code or binaries are incorporated.

use anyhow::{Context, Result, ensure};
use glob::{MatchOptions, Pattern};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

pub const DEFAULT_LIMIT: u64 = 256 * 1024 * 1024;
pub const MANIFEST: &str = "hell-hound-manifest.json";

pub fn safe_relative(value: &str, empty: bool) -> Result<()> {
    if empty && (value.is_empty() || value == ".") {
        return Ok(());
    }
    ensure!(
        !value.is_empty() && !value.contains('\\'),
        "Not a portable relative path: {value:?}"
    );
    for part in value.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        let device = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
            || (1..=9).any(|n| stem == format!("com{n}") || stem == format!("lpt{n}"));
        ensure!(
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with([' ', '.'])
                && !device
                && !part
                    .chars()
                    .any(|c| c.is_control() || "<>:\"|?*".contains(c)),
            "Not a portable relative path: {value:?}"
        );
    }
    Ok(())
}

pub fn kind_of(name: &str) -> &'static str {
    let suffix = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match suffix.as_str() {
        "iwi" | "dds" | "png" | "tga" | "jpg" => "image",
        "wav" | "mp3" | "ogg" | "flac" => "sound",
        _ => "file",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub source: PathBuf,
    pub member: String,
    pub size: u64,
    pub crc: u32,
}

fn required_default() -> bool {
    true
}
fn kind_default() -> String {
    "any".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default = "kind_default")]
    pub kind: String,
    #[serde(default)]
    pub into: String,
    #[serde(default = "required_default")]
    pub required: bool,
}

impl Default for Rule {
    fn default() -> Self {
        Self {
            names: vec![],
            include: vec![],
            exclude: vec![],
            kind: kind_default(),
            into: String::new(),
            required: true,
        }
    }
}

impl Rule {
    pub fn validate(&self) -> Result<()> {
        safe_relative(&self.into, true)?;
        ensure!(
            !self.names.is_empty() || !self.include.is_empty(),
            "Each extraction rule needs names or include patterns"
        );
        ensure!(
            ["any", "image", "sound", "file"].contains(&self.kind.as_str()),
            "Unsupported asset kind: {}",
            self.kind
        );
        for name in &self.names {
            safe_relative(name, false)?;
        }
        for pattern in self.include.iter().chain(&self.exclude) {
            Pattern::new(pattern)?;
        }
        Ok(())
    }

    pub fn matches(&self, asset: &Asset) -> bool {
        let matches = |p: &String| {
            Pattern::new(p).is_ok_and(|pattern| {
                pattern.matches_with(
                    &asset.member,
                    MatchOptions {
                        case_sensitive: true,
                        require_literal_separator: false,
                        require_literal_leading_dot: false,
                    },
                )
            })
        };
        (self.names.contains(&asset.member) || self.include.iter().any(matches))
            && !self.exclude.iter().any(matches)
            && (self.kind == "any" || self.kind == kind_of(&asset.member))
    }
}

pub fn sources_from(patterns: &[String], base: &Path) -> Result<Vec<PathBuf>> {
    let mut result = vec![];
    for pattern in patterns {
        let expanded = if pattern == "~" || pattern.starts_with("~/") {
            let home = std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .context("Cannot expand ~: home directory is unknown")?;
            PathBuf::from(home).join(pattern.strip_prefix("~/").unwrap_or(""))
        } else {
            PathBuf::from(pattern)
        };
        let absolute = if expanded.is_absolute() {
            expanded
        } else {
            base.join(expanded)
        };
        let mut matches = glob::glob(absolute.to_str().context("Source glob is not UTF-8")?)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        matches.sort();
        ensure!(
            !matches.is_empty(),
            "Source did not match any files: {pattern}"
        );
        for path in matches {
            ensure!(
                !path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("ff")),
                "Fastfile (.ff) parsing is not implemented; use IWD archives"
            );
            ensure!(path.is_file(), "Source is not a file: {}", path.display());
            let canonical = path.canonicalize()?;
            if !result.contains(&canonical) {
                result.push(canonical);
            }
        }
    }
    ensure!(
        !result.is_empty(),
        "At least one source archive is required"
    );
    Ok(result)
}

pub fn catalog(sources: &[PathBuf]) -> Result<Vec<Asset>> {
    let mut effective = BTreeMap::new();
    for source in sources {
        let mut archive = ZipArchive::new(File::open(source)?)
            .with_context(|| format!("Cannot read archive {}", source.display()))?;
        let mut seen = BTreeSet::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_owned();
            safe_relative(&name, false)?;
            ensure!(
                entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                "Archive symlink is not allowed: {name}"
            );
            ensure!(
                seen.insert(name.to_lowercase()),
                "Duplicate member (ignoring case) in one archive: {name}"
            );
            // Later source archives override earlier ones by path, ignoring case as the
            // game's file system does.
            effective.insert(
                name.to_lowercase(),
                Asset {
                    source: source.clone(),
                    member: name,
                    size: entry.size(),
                    crc: entry.crc32(),
                },
            );
        }
    }
    Ok(effective.into_values().collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub asset: Asset,
    pub target: String,
}

pub fn select(assets: &[Asset], rules: &[Rule], limit: u64) -> Result<Vec<Planned>> {
    ensure!(limit > 0, "Byte limit must be positive");
    let mut targets: BTreeMap<String, Planned> = BTreeMap::new();
    for (index, rule) in rules.iter().enumerate() {
        rule.validate()?;
        let matches: Vec<_> = assets.iter().filter(|a| rule.matches(a)).collect();
        if rule.required {
            ensure!(!matches.is_empty(), "Rule {} matched no assets", index + 1);
            for name in &rule.names {
                ensure!(
                    matches.iter().any(|a| &a.member == name),
                    "Rule {} missing exact name: {name}",
                    index + 1
                );
            }
        }
        for asset in matches {
            ensure!(
                asset.size <= limit,
                "Asset exceeds byte limit: {} ({})",
                asset.member,
                asset.size
            );
            let target = if rule.into.is_empty() || rule.into == "." {
                asset.member.clone()
            } else {
                format!("{}/{}", rule.into, asset.member)
            };
            safe_relative(&target, false)?;
            let key = target.to_lowercase();
            ensure!(
                key != MANIFEST && !key.starts_with(&format!("{MANIFEST}/")),
                "Asset collides with extraction manifest"
            );
            let entry = Planned {
                asset: asset.clone(),
                target,
            };
            if let Some(previous) = targets.get(&key) {
                ensure!(previous == &entry, "Output collision: {}", entry.target);
            }
            targets.insert(key, entry);
        }
    }
    for key in targets.keys() {
        let mut prefix = String::new();
        let parts: Vec<_> = key.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            ensure!(
                !targets.contains_key(&prefix),
                "File/directory collision: {key}"
            );
        }
    }
    let mut result: Vec<_> = targets.into_values().collect();
    result.sort_by(|a, b| a.target.cmp(&b.target));
    Ok(result)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn file_hash(path: &Path) -> Result<String> {
    let mut reader = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn no_symlinks(path: &Path) -> Result<()> {
    for component in path.ancestors() {
        match fs::symlink_metadata(component) {
            Ok(meta) => ensure!(
                !meta.file_type().is_symlink(),
                "Refusing symlink output path: {}",
                component.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn target_path(output: &Path, relative: &str) -> Result<PathBuf> {
    safe_relative(relative, false)?;
    let path = output.join(relative);
    no_symlinks(&path)?;
    for parent in path.ancestors().skip(1) {
        ensure!(
            !parent.exists() || parent.is_dir(),
            "Output parent is not a directory: {}",
            parent.display()
        );
    }
    Ok(path)
}

pub fn read_asset(asset: &Asset, limit: u64) -> Result<Vec<u8>> {
    let mut archive = ZipArchive::new(File::open(&asset.source)?)?;
    let member = archive.by_name(&asset.member)?;
    ensure!(
        member.size() == asset.size && member.crc32() == asset.crc,
        "Archive changed during extraction: {}",
        asset.source.display()
    );
    let mut data = vec![];
    member
        .take(limit.saturating_add(1))
        .read_to_end(&mut data)?;
    ensure!(
        data.len() as u64 <= limit && data.len() as u64 == asset.size,
        "Invalid/oversized member: {}",
        asset.member
    );
    Ok(data)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    no_symlinks(path)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    if overwrite {
        temporary.persist(path).map_err(|e| e.error)?;
    } else {
        temporary.persist_noclobber(path).map_err(|e| e.error)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub name: String,
    pub kind: String,
    pub archive: String,
    pub archive_sha256: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub tool: String,
    pub version: u32,
    pub assets: Vec<Record>,
}

pub fn extract(
    plan: &[Planned],
    output: &Path,
    limit: u64,
    overwrite: bool,
    dry_run: bool,
    lock: Option<&Manifest>,
) -> Result<Manifest> {
    ensure!(limit > 0, "Byte limit must be positive");
    let output = std::path::absolute(output)?;
    let mut records = vec![];
    let mut hashes = BTreeMap::new();
    let sources: BTreeSet<_> = plan.iter().map(|p| &p.asset.source).collect();
    // Validate the whole plan, existing outputs, and lock before publishing any file.
    for entry in plan {
        let path = target_path(&output, &entry.target)?;
        let data = read_asset(&entry.asset, limit)?;
        let sha256 = digest(&data);
        if path.exists() {
            ensure!(path.is_file(), "Output is not a file: {}", path.display());
            ensure!(
                !sources.contains(&path.canonicalize()?),
                "Output would overwrite a source archive"
            );
            ensure!(
                overwrite || file_hash(&path)? == sha256,
                "Different file exists (use --overwrite): {}",
                path.display()
            );
        }
        if !hashes.contains_key(&entry.asset.source) {
            hashes.insert(entry.asset.source.clone(), file_hash(&entry.asset.source)?);
        }
        records.push(Record {
            name: entry.asset.member.clone(),
            kind: kind_of(&entry.asset.member).into(),
            archive: entry
                .asset
                .source
                .file_name()
                .context("Source has no filename")?
                .to_string_lossy()
                .into(),
            archive_sha256: hashes[&entry.asset.source].clone(),
            path: entry.target.clone(),
            size: data.len() as u64,
            sha256,
        });
    }
    let manifest = Manifest {
        tool: "hell-hound".into(),
        version: 1,
        assets: records,
    };
    if let Some(lock) = lock {
        ensure!(
            lock == &manifest,
            "Selection/content differs from lock manifest; nothing extracted"
        );
    }
    let manifest_path = target_path(&output, MANIFEST)?;
    if manifest_path.exists() {
        ensure!(manifest_path.is_file(), "Manifest path is not a file");
        ensure!(
            !sources.contains(&manifest_path.canonicalize()?),
            "Manifest would overwrite a source archive"
        );
        // Even --overwrite must not adopt an unrelated/malformed manifest.
        let previous: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)
            .context("Existing manifest is not a valid Hell Hound manifest")?;
        ensure!(
            previous.tool == "hell-hound" && previous.version == 1,
            "Refusing unrelated manifest"
        );
        ensure!(
            overwrite || previous == manifest,
            "Different manifest exists; choose another output or use --overwrite"
        );
    }
    if dry_run {
        return Ok(manifest);
    }
    for (entry, record) in plan.iter().zip(&manifest.assets) {
        let data = read_asset(&entry.asset, limit)?;
        ensure!(
            digest(&data) == record.sha256,
            "Source changed since preflight; extraction may be partial"
        );
        let path = target_path(&output, &entry.target)?;
        if path.is_file() && file_hash(&path)? == record.sha256 {
            continue;
        }
        atomic_write(&path, &data, overwrite)?;
    }
    let mut bytes = serde_json::to_vec_pretty(&manifest)?;
    bytes.push(b'\n');
    if !manifest_path.exists() || fs::read(&manifest_path)? != bytes {
        atomic_write(&manifest_path, &bytes, overwrite)?;
    }
    Ok(manifest)
}
