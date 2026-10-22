use std::collections::BTreeSet;

use anyhow::Result;
use ratatui::{
    crossterm::event::KeyCode,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
};

use crate::tui::{
    Grimoire, Page, Phase, Ritual, layout, popups::legend::Legend, screens, text, theme::THEME,
    wants_legend,
};

/// One asset in a [`Browser`].
pub struct Row {
    pub name: String,
    /// Shown muted after the name: a size, or a model's bones.
    pub detail: String,
}

/// A searchable list of assets to pick from. Returns the rows picked (the marked ones, or the one
/// under the cursor if none are marked), or `None` when the user goes back.
pub struct Browser {
    title: String,
    /// What `Enter` does to the picked rows, for the footer and the keys.
    action: &'static str,
    rows: Vec<Row>,
    marked: BTreeSet<usize>,
    /// The rows that match the search, as indices into `rows`.
    shown: Vec<usize>,
    search: String,
    caret: usize,
    searching: bool,
    cursor: usize,
    offset: usize,
}

impl Browser {
    pub fn new(title: impl Into<String>, action: &'static str, rows: Vec<Row>) -> Self {
        let mut browser = Self {
            title: title.into(),
            action,
            shown: vec![],
            rows,
            marked: BTreeSet::new(),
            search: String::new(),
            caret: 0,
            searching: false,
            cursor: 0,
            offset: 0,
        };
        browser.filter();
        browser
    }

    /// Marks the rows named in `names`, as picked before.
    pub fn mark(mut self, names: &[String]) -> Self {
        self.marked = (0..self.rows.len())
            .filter(|&i| names.contains(&self.rows[i].name))
            .collect();
        self
    }

    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    pub fn name(&self, row: usize) -> &str {
        &self.rows[row].name
    }

    /// Shows the rows matching the search: a glob if it has `*`, `?` or `[`, otherwise a piece of
    /// the name, either way ignoring case.
    fn filter(&mut self) {
        let search = self.search.to_lowercase();
        let glob = search
            .contains(['*', '?', '['])
            .then(|| glob::Pattern::new(&search).ok())
            .flatten();
        self.shown = (0..self.rows.len())
            .filter(|&i| {
                let name = self.rows[i].name.to_lowercase();
                match &glob {
                    Some(glob) => glob.matches(&name),
                    None => name.contains(&search),
                }
            })
            .collect();
        self.cursor = self.cursor.min(self.shown.len().saturating_sub(1));
    }

    fn legend(&self) -> [[&str; 2]; 7] {
        [
            ["Enter", self.action],
            ["Space", "Mark"],
            ["A", "Mark all shown"],
            ["/", "Search"],
            ["J↓/K↑", "Move"],
            ["Esc", "Back"],
            ["?", "Keys"],
        ]
    }

    fn answer(&mut self, key_code: KeyCode) -> Phase<Option<Vec<usize>>> {
        if self.searching {
            return match key_code {
                KeyCode::Enter | KeyCode::Down | KeyCode::Tab => {
                    self.searching = false;
                    Phase::Render
                }
                KeyCode::Esc => {
                    self.searching = false;
                    self.search.clear();
                    self.filter();
                    Phase::Render
                }
                key_code => match text::edit(&mut self.search, &mut self.caret, key_code) {
                    Some(changed) => Phase::render_if_then(changed, || self.filter()),
                    None => Phase::Prompt,
                },
            };
        }
        if let Some(moved) = screens::traverse(&mut self.cursor, self.shown.len(), key_code) {
            return Phase::render_if(moved);
        }
        match key_code {
            KeyCode::Char(' ') => {
                if let Some(&row) = self.shown.get(self.cursor) {
                    if !self.marked.remove(&row) {
                        self.marked.insert(row);
                    }
                    self.cursor = (self.cursor + 1).min(self.shown.len() - 1);
                }
                Phase::Render
            }
            KeyCode::Char('a') => {
                if self.shown.iter().all(|row| self.marked.contains(row)) {
                    for row in &self.shown {
                        self.marked.remove(row);
                    }
                } else {
                    self.marked.extend(&self.shown);
                }
                Phase::Render
            }
            KeyCode::Char('/') => {
                self.searching = true;
                self.caret = self.search.chars().count();
                Phase::Render
            }
            KeyCode::Enter if !self.marked.is_empty() => {
                Phase::Return(Some(self.marked.iter().copied().collect()))
            }
            KeyCode::Enter => match self.shown.get(self.cursor) {
                Some(&row) => Phase::Return(Some(vec![row])),
                None => Phase::Prompt,
            },
            KeyCode::Esc => Phase::Return(None),
            _ => Phase::Prompt,
        }
    }
}

impl Ritual for Browser {
    type Return = Option<Vec<usize>>;

    fn render(&mut self, page: &mut Page) {
        let inner = screens::window(page, &self.title);
        let row = |y: u16| Rect::new(inner.x, y, inner.width, 1);
        // The search, on top.
        let label = Span::styled("Search ", Style::new().fg(THEME.muted));
        let search = if self.search.is_empty() && !self.searching {
            Span::styled("press / to search", Style::new().fg(THEME.divider))
        } else {
            Span::styled(self.search.clone(), Style::new().fg(THEME.text))
        };
        page.render_widget(Line::from(vec![label, search]), row(inner.y));
        if self.searching {
            let x = inner.x + 7 + layout::width(text::before(&self.search, self.caret));
            page.set_cursor_position(Position::new(x.min(inner.right()), inner.y));
        }
        // The rows, under it.
        let list = Rect {
            y: inner.y + 2,
            height: inner.height.saturating_sub(4),
            ..inner
        };
        let width = list.width as usize;
        let rows: Vec<_> = self
            .shown
            .iter()
            .map(|&i| {
                let row = &self.rows[i];
                let marked = self.marked.contains(&i);
                let mark = if marked { THEME.mark } else { " " };
                let detail_width = row.detail.chars().count();
                let name = layout::tail(&row.name, width.saturating_sub(detail_width + 4));
                let gap = width.saturating_sub(name.chars().count() + detail_width + 2);
                let name_style = if marked {
                    THEME.marked
                } else {
                    Style::new().fg(THEME.text)
                };
                Line::from(vec![
                    Span::styled(format!("{mark} "), THEME.marked),
                    Span::styled(name, name_style),
                    Span::raw(" ".repeat(gap)),
                    Span::styled(row.detail.clone(), Style::new().fg(THEME.muted)),
                ])
            })
            .collect();
        let cursor = (!self.searching && !self.shown.is_empty()).then_some(self.cursor);
        screens::list(page, list, rows, cursor, &mut self.offset);
        if self.shown.is_empty() {
            let none = if self.rows.is_empty() {
                "Nothing here."
            } else {
                "Nothing matches the search."
            };
            page.render_widget(
                Line::styled(none, Style::new().fg(THEME.muted)),
                row(list.y),
            );
        }
        // How many, at the bottom.
        let count = format!(
            "{} of {} shown · {} marked · Enter: {}",
            self.shown.len(),
            self.rows.len(),
            self.marked.len(),
            self.action
        );
        page.render_widget(
            Line::styled(count, Style::new().fg(THEME.muted)),
            row(inner.bottom().saturating_sub(1)),
        );
    }

    fn prompt(
        &mut self,
        grimoire: &mut Grimoire,
        key_code: KeyCode,
    ) -> Result<Phase<Self::Return>> {
        if !self.searching && wants_legend(key_code) {
            Legend::summon(grimoire, &self.legend())?;
            return Ok(Phase::Render);
        }
        Ok(self.answer(key_code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing;

    fn browser() -> Browser {
        let rows = ["viewmodel_ray_gun", "viewmodel_usa_hands", "zombie_body"]
            .map(|name| Row {
                name: name.into(),
                detail: "bones 12".into(),
            })
            .into();
        Browser::new("Models", "Pick", rows)
    }

    fn typed(browser: &mut Browser, text: &str) {
        for c in text.chars() {
            let _ = browser.answer(KeyCode::Char(c));
        }
    }

    #[test]
    fn searching_filters_by_a_piece_of_the_name_or_a_glob() {
        let mut browser = browser();
        typed(&mut browser, "/VIEW");
        assert_eq!(browser.shown, [0, 1]);
        let _ = browser.answer(KeyCode::Esc);
        assert_eq!(browser.shown, [0, 1, 2], "Esc clears the search");
        typed(&mut browser, "/*hands");
        assert_eq!(browser.shown, [1]);
    }

    #[test]
    fn enter_takes_the_marked_rows_or_the_cursor_row() {
        let mut browser = browser();
        let _ = browser.answer(KeyCode::Down);
        assert_eq!(browser.answer(KeyCode::Enter), Phase::Return(Some(vec![1])));
        let _ = browser.answer(KeyCode::Char(' '));
        let _ = browser.answer(KeyCode::Up);
        let _ = browser.answer(KeyCode::Up);
        let _ = browser.answer(KeyCode::Char(' '));
        assert_eq!(
            browser.answer(KeyCode::Enter),
            Phase::Return(Some(vec![0, 1]))
        );
        assert_eq!(browser.answer(KeyCode::Esc), Phase::Return(None));
    }

    #[test]
    fn a_marks_every_shown_row_then_none() {
        let mut browser = browser();
        typed(&mut browser, "/view");
        let _ = browser.answer(KeyCode::Enter);
        let _ = browser.answer(KeyCode::Char('a'));
        assert_eq!(browser.marked, BTreeSet::from([0, 1]));
        let _ = browser.answer(KeyCode::Char('a'));
        assert!(browser.marked.is_empty());
    }

    #[test]
    fn rows_show_their_marks_and_details() {
        let mut browser = browser().mark(&["zombie_body".into()]);
        let text = testing::draw(60, 14, |page| browser.render(page));
        assert!(text.contains("[ Models ]"), "{text}");
        assert!(text.contains("■ zombie_body"), "{text}");
        assert!(text.contains("bones 12"), "{text}");
        assert!(
            text.contains("3 of 3 shown · 1 marked · Enter: Pick"),
            "{text}"
        );
    }
}
