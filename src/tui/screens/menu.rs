use std::path::PathBuf;

use anyhow::Result;
use ratatui::{
    crossterm::event::KeyCode,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};

use super::{
    browser::{Browser, Row},
    export::Export,
};
use crate::{
    archive::{self, Planned, Rule},
    discovery::Game,
    tui::{
        Grimoire, Page, Phase, Ritual, layout,
        popups::{dialog::Dialog, legend::Legend, message, message::Message, prompt::Prompt},
        screens,
        theme::THEME,
        wants_legend,
    },
};

/// How the user left a game's menu.
#[derive(Debug, Eq, PartialEq)]
pub enum Leave {
    /// Back to the running games.
    Back,
    Quit,
}

/// What can be done with one game.
pub struct Menu {
    game: Game,
    cursor: usize,
}

/// The menu's entries: a name, what it does, and the kind of archive file it browses (`None` for
/// the weapon export).
const ENTRIES: [(&str, &str, Option<&str>); 4] = [
    ("Images", "Textures in the game's IWDs", Some("image")),
    ("Sounds", "Sounds in the game's IWDs", Some("sound")),
    ("Other files", "Everything else in the IWDs", Some("file")),
    (
        "Export weapon",
        "Loaded models and animations to a GLB (experimental)",
        None,
    ),
];

impl Menu {
    const LEGEND: &[[&str; 2]] = &[
        ["Enter/1-4", "Open"],
        ["J↓/K↑", "Move"],
        ["Esc", "Back to the games"],
        ["Q", "Quit"],
    ];

    pub fn new(game: Game) -> Self {
        Self { game, cursor: 0 }
    }

    /// Opens the entry under the cursor, showing anything that goes wrong.
    fn open(&mut self, grimoire: &mut Grimoire) -> Result<()> {
        let result = match ENTRIES[self.cursor] {
            (name, _, Some(kind)) => browse(grimoire, &self.game, name, kind),
            (.., None) => Export::summon(grimoire, &self.game),
        };
        message::report(grimoire, result)?;
        Ok(())
    }
}

impl Ritual for Menu {
    type Return = Leave;

    fn render(&mut self, page: &mut Page) {
        let title = format!("{} · PID {}", self.game.title, self.game.pid);
        let inner = screens::window(page, &title);
        let directory = self.game.directory.as_ref().map_or_else(
            || "Game folder not found".to_string(),
            |path| path.display().to_string(),
        );
        let muted = Style::new().fg(THEME.muted);
        let width = inner.width as usize;
        let mut about = vec![layout::tail(&directory, width)];
        about.extend(layout::wrap(self.game.capability(), width));
        for (i, line) in about.iter().enumerate() {
            let line = Line::styled(line.as_str(), muted);
            page.render_widget(line, Rect::new(inner.x, inner.y + i as u16, inner.width, 1));
        }
        let top = about.len() as u16 + 1;
        let rows = ENTRIES
            .iter()
            .enumerate()
            .map(|(i, (name, what, _))| {
                Line::from(vec![
                    Span::styled(format!("{} ", i + 1), Style::new().fg(THEME.key)),
                    Span::styled(format!("{name:<15}"), Style::new().fg(THEME.text)),
                    Span::styled(*what, muted),
                ])
            })
            .collect();
        let list = Rect {
            y: inner.y + top,
            height: inner.height.saturating_sub(top),
            ..inner
        };
        screens::list(page, list, rows, Some(self.cursor), &mut 0);
    }

    fn prompt(&mut self, grimoire: &mut Grimoire, key_code: KeyCode) -> Result<Phase<Leave>> {
        if let Some(moved) = screens::traverse(&mut self.cursor, ENTRIES.len(), key_code) {
            return Ok(Phase::render_if(moved));
        }
        Ok(match key_code {
            KeyCode::Enter => {
                self.open(grimoire)?;
                Phase::Render
            }
            KeyCode::Char(c @ '1'..='4') => {
                self.cursor = c as usize - '1' as usize;
                self.open(grimoire)?;
                Phase::Render
            }
            KeyCode::Esc => Phase::Return(Leave::Back),
            KeyCode::Char('q') => Phase::Return(Leave::Quit),
            key_code if wants_legend(key_code) => {
                Legend::summon(grimoire, Self::LEGEND)?;
                Phase::Render
            }
            _ => Phase::Prompt,
        })
    }
}

/// Browses the game's archive files of one `kind`, extracting what the user picks.
fn browse(grimoire: &mut Grimoire, game: &Game, title: &str, kind: &str) -> Result<()> {
    message::status(grimoire, "Reading the game's IWDs…")?;
    let rule = Rule {
        include: vec!["*".into()],
        kind: kind.into(),
        required: false,
        ..Rule::default()
    };
    let plan = archive::select(
        &archive::catalog(&game.archives()?)?,
        &[rule],
        archive::DEFAULT_LIMIT,
    )?;
    let rows = plan
        .iter()
        .map(|planned| Row {
            name: planned.asset.member.clone(),
            detail: layout::show_size(planned.asset.size),
        })
        .collect();
    let mut browser = Browser::new(title, "Extract", rows);
    while let Some(picked) = browser.invoke(grimoire)? {
        let picked: Vec<_> = picked.into_iter().map(|i| plan[i].clone()).collect();
        let default = format!("output/{}/{kind}", game.id);
        let label = match picked.len() {
            1 => "Extract 1 file into".to_string(),
            count => format!("Extract {count} files into"),
        };
        let Some(folder) = Prompt::summon(grimoire, "extract", &label, &default)? else {
            continue;
        };
        let result = extract(grimoire, &picked, folder.into());
        if message::report(grimoire, result)? == Some(true) {
            browser.clear_marks();
        }
    }
    Ok(())
}

/// Extracts `plan` into `folder`, asking first if that would replace files. Returns whether it
/// extracted.
fn extract(grimoire: &mut Grimoire, plan: &[Planned], folder: PathBuf) -> Result<bool> {
    let existing = plan
        .iter()
        .filter(|planned| folder.join(&planned.target).exists())
        .count();
    if existing > 0 {
        let question = if plan.len() == 1 {
            format!(
                "That file is already in {}.\n\nOverwrite it?",
                folder.display()
            )
        } else {
            format!(
                "{existing} of the {} files are already in {}.\n\nOverwrite them?",
                plan.len(),
                folder.display()
            )
        };
        if !Dialog::summon(grimoire, "problem", &question)? {
            return Ok(false);
        }
    }
    message::status(grimoire, "Extracting…")?;
    archive::extract(
        plan,
        &folder,
        archive::DEFAULT_LIMIT,
        existing > 0,
        false,
        None,
    )?;
    let done = format!("Extracted {} files into {}.", plan.len(), folder.display());
    Message::done(grimoire, &done)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing;

    #[test]
    fn the_menu_shows_the_game_and_what_it_can_do() {
        let mut menu = Menu::new(Game {
            pid: 4242,
            id: "waw".into(),
            title: "World at War".into(),
            executable: "CoDWaW.exe".into(),
            directory: Some("/games/waw".into()),
            image_base: None,
            live_inspection: true,
        });
        let text = testing::draw(100, 16, |page| menu.render(page));
        assert!(text.contains("[ World at War · PID 4242 ]"), "{text}");
        assert!(text.contains("/games/waw"), "{text}");
        assert!(text.contains("4 Export weapon"), "{text}");
    }
}
