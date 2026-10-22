//! Editing a line of text with a cursor in it, the same in every field: typing goes in at the
//! cursor, `←`/`→` move it, `Home`/`End` jump to either end, `Backspace` deletes the character
//! before it and `Delete` the one under it. The cursor counts characters, not bytes.

use ratatui::crossterm::event::KeyCode;

/// Takes one key for `text`, whose cursor is at character `caret`.
///
/// `None` if the key isn't one for editing text, so the caller can give it another meaning;
/// otherwise whether anything changed and needs drawing.
pub fn edit(text: &mut String, caret: &mut usize, key_code: KeyCode) -> Option<bool> {
    let length = text.chars().count();
    *caret = (*caret).min(length);
    match key_code {
        KeyCode::Char(c) => {
            text.insert(byte_at(text, *caret), c);
            *caret += 1;
            Some(true)
        }
        KeyCode::Backspace if *caret > 0 => {
            *caret -= 1;
            text.remove(byte_at(text, *caret));
            Some(true)
        }
        KeyCode::Delete if *caret < length => {
            text.remove(byte_at(text, *caret));
            Some(true)
        }
        KeyCode::Backspace | KeyCode::Delete => Some(false),
        KeyCode::Left => Some(move_to(caret, caret.saturating_sub(1))),
        KeyCode::Right => Some(move_to(caret, (*caret + 1).min(length))),
        KeyCode::Home => Some(move_to(caret, 0)),
        KeyCode::End => Some(move_to(caret, length)),
        _ => None,
    }
}

/// The part of `text` before character `caret`, to measure where the cursor is drawn.
pub fn before(text: &str, caret: usize) -> &str {
    &text[..byte_at(text, caret)]
}

/// The byte where character `index` starts, or the end of `text`.
fn byte_at(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(at, _)| at)
}

/// Moves the cursor to `to`, saying whether it moved.
fn move_to(caret: &mut usize, to: usize) -> bool {
    let moved = *caret != to;
    *caret = to;
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str, keys: &[KeyCode]) -> (String, usize) {
        let mut text = text.to_string();
        let mut caret = text.chars().count();
        for &key in keys {
            edit(&mut text, &mut caret, key);
        }
        (text, caret)
    }

    #[test]
    fn typing_goes_in_at_the_cursor() {
        let keys = [KeyCode::Left, KeyCode::Left, KeyCode::Char('X')];
        assert_eq!(typed("colt", &keys), ("coXlt".into(), 3));
        assert_eq!(
            typed("colt", &[KeyCode::Home, KeyCode::Char('>')]),
            (">colt".into(), 1)
        );
    }

    #[test]
    fn backspace_and_delete_take_either_side_of_the_cursor() {
        assert_eq!(
            typed("colt", &[KeyCode::Left, KeyCode::Backspace]),
            ("cot".into(), 2)
        );
        assert_eq!(
            typed("colt", &[KeyCode::Home, KeyCode::Delete]),
            ("olt".into(), 0)
        );
    }

    #[test]
    fn characters_are_counted_not_bytes() {
        let keys = [KeyCode::Left, KeyCode::Backspace, KeyCode::Char('u')];
        assert_eq!(typed("Verrückt", &keys[..1]).1, 7);
        assert_eq!(typed("rüü", &keys), ("ruü".into(), 2));
        assert_eq!(before("Verrückt", 5), "Verrü");
    }

    #[test]
    fn other_keys_are_left_to_the_caller() {
        let mut text = String::new();
        let mut caret = 0;
        assert_eq!(edit(&mut text, &mut caret, KeyCode::Tab), None);
        assert_eq!(edit(&mut text, &mut caret, KeyCode::Enter), None);
    }
}
