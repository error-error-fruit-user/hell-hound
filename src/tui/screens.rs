//! Full-screen rituals: the running games, a game's menu, the asset browser and the weapon export.

pub mod browser;
pub mod export;
pub mod games;
pub mod menu;

use ratatui::{
    crossterm::event::KeyCode,
    layout::Rect,
    text::{Line, Span},
    widgets::Block,
};

use super::{Page, conjure_window, theme::THEME};

/// The widest a screen's window gets.
pub const MAX_WIDTH: u16 = 100;

/// Draws a screen's window, as big as the page allows, with its `[F1/?]` hint. Returns the area
/// inside its border.
pub fn window(page: &mut Page, title: &str) -> Rect {
    let area = page.area();
    let (block, window_area, sigil, sigil_area) = conjure_window(
        title,
        MAX_WIDTH.min(area.width),
        area.height,
        THEME.screen,
        area,
    );
    let inner = block.inner(window_area);
    page.render_widget(block, window_area);
    page.render_widget(sigil, sigil_area);
    inner
}

/// Draws `rows` in `area`, scrolled so the `cursor` row is in sight, that row in the cursor
/// style across the whole width.
pub fn list(
    page: &mut Page,
    area: Rect,
    rows: Vec<Line>,
    cursor: Option<usize>,
    offset: &mut usize,
) {
    if let Some(cursor) = cursor {
        *offset = super::layout::scroll(*offset, cursor, area.height as usize);
    }
    for (i, row) in rows
        .into_iter()
        .enumerate()
        .skip(*offset)
        .take(area.height as usize)
    {
        let row_area = Rect::new(area.x, area.y + (i - *offset) as u16, area.width, 1);
        if Some(i) == cursor {
            page.render_widget(Block::new().style(THEME.cursor), row_area);
            let spans: Vec<_> = row
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content, THEME.cursor))
                .collect();
            page.render_widget(Line::from(spans), row_area);
        } else {
            page.render_widget(row, row_area);
        }
    }
}

/// Moves `cursor` through `count` rows for the list keys (`J`/`↓`, `K`/`↑`, `PgDn`/`PgUp`,
/// `Home`/`End`), saying whether it moved, or `None` if the key isn't one of them.
pub fn traverse(cursor: &mut usize, count: usize, key_code: KeyCode) -> Option<bool> {
    let last = count.saturating_sub(1);
    let to = match key_code {
        KeyCode::Char('j') | KeyCode::Down => (*cursor + 1).min(last),
        KeyCode::Char('k') | KeyCode::Up => cursor.saturating_sub(1),
        KeyCode::PageDown => (*cursor + 10).min(last),
        KeyCode::PageUp => cursor.saturating_sub(10),
        KeyCode::Home | KeyCode::Char('g') => 0,
        KeyCode::End | KeyCode::Char('G') => last,
        _ => return None,
    };
    let moved = *cursor != to;
    *cursor = to;
    Some(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_keys_stop_at_the_ends() {
        let mut cursor = 0;
        assert_eq!(traverse(&mut cursor, 3, KeyCode::Up), Some(false));
        assert_eq!(traverse(&mut cursor, 3, KeyCode::Char('j')), Some(true));
        assert_eq!(traverse(&mut cursor, 3, KeyCode::End), Some(true));
        assert_eq!(cursor, 2);
        assert_eq!(traverse(&mut cursor, 3, KeyCode::PageDown), Some(false));
        assert_eq!(traverse(&mut cursor, 0, KeyCode::Down), Some(true));
        assert_eq!(cursor, 0, "an empty list keeps the cursor at the top");
        assert_eq!(traverse(&mut cursor, 3, KeyCode::Enter), None);
    }
}
