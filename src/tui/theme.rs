//! Every color, border and glyph the interface uses, in the one [`THEME`]. Screens ask it for a
//! role ("a screen border", "something wrong"), never for a color.

use ratatui::{
    style::{Color, Modifier, Style},
    widgets::BorderType,
};

// The palette, from World at War's zombies maps. Every role below is picked from these.

/// Night over Nacht der Untoten: almost black, a little green.
pub const NIGHT: Color = Color::Rgb(10, 12, 10);
/// The chalk outlines of a wall buy.
pub const CHALK: Color = Color::Rgb(222, 218, 200);
/// Rusted barricades and boarded windows.
pub const RUST: Color = Color::Rgb(122, 108, 94);
/// Cold iron: lines that only divide things.
pub const IRON: Color = Color::Rgb(58, 60, 56);
/// The tally marks of the round counter.
pub const BLOOD: Color = Color::Rgb(170, 18, 14);
/// Element 115, glowing in Der Riese's teleporters.
pub const ELEMENT_115: Color = Color::Rgb(64, 214, 200);
/// The Pack-a-Punch machine's light.
pub const PACK_A_PUNCH: Color = Color::Rgb(150, 70, 220);
/// Juggernog, Speed Cola, Double Tap and Quick Revive.
pub const JUGGERNOG: Color = Color::Rgb(220, 40, 30);
pub const SPEED_COLA: Color = Color::Rgb(90, 200, 60);
pub const DOUBLE_TAP: Color = Color::Rgb(240, 170, 30);
pub const QUICK_REVIVE: Color = Color::Rgb(80, 150, 240);

/// How the whole interface looks.
pub struct Theme {
    /// Painted behind everything.
    pub background: Color,
    pub text: Color,
    /// Labels, hints, and anything unset rather than wrong.
    pub muted: Color,
    pub title: Color,
    /// Keys the user can press, wherever they are shown.
    pub key: Color,
    /// The border of the main screens.
    pub screen: Color,
    /// The border of questions.
    pub dialog: Color,
    /// The border of problems and errors, and the color of anything wrong.
    pub warning: Color,
    /// The border of the keys popup.
    pub legend: Color,
    /// The border of windows shown while work blocks.
    pub busy: Color,
    /// Something that worked, and the border that says so.
    pub good: Color,
    /// Lines that only divide things.
    pub divider: Color,
    /// The cursor row of a list.
    pub cursor: Style,
    /// A row marked to be taken, under the cursor or not.
    pub marked: Style,
    /// Column headings.
    pub header: Style,
    /// The border of full screens, and of popups.
    pub window_border: BorderType,
    pub popup_border: BorderType,
    /// In front of a marked row.
    pub mark: &'static str,
}

pub static THEME: Theme = Theme {
    background: NIGHT,
    text: CHALK,
    muted: RUST,
    title: DOUBLE_TAP,
    key: ELEMENT_115,
    screen: BLOOD,
    dialog: CHALK,
    warning: JUGGERNOG,
    legend: RUST,
    busy: QUICK_REVIVE,
    good: SPEED_COLA,
    divider: IRON,
    cursor: Style::new()
        .fg(NIGHT)
        .bg(ELEMENT_115)
        .add_modifier(Modifier::BOLD),
    marked: Style::new().fg(PACK_A_PUNCH).add_modifier(Modifier::BOLD),
    header: Style::new().fg(RUST).add_modifier(Modifier::BOLD),
    window_border: BorderType::Thick,
    popup_border: BorderType::Plain,
    mark: "■",
};
