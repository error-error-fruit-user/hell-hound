use anyhow::Result;
use ratatui::{
    crossterm::event::KeyCode,
    layout::Position,
    style::Style,
    text::{Line, Span},
};

use crate::tui::{Grimoire, Page, Phase, Ritual, layout, popups, text, theme::THEME};

/// A popup with one line to type in: `Enter` takes it, `Esc` gives up.
pub struct Prompt<'a> {
    title: &'a str,
    label: &'a str,
    text: String,
    caret: usize,
}

impl<'a> Prompt<'a> {
    /// How wide the field is drawn; longer text keeps its end in sight.
    const FIELD_WIDTH: usize = 50;

    pub fn new(title: &'a str, label: &'a str, text: &str) -> Self {
        Self {
            title,
            label,
            text: text.to_string(),
            caret: text.chars().count(),
        }
    }

    /// The typed text, or `None` if the user gave up.
    pub fn summon(
        grimoire: &mut Grimoire,
        title: &str,
        label: &str,
        text: &str,
    ) -> Result<Option<String>> {
        Prompt::new(title, label, text).invoke(grimoire)
    }

    fn answer(&mut self, key_code: KeyCode) -> Phase<Option<String>> {
        match key_code {
            KeyCode::Enter => Phase::Return(Some(self.text.trim().to_string())),
            KeyCode::Esc => Phase::Return(None),
            key_code => Phase::render_if(
                text::edit(&mut self.text, &mut self.caret, key_code) == Some(true),
            ),
        }
    }
}

impl Ritual for Prompt<'_> {
    type Return = Option<String>;

    fn render(&mut self, page: &mut Page) {
        let mut lines = popups::lines(self.label, THEME.muted, page.area());
        let room = Self::FIELD_WIDTH.min(
            page.area()
                .width
                .saturating_sub(2 * layout::MARGIN + 4 + 2 * layout::PADDING) as usize,
        );
        let before = text::before(&self.text, self.caret);
        // Long text shows its end, up to the cursor.
        let shown = layout::tail(before, room.saturating_sub(1));
        let after: String = self
            .text
            .chars()
            .skip(self.caret)
            .take(room.saturating_sub(shown.chars().count()))
            .collect();
        let field = format!("{shown}{after}");
        let padded = format!("{field:<room$}");
        lines.push(Line::from(Span::styled(
            padded,
            Style::new().fg(THEME.text).bg(THEME.divider),
        )));
        let footer = popups::hints(&[("Enter", "take"), ("Esc", "cancel")]);
        let inner = popups::draw(page, self.title, THEME.dialog, &lines, Some(footer));
        // The field is centered on the row after the label.
        let field_row = inner.y + lines.len() as u16;
        let field_x = inner.x + (inner.width.saturating_sub(room as u16)) / 2;
        page.set_cursor_position(Position::new(field_x + layout::width(&shown), field_row));
    }

    fn prompt(&mut self, _: &mut Grimoire, key_code: KeyCode) -> Result<Phase<Option<String>>> {
        Ok(self.answer(key_code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing;

    #[test]
    fn typing_edits_and_enter_takes_the_text() {
        let mut prompt = Prompt::new("output", "Where to?", "output/waw");
        assert_eq!(prompt.answer(KeyCode::Char('/')), Phase::Render);
        assert_eq!(prompt.answer(KeyCode::Char('x')), Phase::Render);
        assert_eq!(
            prompt.answer(KeyCode::Enter),
            Phase::Return(Some("output/waw/x".into()))
        );
        assert_eq!(prompt.answer(KeyCode::Esc), Phase::Return(None));
    }

    #[test]
    fn the_field_is_drawn_under_the_label() {
        let mut prompt = Prompt::new("output", "Output GLB", "output/waw/weapon.glb");
        let text = testing::draw(70, 14, |page| prompt.render(page));
        assert!(text.contains("Output GLB"), "{text}");
        assert!(text.contains("output/waw/weapon.glb"), "{text}");
    }
}
