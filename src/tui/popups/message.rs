use anyhow::Result;
use ratatui::{
    crossterm::event::KeyCode,
    style::{Color, Style},
    text::Line,
};

use crate::tui::{Grimoire, Haunted, Interrupted, Page, Phase, Ritual, popups, theme::THEME};

/// A popup that tells one thing, closed by any key.
pub struct Message<'a> {
    title: &'a str,
    text: &'a str,
    border: Color,
}

impl<'a> Message<'a> {
    pub fn new(title: &'a str, text: &'a str, border: Color) -> Self {
        Self {
            title,
            text,
            border,
        }
    }

    pub fn summon(grimoire: &mut Grimoire, title: &str, text: &str, border: Color) -> Result<()> {
        Message::new(title, text, border).invoke(grimoire)
    }

    /// Something that worked.
    pub fn done(grimoire: &mut Grimoire, text: &str) -> Result<()> {
        Self::summon(grimoire, "done", text, THEME.good)
    }
}

impl Ritual for Message<'_> {
    type Return = ();

    fn render(&mut self, page: &mut Page) {
        let lines = popups::lines(self.text, THEME.text, page.area());
        let footer = Line::styled("any key closes", Style::new().fg(THEME.muted));
        popups::draw(page, self.title, self.border, &lines, Some(footer));
    }

    fn prompt(&mut self, _: &mut Grimoire, _: KeyCode) -> Result<Phase<()>> {
        Ok(Phase::Return(()))
    }
}

/// `result`'s value, or `None` after showing its error. Ctrl+C is passed on, not shown.
pub fn report<T>(grimoire: &mut Grimoire, result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.is::<Interrupted>() => Err(error),
        Err(error) => {
            Message::summon(grimoire, "error", &format!("{error:#}"), THEME.warning)?;
            Ok(None)
        }
    }
}

/// Draws a one-off notice for work that blocks, until the next draw replaces it.
pub fn status(grimoire: &mut Grimoire, text: &str) -> Result<()> {
    grimoire.bewitch(|page| {
        let lines = popups::lines(text, THEME.text, page.area());
        popups::draw(page, "working", THEME.busy, &lines, None);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing;

    #[test]
    fn a_message_says_how_to_close_it() {
        let mut message = Message::new("done", "Wrote output/waw/weapon.glb", THEME.good);
        let text = testing::draw(60, 12, |page| message.render(page));
        assert!(text.contains("[ done ]"), "{text}");
        assert!(text.contains("Wrote output/waw/weapon.glb"), "{text}");
        assert!(text.contains("any key closes"), "{text}");
    }
}
