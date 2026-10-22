use std::path::PathBuf;

use anyhow::Result;
use ratatui::{
    crossterm::event::KeyCode,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};

use super::browser::{Browser, Row};
use crate::{
    discovery::{self, Game},
    live::{self, LiveAsset, Mode},
    tui::{
        Grimoire, Page, Phase, Ritual, layout,
        popups::{dialog::Dialog, legend::Legend, message, message::Message, prompt::Prompt},
        screens,
        theme::THEME,
        wants_legend,
    },
    weapon,
};

/// Picks loaded models and animations from a running WaW and exports them into one GLB.
pub struct Export<'a> {
    game: &'a Game,
    mode: Mode,
    image_base: u64,
    /// The loaded models and animations, read when the screen opened.
    assets: Vec<LiveAsset>,
    models: Vec<String>,
    animations: Vec<String>,
    output: String,
    textures: bool,
    cursor: usize,
}

/// The rows of the form, in order.
const MODELS: usize = 0;
const ANIMATIONS: usize = 1;
const OUTPUT: usize = 2;
const TEXTURES: usize = 3;
const EXPORT: usize = 4;

impl<'a> Export<'a> {
    const LEGEND: &[[&str; 2]] = &[
        ["Enter", "Change, or export"],
        ["Space", "Turn textures on/off"],
        ["E", "Export"],
        ["J↓/K↑", "Move"],
        ["Esc", "Back"],
    ];

    pub fn summon(grimoire: &mut Grimoire, game: &Game) -> Result<()> {
        let mode = game.waw_mode()?;
        let image_base = game.image_base.unwrap_or(0x400000);
        message::status(grimoire, "Reading the loaded models and animations…")?;
        let memory = live::Process::open(game.pid)?;
        let assets = live::list(&memory, mode, image_base)?;
        Export {
            game,
            mode,
            image_base,
            assets,
            models: vec![],
            animations: vec![],
            output: format!("output/{}/weapon.glb", game.id),
            textures: true,
            cursor: 0,
        }
        .invoke(grimoire)
    }

    /// Lets the user pick from the loaded assets of one `kind`, keeping `picked` if they go back.
    fn pick(&self, grimoire: &mut Grimoire, kind: &str, picked: &[String]) -> Result<Vec<String>> {
        let found: Vec<_> = self.assets.iter().filter(|a| a.kind == kind).collect();
        let rows = found
            .iter()
            .map(|asset| Row {
                name: asset.name.clone(),
                detail: match (asset.frames, asset.fps) {
                    (Some(frames), Some(fps)) => format!("{frames} frames · {fps} fps"),
                    _ => format!("{} bones", asset.bones),
                },
            })
            .collect();
        let title = if kind == "model" {
            "Loaded models"
        } else {
            "Loaded animations"
        };
        let mut browser = Browser::new(title, "Pick", rows).mark(picked);
        Ok(match browser.invoke(grimoire)? {
            Some(rows) => rows.iter().map(|&i| browser.name(i).to_string()).collect(),
            None => picked.to_vec(),
        })
    }

    /// Exports, asking about every problem the export could work around.
    fn export(&self, grimoire: &mut Grimoire) -> Result<()> {
        if self.models.is_empty() {
            return Message::summon(
                grimoire,
                "nothing to export",
                "Pick at least one model first. Add the hands model too if the animations move arms.",
                THEME.warning,
            );
        }
        let game = discovery::current(self.game)?;
        // Missing IWDs are asked about by the export.
        let sources = if self.textures {
            game.archives().unwrap_or_default()
        } else {
            vec![]
        };
        let options = weapon::Options {
            models: self.models.clone(),
            animations: self.animations.clone(),
            no_textures: !self.textures,
            ..weapon::Options::default()
        };
        let output = PathBuf::from(&self.output);
        let memory = live::Process::open(game.pid)?;
        message::status(grimoire, "Exporting…")?;
        weapon::export(
            &memory,
            self.mode,
            self.image_base,
            &sources,
            &output,
            &options,
            &mut |problem| {
                let answer = Dialog::should_continue(grimoire, problem)?;
                message::status(grimoire, "Exporting…")?;
                Ok(answer)
            },
        )?;
        let done = format!(
            "Wrote {}.\n\nIn Blender: File → Import → glTF 2.0. The export is experimental, so check it looks right.",
            output.display()
        );
        Message::done(grimoire, &done)
    }

    /// What a list of names says in its row.
    fn summary(names: &[String], none: &str) -> String {
        match names {
            [] => none.to_string(),
            [one] => one.clone(),
            [first, rest @ ..] => format!("{first} and {} more", rest.len()),
        }
    }

    fn rows(&self, width: usize) -> Vec<Line<'_>> {
        let field = |label: &str, value: String, unset: bool| {
            let value_style = Style::new().fg(if unset { THEME.muted } else { THEME.text });
            Line::from(vec![
                Span::styled(format!("{label:<12}"), Style::new().fg(THEME.muted)),
                Span::styled(layout::tail(&value, width.saturating_sub(12)), value_style),
            ])
        };
        vec![
            field(
                "Models",
                Self::summary(&self.models, "none: Enter to pick"),
                self.models.is_empty(),
            ),
            field(
                "Animations",
                Self::summary(&self.animations, "none: model only"),
                self.animations.is_empty(),
            ),
            field("Output", self.output.clone(), false),
            field(
                "Textures",
                if self.textures { "export" } else { "skip" }.into(),
                !self.textures,
            ),
            Line::styled("Export", Style::new().fg(THEME.title)),
        ]
    }
}

impl Ritual for Export<'_> {
    type Return = ();

    fn render(&mut self, page: &mut Page) {
        let inner = screens::window(page, "Export weapon");
        let note = "Pick the weapon's model, and its hands model if the animations move arms.";
        let notes: Vec<_> = layout::wrap(note, inner.width as usize)
            .into_iter()
            .map(|line| Line::styled(line, Style::new().fg(THEME.muted)))
            .collect();
        let below = notes.len() as u16 + 1;
        for (i, line) in notes.into_iter().enumerate() {
            page.render_widget(line, Rect::new(inner.x, inner.y + i as u16, inner.width, 1));
        }
        let list = Rect {
            y: inner.y + below,
            height: inner.height.saturating_sub(below),
            ..inner
        };
        let mut rows = self.rows(inner.width as usize);
        // A blank row sets the button apart.
        rows.insert(EXPORT, Line::raw(""));
        let cursor = if self.cursor == EXPORT {
            EXPORT + 1
        } else {
            self.cursor
        };
        screens::list(page, list, rows, Some(cursor), &mut 0);
    }

    fn prompt(&mut self, grimoire: &mut Grimoire, key_code: KeyCode) -> Result<Phase<()>> {
        if let Some(moved) = screens::traverse(&mut self.cursor, EXPORT + 1, key_code) {
            return Ok(Phase::render_if(moved));
        }
        match key_code {
            KeyCode::Enter => match self.cursor {
                MODELS => self.models = self.pick(grimoire, "model", &self.models)?,
                ANIMATIONS => {
                    self.animations = self.pick(grimoire, "animation", &self.animations)?
                }
                OUTPUT => {
                    if let Some(output) =
                        Prompt::summon(grimoire, "output", "Output GLB", &self.output)?
                        && !output.is_empty()
                    {
                        self.output = output;
                    }
                }
                TEXTURES => self.textures = !self.textures,
                _ => {
                    let result = self.export(grimoire);
                    message::report(grimoire, result)?;
                }
            },
            KeyCode::Char(' ') if self.cursor == TEXTURES => self.textures = !self.textures,
            KeyCode::Char('e') => {
                let result = self.export(grimoire);
                message::report(grimoire, result)?;
            }
            KeyCode::Esc => return Ok(Phase::Return(())),
            key_code if wants_legend(key_code) => Legend::summon(grimoire, Self::LEGEND)?,
            _ => return Ok(Phase::Prompt),
        }
        Ok(Phase::Render)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing;

    #[test]
    fn the_form_shows_what_will_be_exported() {
        let game = Game {
            pid: 1,
            id: "waw".into(),
            title: "World at War".into(),
            executable: "CoDWaW.exe".into(),
            directory: None,
            image_base: None,
            live_inspection: true,
        };
        let mut export = Export {
            game: &game,
            mode: Mode::Sp,
            image_base: 0x400000,
            assets: vec![],
            models: vec!["viewmodel_ray_gun".into(), "viewmodel_usa_hands".into()],
            animations: vec![],
            output: "output/waw/weapon.glb".into(),
            textures: false,
            cursor: EXPORT,
        };
        let text = testing::draw(80, 18, |page| export.render(page));
        assert!(text.contains("viewmodel_ray_gun and 1 more"), "{text}");
        assert!(text.contains("none: model only"), "{text}");
        assert!(text.contains("Textures    skip"), "{text}");
        assert!(text.contains("Export"), "{text}");
    }
}
