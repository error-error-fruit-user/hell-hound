use anyhow::Result;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyModifiers},
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
};

use crate::tui::{Grimoire, Haunted, Interrupted, layout, theme::THEME};

/// Shows how much bigger the terminal has to be, until it is; Ctrl+C still quits.
pub fn summon(grimoire: &mut Grimoire) -> Result<()> {
    loop {
        grimoire.bewitch(|page| {
            let area = page.area();
            let middle = area.height / 2;
            let muted = Style::new().fg(THEME.muted);
            let lines = [
                Line::styled(
                    "too small",
                    Style::new().fg(THEME.warning).add_modifier(Modifier::BOLD),
                ),
                Line::styled(
                    format!("{}×{} needed", layout::MIN_WIDTH, layout::MIN_HEIGHT),
                    muted,
                ),
                Line::styled(format!("{}×{} now", area.width, area.height), muted),
            ];
            for (i, line) in lines.into_iter().enumerate() {
                let y = (middle + i as u16).saturating_sub(1);
                if y < area.height {
                    page.render_widget(line.centered(), Rect::new(0, y, area.width, 1));
                }
            }
        })?;
        match event::read()? {
            Event::Resize(width, height) if !layout::is_too_small(width, height) => return Ok(()),
            Event::Key(key)
                if key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                return Err(Interrupted.into());
            }
            _ => {}
        }
    }
}
