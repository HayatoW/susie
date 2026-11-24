//! ブラウザのキー。 Hayato が割り当てた。
//! Alt+← で戻る。 Alt+→ で進む。 Ctrl+R で再読み込み。 Ctrl+L でアドレス欄へ移る。
//! Vim 風のキーは、地図の 8 番まで入れない。

use gpui::Keystroke;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    Back,
    Forward,
    Reload,
    FocusAddress,
}

/// 割り当てそのもの。 shift や super が付いた別の組み合わせは、この割り当てにしない。
pub fn shortcut(keystroke: &Keystroke) -> Option<Shortcut> {
    let modifiers = keystroke.modifiers;
    if modifiers.shift || modifiers.platform || modifiers.function {
        return None;
    }
    match (modifiers.alt, modifiers.control, keystroke.key.as_str()) {
        (true, false, "left") => Some(Shortcut::Back),
        (true, false, "right") => Some(Shortcut::Forward),
        (false, true, "r") => Some(Shortcut::Reload),
        (false, true, "l") => Some(Shortcut::FocusAddress),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(source: &str) -> Keystroke {
        Keystroke::parse(source).expect("keystroke")
    }

    #[test]
    fn assigned_shortcuts() {
        assert_eq!(shortcut(&parsed("alt-left")), Some(Shortcut::Back));
        assert_eq!(shortcut(&parsed("alt-right")), Some(Shortcut::Forward));
        assert_eq!(shortcut(&parsed("ctrl-r")), Some(Shortcut::Reload));
        assert_eq!(shortcut(&parsed("ctrl-l")), Some(Shortcut::FocusAddress));
    }

    #[test]
    fn other_keys_are_not_shortcuts() {
        assert_eq!(shortcut(&parsed("left")), None);
        assert_eq!(shortcut(&parsed("r")), None);
        assert_eq!(shortcut(&parsed("ctrl-shift-r")), None);
        assert_eq!(shortcut(&parsed("alt-ctrl-left")), None);
        assert_eq!(shortcut(&parsed("ctrl-left")), None);
        assert_eq!(shortcut(&parsed("alt-r")), None);
        assert_eq!(shortcut(&parsed("super-l")), None);
    }
}
