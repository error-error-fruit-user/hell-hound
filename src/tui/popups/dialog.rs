use anyhow::Result;
use ratatui::crossterm::event::KeyCode;

use crate::{
    tui::{Grimoire, Page, Phase, Ritual, popups, theme::THEME},
    weapon::Problem,
};

/// A yes/no popup, answered with `y` or `n`, or by picking an answer with `←`/`→` and `Enter`.
/// `Esc` answers no.
pub struct Dialog<'a> {
    title: &'a str,
    message: &'a str,
    /// The answer picked with the arrows, if any: 0 for yes, 1 for no.
    selected: Option<usize>,
}

impl<'a> Dialog<'a> {
    pub fn new(title: &'a str, message: &'a str) -> Self {
        Self {
            title,
            message,
            selected: None,
        }
    }

    /// Returns whether the user answered yes.
    pub fn summon(grimoire: &mut Grimoire, title: &str, message: &str) -> Result<bool> {
        Dialog::new(title, message).invoke(grimoire)
    }

    /// Asks whether to work around `problem`.
    pub fn should_continue(grimoire: &mut Grimoire, problem: &Problem) -> Result<bool> {
        let message = format!("{problem}.\n\n{}?", problem.workaround());
        Self::summon(grimoire, "problem", &message)
    }
}

impl Ritual for Dialog<'_> {
    type Return = bool;

    fn render(&mut self, page: &mut Page) {
        let lines = popups::lines(self.message, THEME.text, page.area());
        let answers = popups::answers(&["Yes", "No"], self.selected);
        popups::draw(page, self.title, THEME.dialog, &lines, Some(answers));
    }

    fn prompt(&mut self, _: &mut Grimoire, key_code: KeyCode) -> Result<Phase<bool>> {
        Ok(self.answer(key_code))
    }
}

impl Dialog<'_> {
    fn answer(&mut self, key_code: KeyCode) -> Phase<bool> {
        if let Some(phase) = popups::pick(&mut self.selected, 2, key_code) {
            return match phase {
                Phase::Return(answer) => Phase::Return(answer == 0),
                Phase::Render => Phase::Render,
                Phase::Prompt => Phase::Prompt,
            };
        }
        match key_code {
            KeyCode::Char('y' | 'Y') => Phase::Return(true),
            KeyCode::Char('n' | 'N') | KeyCode::Esc => Phase::Return(false),
            _ => Phase::Prompt,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::testing;

    #[test]
    fn a_problem_asks_whether_to_work_around_it() {
        let bones = ["j_wrist_le".to_string()];
        let problem = Problem::MissingBones {
            clip: "viewmodel_ray_gun_fire",
            bones: &bones,
        };
        let message = format!("{problem}.\n\n{}?", problem.workaround());
        let mut dialog = Dialog::new("problem", &message);
        let text = testing::draw(70, 14, |page| dialog.render(page));
        assert!(text.contains("[ problem ]"), "{text}");
        assert!(text.contains("j_wrist_le"), "{text}");
        assert!(text.contains("Leave those bones out?"), "{text}");
        assert!(text.contains("[Y]es  [N]o"), "{text}");
    }

    #[test]
    fn keys_answer_and_escape_means_no() {
        let mut dialog = Dialog::new("t", "m");
        assert_eq!(dialog.answer(KeyCode::Char('y')), Phase::Return(true));
        assert_eq!(dialog.answer(KeyCode::Esc), Phase::Return(false));
        assert_eq!(dialog.answer(KeyCode::Char('x')), Phase::Prompt);
        assert_eq!(dialog.answer(KeyCode::Right), Phase::Render);
        assert_eq!(dialog.answer(KeyCode::Enter), Phase::Return(true));
    }
}
