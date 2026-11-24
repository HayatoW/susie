//! アドレスバーの文字列。
//! 入力欄は 1 つで、 URL を打って Enter で開く。
//! 検索、補完、履歴は持たない。フォーカスを移すキーは、この欄の外で割り当てる。

use gpui::Keystroke;

use crate::text::committed_text;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    shown: String,
    opened: String,
    focused: bool,
    /// フォーカスした直後は、次の文字が見えている URL を置き換える。
    replace: bool,
}

pub enum AddressKey {
    Text(String),
    Backspace,
    Enter,
}

impl Address {
    pub fn new(url: &str) -> Self {
        Self {
            shown: url.to_string(),
            opened: url.to_string(),
            focused: false,
            replace: false,
        }
    }

    pub fn shown(&self) -> &str {
        &self.shown
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    /// 見えている文字列が選択されていて、次の文字がそれを置き換える。
    pub fn replaces(&self) -> bool {
        self.focused && self.replace
    }

    /// 入力欄をクリックした。ページから移ったときだけ、開いている URL を選択する。
    pub fn focus_from_click(&mut self) {
        if self.focused {
            return;
        }
        self.focused = true;
        self.replace = true;
        self.shown = self.opened.clone();
    }

    /// 移動で、開いている URL が変わった。
    /// 未確定の下書きは残す。確定済みなら、表示を移動先に合わせる。
    pub fn note_current(&mut self, url: &str) {
        if self.opened == url {
            return;
        }
        let editing = self.editing();
        self.opened = url.to_string();
        if !editing {
            self.shown = self.opened.clone();
        }
    }

    /// Ctrl+L。未確定の下書きは残す。そうでなければ、開いている URL を選択する。
    pub fn focus_from_shortcut(&mut self) {
        if self.editing() {
            return;
        }
        self.focused = true;
        self.replace = true;
        self.shown = self.opened.clone();
    }

    fn editing(&self) -> bool {
        self.focused && !self.replace && self.shown != self.opened
    }

    /// ページをクリックした。書きかけは捨てて、開いている URL に戻す。
    pub fn blur_to_page(&mut self) {
        self.focused = false;
        self.replace = false;
        self.shown = self.opened.clone();
    }

    /// Enter で開く URL があれば返す。 URL でなければページは変えない。
    pub fn on_key(&mut self, key: AddressKey) -> Option<String> {
        if !self.focused {
            return None;
        }
        match key {
            AddressKey::Text(text) => {
                if self.replace {
                    self.shown = text;
                    self.replace = false;
                } else {
                    self.shown.push_str(&text);
                }
                None
            }
            AddressKey::Backspace => {
                if self.replace {
                    self.shown.clear();
                    self.replace = false;
                } else {
                    self.shown.pop();
                }
                None
            }
            AddressKey::Enter => self.submit(),
        }
    }

    fn submit(&mut self) -> Option<String> {
        let target = navigation_url(&self.shown)?;
        self.opened = target.clone();
        self.shown = target.clone();
        self.replace = false;
        Some(target)
    }
}

/// スキーム付きの http、 https、 file だけを開く。
/// スキームのない文字列は検索にしない。
fn navigation_url(text: &str) -> Option<String> {
    let text = text.trim();
    let (scheme, rest) = text.split_once("://")?;
    if rest.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    if scheme.eq_ignore_ascii_case("http")
        || scheme.eq_ignore_ascii_case("https")
        || scheme.eq_ignore_ascii_case("file")
    {
        Some(text.to_string())
    } else {
        None
    }
}

/// 入力欄が受け取るキー。 ctrl や alt や super の付いた割り当ては、ここでは決めない。
pub fn address_key(keystroke: &Keystroke) -> Option<AddressKey> {
    if keystroke.modifiers.control || keystroke.modifiers.alt || keystroke.modifiers.platform {
        return None;
    }
    match keystroke.key.as_str() {
        "enter" => Some(AddressKey::Enter),
        "backspace" => Some(AddressKey::Backspace),
        _ => committed_text(keystroke).map(AddressKey::Text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    fn address() -> Address {
        Address::new("file:///tmp/frompage.html")
    }

    fn stroke(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_string(),
            key_char: key_char.map(str::to_string),
        }
    }

    #[test]
    fn new_address_shows_the_opened_url() {
        let address = address();
        assert_eq!(address.shown(), "file:///tmp/frompage.html");
        assert_eq!(address.opened, "file:///tmp/frompage.html");
        assert!(!address.focused());
        assert!(!address.replaces());
    }

    #[test]
    fn click_selects_the_opened_url() {
        let mut address = address();
        address.shown = "draft".to_string();
        address.focus_from_click();
        assert!(address.focused());
        assert!(address.replaces());
        assert_eq!(address.shown(), "file:///tmp/frompage.html");
    }

    #[test]
    fn another_click_keeps_the_draft() {
        let mut address = address();
        address.focus_from_click();
        address.on_key(AddressKey::Text("h".to_string()));
        address.focus_from_click();
        assert_eq!(address.shown(), "h");
        assert!(!address.replaces());
    }

    #[test]
    fn typing_replaces_the_selection_then_appends() {
        let mut address = address();
        address.focus_from_click();
        assert!(address.on_key(AddressKey::Text("f".to_string())).is_none());
        address.on_key(AddressKey::Text("ile".to_string()));
        assert_eq!(address.shown(), "file");
        assert!(!address.replaces());
        assert_eq!(address.opened, "file:///tmp/frompage.html");
    }

    #[test]
    fn backspace_clears_the_selection_then_deletes() {
        let mut address = address();
        address.focus_from_click();
        address.on_key(AddressKey::Backspace);
        assert_eq!(address.shown(), "");
        address.on_key(AddressKey::Text("ab".to_string()));
        address.on_key(AddressKey::Backspace);
        assert_eq!(address.shown(), "a");
    }

    #[test]
    fn enter_opens_a_url_and_keeps_it() {
        let mut address = address();
        address.focus_from_click();
        address.on_key(AddressKey::Text("https://example.com".to_string()));
        let opened = address.on_key(AddressKey::Enter);
        assert_eq!(opened.as_deref(), Some("https://example.com"));
        assert_eq!(address.shown(), "https://example.com");
        assert_eq!(address.opened, "https://example.com");
        assert!(address.focused());
        assert!(!address.replaces());
    }

    #[test]
    fn enter_on_words_does_not_open() {
        let mut address = address();
        address.focus_from_click();
        address.on_key(AddressKey::Text("susie".to_string()));
        assert!(address.on_key(AddressKey::Enter).is_none());
        assert_eq!(address.shown(), "susie");
        assert_eq!(address.opened, "file:///tmp/frompage.html");
    }

    #[test]
    fn navigation_updates_the_opened_url_unless_a_draft_is_in_progress() {
        let mut address = address();
        address.note_current("file:///tmp/next.html");
        assert_eq!(address.shown(), "file:///tmp/next.html");
        assert_eq!(address.opened, "file:///tmp/next.html");

        address.focus_from_click();
        address.on_key(AddressKey::Text("https://example.com".to_string()));
        address.note_current("file:///tmp/other.html");
        assert_eq!(address.shown(), "https://example.com");
        assert_eq!(address.opened, "file:///tmp/other.html");
        address.blur_to_page();
        assert_eq!(address.shown(), "file:///tmp/other.html");
    }

    #[test]
    fn submitted_url_follows_navigation_and_the_shortcut_selects_it() {
        let mut address = address();
        address.focus_from_click();
        address.on_key(AddressKey::Text("https://example.com/b".to_string()));
        assert!(address.on_key(AddressKey::Enter).is_some());
        address.note_current("https://example.com/a");
        assert_eq!(address.shown(), "https://example.com/a");
        assert_eq!(address.opened, "https://example.com/a");
        address.focus_from_shortcut();
        assert!(address.replaces());
        address.on_key(AddressKey::Text("https://example.com/c".to_string()));
        assert_eq!(address.shown(), "https://example.com/c");
    }

    #[test]
    fn shortcut_keeps_an_unsent_draft() {
        let mut address = address();
        address.focus_from_click();
        address.on_key(AddressKey::Text("https://example.com".to_string()));
        address.focus_from_shortcut();
        assert_eq!(address.shown(), "https://example.com");
        assert!(!address.replaces());
    }

    #[test]
    fn navigation_replaces_a_fresh_selection() {
        let mut address = address();
        address.focus_from_click();
        address.note_current("file:///tmp/next.html");
        assert!(address.replaces());
        assert_eq!(address.shown(), "file:///tmp/next.html");
    }

    #[test]
    fn blur_drops_the_draft() {
        let mut address = address();
        address.focus_from_click();
        address.on_key(AddressKey::Text("https://example.com".to_string()));
        address.blur_to_page();
        assert!(!address.focused());
        assert_eq!(address.shown(), "file:///tmp/frompage.html");
        assert!(address.on_key(AddressKey::Text("a".to_string())).is_none());
        assert_eq!(address.shown(), "file:///tmp/frompage.html");
    }

    #[test]
    fn navigation_url_accepts_http_https_and_file() {
        assert_eq!(
            navigation_url("https://example.com/a"),
            Some("https://example.com/a".to_string())
        );
        assert_eq!(
            navigation_url("  HTTP://example.com  "),
            Some("HTTP://example.com".to_string())
        );
        assert_eq!(
            navigation_url("file:///tmp/typedurl.html"),
            Some("file:///tmp/typedurl.html".to_string())
        );
    }

    #[test]
    fn navigation_url_rejects_search_and_other_schemes() {
        assert_eq!(navigation_url("example.com"), None);
        assert_eq!(navigation_url("susie"), None);
        assert_eq!(navigation_url("https://"), None);
        assert_eq!(navigation_url("https://a b"), None);
        assert_eq!(navigation_url("javascript:alert(1)"), None);
        assert_eq!(navigation_url(""), None);
    }

    #[test]
    fn enter_and_backspace_are_address_keys() {
        assert!(matches!(
            address_key(&stroke("enter", None, Modifiers::none())),
            Some(AddressKey::Enter)
        ));
        assert!(matches!(
            address_key(&stroke("backspace", None, Modifiers::none())),
            Some(AddressKey::Backspace)
        ));
    }

    #[test]
    fn letter_is_address_text_and_shortcut_is_not() {
        let text = address_key(&stroke("f", Some("f"), Modifiers::none()));
        assert!(matches!(text, Some(AddressKey::Text(value)) if value == "f"));
        let mut modifiers = Modifiers::none();
        modifiers.control = true;
        assert!(address_key(&stroke("l", Some("l"), modifiers)).is_none());
    }
}
