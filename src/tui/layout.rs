//! Where windows go and how big they are. Every window is sized from what it holds, then shrunk to
//! fit the terminal; lists that no longer fit scroll instead.

use ratatui::layout::{Margin, Rect};

/// The smallest terminal the interface is drawn in. Anything smaller shows the resize notice.
pub const MIN_WIDTH: u16 = 40;
pub const MIN_HEIGHT: u16 = 16;
/// Space kept free between windows and the edges of the terminal.
pub const MARGIN: u16 = 1;
/// Blank columns between a window's border and what is inside it.
pub const PADDING: u16 = 1;

/// Whether the terminal is smaller than the interface can be drawn in.
pub fn is_too_small(width: u16, height: u16) -> bool {
    width < MIN_WIDTH || height < MIN_HEIGHT
}

/// The outer size of a window whose contents are `width` x `height`: its borders and padding added.
pub fn window(width: u16, height: u16) -> (u16, u16) {
    (width + 2 + 2 * PADDING, height + 2)
}

/// A `width` x `height` box centered in `area` inside the margin, shrunk to fit if `area` is
/// smaller.
pub fn center(width: u16, height: u16, area: Rect) -> Rect {
    let usable = area.inner(Margin::new(MARGIN, MARGIN));
    let width = width.min(usable.width);
    let height = height.min(usable.height);
    Rect::new(
        usable.x + (usable.width - width) / 2,
        usable.y + (usable.height - height) / 2,
        width,
        height,
    )
}

/// The first row to show in a list `height` rows tall, starting from `offset`, so that `selected`
/// is on screen. Scrolls as little as possible.
pub fn scroll(offset: usize, selected: usize, height: usize) -> usize {
    if selected < offset {
        selected
    } else if height > 0 && selected >= offset + height {
        selected + 1 - height
    } else {
        offset
    }
}

/// `bytes` in the largest unit there is at least one of, to one decimal.
pub fn show_size(bytes: u64) -> String {
    match bytes {
        1_000_000_000.. => format!("{:.1} GB", bytes as f64 / 1e9),
        1_000_000.. => format!("{:.1} MB", bytes as f64 / 1e6),
        1_000.. => format!("{:.1} KB", bytes as f64 / 1e3),
        _ => format!("{bytes} B"),
    }
}

/// The display width of `text`.
pub fn width(text: &str) -> u16 {
    ratatui::text::Line::from(text).width() as u16
}

/// `text` broken into lines no wider than `width`, at spaces where possible.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let mut word = word.to_string();
            while word.chars().count() > width {
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                let rest = word.split_off(
                    word.char_indices()
                        .nth(width)
                        .map_or(word.len(), |(i, _)| i),
                );
                lines.push(std::mem::replace(&mut word, rest));
            }
            let needed =
                line.chars().count() + usize::from(!line.is_empty()) + word.chars().count();
            if needed > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&word);
        }
        lines.push(line);
    }
    lines
}

/// The last `max` characters of `text`, with a `…` in front if the start was cut off, so the end
/// of a long path (the part that matters) stays visible.
pub fn tail(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        text.to_string()
    } else if max == 0 {
        String::new()
    } else {
        format!(
            "…{}",
            text.chars().skip(count - (max - 1)).collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_is_centered_inside_the_margin_and_shrunk_to_fit() {
        let screen = Rect::new(0, 0, 80, 24);
        assert_eq!(center(20, 10, screen), Rect::new(30, 7, 20, 10));
        assert_eq!(center(500, 500, screen), Rect::new(1, 1, 78, 22));
    }

    #[test]
    fn scrolling_only_moves_when_the_selection_leaves_the_screen() {
        assert_eq!(scroll(0, 3, 5), 0);
        assert_eq!(scroll(0, 5, 5), 1);
        assert_eq!(scroll(4, 2, 5), 2);
        assert_eq!(scroll(0, 0, 0), 0);
    }

    #[test]
    fn wrapping_breaks_at_spaces_and_keeps_newlines() {
        assert_eq!(wrap("the round is over", 9), ["the round", "is over"]);
        assert_eq!(wrap("aaaaaaaaaa b", 4), ["aaaa", "aaaa", "aa b"]);
        assert_eq!(wrap("one\ntwo", 10), ["one", "two"]);
        assert_eq!(wrap("", 10), [""]);
    }

    #[test]
    fn long_text_keeps_its_ending() {
        assert_eq!(tail("short", 10), "short");
        assert_eq!(tail("output/waw/weapon.glb", 12), "…/weapon.glb");
    }

    #[test]
    fn sizes_are_shown_in_the_largest_unit() {
        assert_eq!(show_size(900), "900 B");
        assert_eq!(show_size(48_000_000), "48.0 MB");
    }
}
