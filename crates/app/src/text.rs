use gpui::Keystroke;

/// キーから、すでに確定した文字だけを取り出す。
/// 文字を出さないキーと、 ctrl や alt や super が付いたショートカットは含めない。
/// 変換している途中の文字は、ここには来ない。
pub fn committed_text(keystroke: &Keystroke) -> Option<String> {
    if keystroke.modifiers.control || keystroke.modifiers.alt || keystroke.modifiers.platform {
        return None;
    }
    let raw = keystroke.key_char.as_deref()?;
    let text: String = raw.chars().filter(|ch| !ch.is_control()).collect();
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Keystroke, Modifiers};

    fn stroke(key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key_char.unwrap_or("s").to_string(),
            key_char: key_char.map(str::to_string),
        }
    }

    #[test]
    fn letter_is_committed_text() {
        let text = committed_text(&stroke(Some("s"), Modifiers::none()));
        assert_eq!(text.as_deref(), Some("s"));
    }

    #[test]
    fn shifted_letter_keeps_the_character() {
        let mut modifiers = Modifiers::none();
        modifiers.shift = true;
        let text = committed_text(&stroke(Some("S"), modifiers));
        assert_eq!(text.as_deref(), Some("S"));
    }

    #[test]
    fn shortcut_is_not_committed_text() {
        let mut modifiers = Modifiers::none();
        modifiers.control = true;
        assert_eq!(committed_text(&stroke(Some("s"), modifiers)), None);
    }

    #[test]
    fn key_without_a_character_is_not_committed_text() {
        assert_eq!(committed_text(&stroke(None, Modifiers::none())), None);
    }

    #[test]
    fn control_character_is_not_committed_text() {
        assert_eq!(committed_text(&stroke(Some("\n"), Modifiers::none())), None);
    }
}
