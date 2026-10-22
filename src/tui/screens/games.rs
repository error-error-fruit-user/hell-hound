use std::path::PathBuf;

use anyhow::Result;
use ratatui::{
    crossterm::event::KeyCode,
    layout::Rect,
    style::Style,
    text::{Line, Span, Text},
};

use super::menu::{Leave, Menu};
use crate::{
    discovery::{self, Game},
    tui::{
        Grimoire, Page, Phase, Ritual, layout,
        popups::{legend::Legend, message},
        screens,
        theme::THEME,
        wants_legend,
    },
};

/// The running games, kept up to date while the screen is open. The first screen, and the one
/// every game's menu goes back to.
pub struct Games {
    options: discovery::Options,
    /// Used in place of each game's own directory, when given.
    game_dir: Option<PathBuf>,
    games: Vec<Game>,
    /// Why the games couldn't be listed, if they couldn't.
    error: Option<String>,
    cursor: usize,
    offset: usize,
}

impl Games {
    const LEGEND: &[[&str; 2]] = &[
        ["Enter", "Open the game"],
        ["J↓/K↑", "Move"],
        ["R", "Refresh"],
        ["Esc/Q", "Quit"],
    ];

    pub fn new(options: discovery::Options, game_dir: Option<PathBuf>) -> Self {
        Self {
            options,
            game_dir,
            games: vec![],
            error: None,
            cursor: 0,
            offset: 0,
        }
    }

    /// Lists the games again, saying whether anything changed.
    fn refresh(&mut self) -> bool {
        let (games, error) = match discovery::discover() {
            Ok(games) => (discovery::matching(games, &self.options), None),
            Err(error) => (vec![], Some(format!("{error:#}"))),
        };
        let changed = games != self.games || error != self.error;
        self.games = games;
        self.error = error;
        self.cursor = self.cursor.min(self.games.len().saturating_sub(1));
        changed
    }

    /// Opens the list, or straight away the game if exactly one is running.
    pub fn summon(
        grimoire: &mut Grimoire,
        options: discovery::Options,
        game_dir: Option<PathBuf>,
    ) -> Result<()> {
        let mut games = Self::new(options, game_dir);
        games.refresh();
        if games.games.len() == 1 && games.open(grimoire)? {
            return Ok(());
        }
        games.invoke(grimoire)
    }

    /// Opens the game under the cursor, saying whether the user quit from it.
    fn open(&mut self, grimoire: &mut Grimoire) -> Result<bool> {
        let Some(game) = self.games.get(self.cursor) else {
            return Ok(false);
        };
        let mut game = game.clone();
        if let Some(directory) = &self.game_dir {
            game.directory = Some(directory.clone());
        }
        let result = Menu::new(game).invoke(grimoire);
        let leave = message::report(grimoire, result)?;
        self.refresh();
        Ok(leave == Some(Leave::Quit))
    }

    fn rows(&self) -> Vec<Line<'static>> {
        self.games
            .iter()
            .map(|game| {
                Line::from(vec![
                    Span::styled(game.title.clone(), Style::new().fg(THEME.text)),
                    Span::styled(
                        format!("  {} · PID {}", game.executable, game.pid),
                        Style::new().fg(THEME.muted),
                    ),
                ])
            })
            .collect()
    }
}

impl Ritual for Games {
    type Return = ();

    fn attach(&mut self, _: &mut Grimoire) -> Result<()> {
        self.refresh();
        Ok(())
    }

    fn render(&mut self, page: &mut Page) {
        let inner = screens::window(page, "Hell Hound");
        let heading = Line::styled("Running games", THEME.header);
        page.render_widget(heading, Rect { height: 1, ..inner });
        let list = Rect {
            y: inner.y + 2,
            height: inner.height.saturating_sub(2),
            ..inner
        };
        if self.games.is_empty() {
            let text = match &self.error {
                Some(error) => error.clone(),
                None => "No Call of Duty game is running. Start one; this list updates by itself."
                    .into(),
            };
            let color = if self.error.is_some() {
                THEME.warning
            } else {
                THEME.muted
            };
            let lines: Vec<_> = layout::wrap(&text, list.width as usize)
                .into_iter()
                .map(|line| Line::styled(line, Style::new().fg(color)))
                .collect();
            page.render_widget(Text::from(lines), list);
        } else {
            let rows = self.rows();
            screens::list(page, list, rows, Some(self.cursor), &mut self.offset);
        }
    }

    fn prompt(&mut self, grimoire: &mut Grimoire, key_code: KeyCode) -> Result<Phase<()>> {
        if let Some(moved) = screens::traverse(&mut self.cursor, self.games.len(), key_code) {
            return Ok(Phase::render_if(moved));
        }
        Ok(match key_code {
            KeyCode::Enter => {
                if self.open(grimoire)? {
                    Phase::Return(())
                } else {
                    Phase::Render
                }
            }
            KeyCode::Char('r') => {
                self.refresh();
                Phase::Render
            }
            KeyCode::Esc | KeyCode::Char('q') => Phase::Return(()),
            key_code if wants_legend(key_code) => {
                Legend::summon(grimoire, Self::LEGEND)?;
                Phase::Render
            }
            _ => Phase::Prompt,
        })
    }

    fn tick(&mut self, _: &mut Grimoire) -> Result<Phase<()>> {
        Ok(Phase::render_if(self.refresh()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing;

    fn waw() -> Game {
        Game {
            pid: 4242,
            id: "waw".into(),
            title: "World at War".into(),
            executable: "CoDWaW.exe".into(),
            directory: None,
            image_base: None,
            live_inspection: true,
        }
    }

    #[test]
    fn running_games_are_listed() {
        let mut games = Games::new(discovery::Options::default(), None);
        games.games = vec![waw()];
        let text = testing::draw(70, 16, |page| games.render(page));
        assert!(text.contains("[ Hell Hound ]"), "{text}");
        assert!(
            text.contains("World at War  CoDWaW.exe · PID 4242"),
            "{text}"
        );
        assert!(text.contains("[F1/?]"), "{text}");
    }

    #[test]
    fn no_game_says_how_to_get_one() {
        let mut games = Games::new(discovery::Options::default(), None);
        let text = testing::draw(70, 16, |page| games.render(page));
        assert!(text.contains("No Call of Duty game is running"), "{text}");
    }
}
