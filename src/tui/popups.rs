//! Small windows that ask or tell one thing, then get out of the way. They share one look: plain
//! border, a blank row around the text, and wrapped lines so they stay small on any terminal.

pub mod dialog;
pub mod legend;
pub mod message;
pub mod prompt;
pub mod resize;

use ratatui::{
    crossterm::event::KeyCode,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
};

use super::{Page, Phase, conjure_window, layout, theme::THEME};

/// The widest a popup's text gets before it wraps, so popups stay small.
pub const MAX_TEXT_WIDTH: u16 = 56;

/// `text` wrapped to fit a popup in `area`.
pub fn wrap(text: &str, area: Rect) -> Vec<String> {
    let room = area
        .width
        .saturating_sub(2 * layout::MARGIN + 2 + 2 * layout::PADDING);
    layout::wrap(text, MAX_TEXT_WIDTH.min(room) as usize)
}

/// `text` wrapped for a popup in `area`, as lines in `color`.
pub fn lines(text: &str, color: Color, area: Rect) -> Vec<Line<'static>> {
    wrap(text, area)
        .into_iter()
        .map(|line| Line::styled(line, Style::new().fg(color)))
        .collect()
}

/// The answers as `[Y]es  [N]o`: the first letter of each is the key that picks it. The one
/// `selected` with the arrows, if any, is drawn as the cursor.
pub fn answers(options: &[&str], selected: Option<usize>) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, option) in options.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        let mut letters = option.chars();
        let first = letters.next().map(String::from).unwrap_or_default();
        let rest = letters.as_str().to_string();
        if selected == Some(i) {
            spans.push(Span::styled(format!("[{first}]{rest}"), THEME.cursor));
            continue;
        }
        spans.push(Span::styled("[", Style::new().fg(THEME.muted)));
        spans.push(Span::styled(first, Style::new().fg(THEME.key)));
        spans.push(Span::styled("]", Style::new().fg(THEME.muted)));
        spans.push(Span::styled(rest, Style::new().fg(THEME.text)));
    }
    Line::from(spans)
}

/// Keys and what they do, as `Enter take  Esc cancel`, for a popup's footer.
pub fn hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (key, text)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(key.to_string(), Style::new().fg(THEME.key)));
        spans.push(Span::styled(
            format!(" {text}"),
            Style::new().fg(THEME.muted),
        ));
    }
    Line::from(spans)
}

/// Picking one of `count` answers with the arrows: `←`/`→` (or `Tab`/`Backtab`) move the
/// highlight, wrapping around, and `Enter` takes the highlighted answer. Nothing is highlighted
/// to begin with, so `Enter` alone never answers a question by accident.
///
/// `None` if the key isn't one of these, for the popup's own keys.
pub fn pick(selected: &mut Option<usize>, count: usize, key_code: KeyCode) -> Option<Phase<usize>> {
    match key_code {
        KeyCode::Right | KeyCode::Tab => {
            *selected = Some(selected.map_or(0, |i| (i + 1) % count));
            Some(Phase::Render)
        }
        KeyCode::Left | KeyCode::BackTab => {
            *selected = Some(selected.map_or(count - 1, |i| (i + count - 1) % count));
            Some(Phase::Render)
        }
        KeyCode::Enter => Some(selected.map_or(Phase::Prompt, Phase::Return)),
        _ => None,
    }
}

/// Draws a popup: `lines` centered, a blank row above and below them, and `footer` (usually the
/// answers) one blank row further down. Returns the area inside its border.
pub fn draw(
    page: &mut Page,
    title: &str,
    border: Color,
    lines: &[Line],
    footer: Option<Line>,
) -> Rect {
    let width = lines
        .iter()
        .chain(&footer)
        .chain([&Line::from(format!("[ {title} ]"))])
        .map(|line| line.width() as u16)
        .max()
        .unwrap_or(0);
    let height = lines.len() as u16 + 2 + footer.as_ref().map_or(0, |_| 2);
    let (width, height) = layout::window(width, height);
    let (block, area, ..) = conjure_window(title, width, height, border, page.area());
    let block = block.border_type(THEME.popup_border);
    let inner = block.inner(area);
    page.render_widget(block, area);
    let row =
        |offset: u16| Rect::new(inner.x, inner.y + offset, inner.width, 1).intersection(inner);
    for (i, line) in lines.iter().enumerate() {
        page.render_widget(line.clone().centered(), row(1 + i as u16));
    }
    if let Some(footer) = footer {
        page.render_widget(footer.centered(), row(lines.len() as u16 + 2));
    }
    inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_highlight_their_first_letter() {
        let line = answers(&["Yes", "No"], None);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "[Y]es  [N]o");
        assert_eq!(line.spans[1].style.fg, Some(THEME.key));
    }

    #[test]
    fn the_arrows_move_between_answers_and_enter_takes_one() {
        let mut selected = None;
        assert_eq!(pick(&mut selected, 2, KeyCode::Enter), Some(Phase::Prompt));
        assert_eq!(pick(&mut selected, 2, KeyCode::Right), Some(Phase::Render));
        pick(&mut selected, 2, KeyCode::Right);
        assert_eq!(selected, Some(1));
        pick(&mut selected, 2, KeyCode::Right);
        assert_eq!(selected, Some(0), "wrapping around");
        assert_eq!(
            pick(&mut selected, 2, KeyCode::Enter),
            Some(Phase::Return(0))
        );
        assert!(pick(&mut selected, 2, KeyCode::Char('y')).is_none());
    }
}
