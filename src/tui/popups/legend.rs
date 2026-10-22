use anyhow::Result;
use ratatui::{
    crossterm::event::KeyCode,
    style::Style,
    text::{Line, Span},
};

use crate::tui::{Grimoire, Page, Phase, Ritual, layout, popups, theme::THEME};

/// A popup listing `[key, what it does]` pairs, keys right-aligned, closed by any key.
pub struct Legend<'a> {
    legend: &'a [[&'a str; 2]],
}

impl<'a> Legend<'a> {
    pub fn new(legend: &'a [[&'a str; 2]]) -> Self {
        Self { legend }
    }

    pub fn summon(grimoire: &mut Grimoire, legend: &[[&str; 2]]) -> Result<()> {
        Legend::new(legend).invoke(grimoire)
    }

    /// Every row padded to the same width, so the columns line up once the rows are centered.
    fn lines(&self) -> Vec<Line<'a>> {
        let key_width = self.legend.iter().map(|[key, _]| layout::width(key)).max();
        let text_width = self
            .legend
            .iter()
            .map(|[_, text]| layout::width(text))
            .max();
        let (key_width, text_width) = (
            key_width.unwrap_or(0) as usize,
            text_width.unwrap_or(0) as usize,
        );
        self.legend
            .iter()
            .map(|[key, text]| {
                Line::from(vec![
                    Span::styled(format!("{key:>key_width$}"), Style::new().fg(THEME.key)),
                    Span::raw("  "),
                    Span::styled(format!("{text:<text_width$}"), Style::new().fg(THEME.text)),
                ])
            })
            .collect()
    }
}

impl Ritual for Legend<'_> {
    type Return = ();

    fn render(&mut self, page: &mut Page) {
        popups::draw(page, "keys", THEME.legend, &self.lines(), None);
    }

    fn prompt(&mut self, _: &mut Grimoire, _: KeyCode) -> Result<Phase<()>> {
        Ok(Phase::Return(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_is_the_same_width_so_the_columns_line_up() {
        let legend = [["Enter", "Open"], ["J↓/K↑", "Move"], ["?", "Keys"]];
        let lines = Legend::new(&legend).lines();
        assert!(lines.iter().all(|line| line.width() == lines[0].width()));
        assert_eq!(lines[2].spans[0].content, "    ?");
    }
}
