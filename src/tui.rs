//! The terminal interface. A [`Ritual`] is one screen or popup: it draws itself, takes keys until
//! it has something to return, and nests others by invoking them. Everything is drawn on the
//! [`Grimoire`] (the terminal) through [`Haunted::bewitch`], in the colors of [`theme::THEME`].

pub mod layout;
pub mod popups;
pub mod screens;
pub mod text;
pub mod theme;

use std::{fmt, io::IsTerminal, path::PathBuf, time::Duration};

use anyhow::{Result, ensure};
pub use ratatui::Frame as Page;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::event::{self, Event, KeyCode, KeyModifiers},
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding},
};

use self::{popups::resize, theme::THEME};
use crate::discovery;

/// The terminal everything is drawn on.
pub type Grimoire = ratatui::Terminal<CrosstermBackend<std::io::Stdout>>;

/// Ctrl+C: not a failure, but how the user leaves from anywhere. It unwinds out of every ritual
/// as an error, and `main` exits quietly on it.
#[derive(Debug)]
pub struct Interrupted;

impl fmt::Display for Interrupted {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Interrupted")
    }
}

impl std::error::Error for Interrupted {}

/// Drawing on a [`Grimoire`].
///
/// # Safety
/// Nothing here can cause undefined behavior; the trait is `unsafe` by choice, so implementing it
/// is always a deliberate act.
pub unsafe trait Haunted {
    /// Draws a whole page: the background first, then `draw`.
    fn bewitch(&mut self, draw: impl FnOnce(&mut Page)) -> Result<()>;

    /// The page that would be drawn next, to measure it without drawing.
    fn consult(&mut self) -> Page<'_>;
}

// # Safety
// See the trait: there is nothing to uphold.
unsafe impl Haunted for Grimoire {
    fn bewitch(&mut self, draw: impl FnOnce(&mut Page)) -> Result<()> {
        self.draw(|page| {
            let background = Style::new().fg(THEME.text).bg(THEME.background);
            page.render_widget(Block::new().style(background), page.area());
            draw(page)
        })?;
        Ok(())
    }

    fn consult(&mut self) -> Page<'_> {
        self.get_frame()
    }
}

/// What a ritual wants after a key: redraw, keep waiting, or give back its value.
#[must_use]
#[derive(Debug, Eq, PartialEq)]
pub enum Phase<T> {
    Render,
    Prompt,
    Return(T),
}

impl<T> Phase<T> {
    /// Render if `condition` holds, otherwise keep waiting for the next key.
    pub fn render_if(condition: bool) -> Self {
        if condition {
            Self::Render
        } else {
            Self::Prompt
        }
    }

    /// If `condition` holds, run `change` and render; otherwise keep waiting.
    pub fn render_if_then(condition: bool, change: impl FnOnce()) -> Self {
        if condition {
            change();
            Self::Render
        } else {
            Self::Prompt
        }
    }
}

/// A screen or popup.
pub trait Ritual {
    type Return;

    /// Runs once before the first draw.
    fn attach(&mut self, _grimoire: &mut Grimoire) -> Result<()> {
        Ok(())
    }

    fn render(&mut self, page: &mut Page);

    fn prompt(&mut self, grimoire: &mut Grimoire, key_code: KeyCode)
    -> Result<Phase<Self::Return>>;

    /// Runs about once a second while no key comes, for screens that watch something.
    fn tick(&mut self, _grimoire: &mut Grimoire) -> Result<Phase<Self::Return>> {
        Ok(Phase::Prompt)
    }

    /// Attaches, then draws and takes keys until there is a value to return.
    fn invoke(&mut self, grimoire: &mut Grimoire) -> Result<Self::Return> {
        self.attach(grimoire)?;
        loop {
            grimoire.bewitch(|page| self.render(page))?;
            loop {
                let phase = match next_input(grimoire)? {
                    Input::Key(key_code) => self.prompt(grimoire, key_code)?,
                    Input::Redraw => Phase::Render,
                    Input::Idle => self.tick(grimoire)?,
                };
                match phase {
                    Phase::Render => break,
                    Phase::Prompt => {}
                    Phase::Return(value) => return Ok(value),
                }
            }
        }
    }
}

enum Input {
    Key(KeyCode),
    /// The terminal was resized.
    Redraw,
    /// A second went by with no key.
    Idle,
}

/// Waits up to a second for a key press. A resize redraws (after the resize notice, if the
/// terminal got too small); Ctrl+C becomes [`Interrupted`].
fn next_input(grimoire: &mut Grimoire) -> Result<Input> {
    if !event::poll(Duration::from_secs(1))? {
        return Ok(Input::Idle);
    }
    match event::read()? {
        Event::Resize(width, height) => {
            if layout::is_too_small(width, height) {
                resize::summon(grimoire)?;
            }
            Ok(Input::Redraw)
        }
        Event::Key(key) if key.is_press() => {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                Err(Interrupted.into())
            } else {
                Ok(Input::Key(key.code))
            }
        }
        _ => Ok(Input::Idle),
    }
}

/// Returns the bordered window, its area, and the `[F1/?]` hint drawn just below it. The window
/// is `width` x `height`, borders included, centered, and smaller if the page is.
pub fn conjure_window<'a>(
    title: &'a str,
    width: u16,
    height: u16,
    border: Color,
    area: Rect,
) -> (Block<'a>, Rect, Line<'a>, Rect) {
    // One extra row underneath, for the hint.
    let around = layout::center(width, height + 1, area);
    let window_area = Rect {
        height: around.height.saturating_sub(1),
        ..around
    };
    let (sigil, sigil_area) = conjure_sigil(window_area);
    (conjure_veil(title, border), window_area, sigil, sigil_area)
}

/// A bordered, titled, padded window, for callers that place it themselves.
pub fn conjure_veil(title: &str, border: Color) -> Block<'_> {
    let title = Line::from(vec![
        Span::styled("[", Style::new().fg(border)),
        Span::styled(
            format!(" {title} "),
            Style::new().fg(THEME.title).add_modifier(Modifier::BOLD),
        ),
        Span::styled("]", Style::new().fg(border)),
    ]);
    Block::new()
        .borders(Borders::ALL)
        .border_type(THEME.window_border)
        .border_style(Style::new().fg(border))
        .title(title)
        .title_alignment(Alignment::Center)
        .padding(Padding::horizontal(layout::PADDING))
}

/// The `[F1/?]` hint on the row just under `window_area`: either key shows the keys.
pub fn conjure_sigil(window_area: Rect) -> (Line<'static>, Rect) {
    let bracket = Style::new().fg(THEME.muted);
    let key = Style::new().fg(THEME.key);
    let sigil = Line::from(vec![
        Span::styled("[", bracket),
        Span::styled("F1", key),
        Span::styled("/", bracket),
        Span::styled("?", key),
        Span::styled("]", bracket),
    ])
    .centered();
    let sigil_area = Rect::new(window_area.x, window_area.bottom(), window_area.width, 1);
    (sigil, sigil_area)
}

/// Whether `key_code` asks for the keys popup.
pub fn wants_legend(key_code: KeyCode) -> bool {
    matches!(key_code, KeyCode::F(1) | KeyCode::Char('?'))
}

/// Opens the interface on the running games, until the user quits.
pub fn run(options: discovery::Options, game_dir: Option<PathBuf>) -> Result<()> {
    ensure!(
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        "The interface needs a terminal. Use list, extract, weapon or run for batch use"
    );
    discovery::validate_options(&options)?;
    let game_dir = game_dir.map(|dir| dir.canonicalize()).transpose()?;
    ratatui::run(|grimoire| screens::games::Games::summon(grimoire, options, game_dir))
}

#[cfg(test)]
pub mod testing {
    use ratatui::{Terminal, backend::TestBackend};

    use super::Page;

    /// What `draw` puts on a `width` x `height` page, as text.
    pub fn draw(width: u16, height: u16, draw: impl FnOnce(&mut Page)) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(draw).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn render_if_renders_or_keeps_waiting() {
        assert_eq!(Phase::<()>::render_if(true), Phase::Render);
        assert_eq!(Phase::<()>::render_if(false), Phase::Prompt);
    }

    #[test]
    fn render_if_then_only_changes_when_the_condition_holds() {
        let changes = Cell::new(0);
        assert_eq!(
            Phase::<()>::render_if_then(true, || changes.set(changes.get() + 1)),
            Phase::Render
        );
        assert_eq!(
            Phase::<()>::render_if_then(false, || changes.set(changes.get() + 1)),
            Phase::Prompt
        );
        assert_eq!(changes.get(), 1);
    }
}
