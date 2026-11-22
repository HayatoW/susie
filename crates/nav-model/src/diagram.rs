//! 図に書いた状態と、その遷移。

/// 検査で扱う履歴の件数。ブラウザ自体に、この上限はない。
pub(crate) const MAX_ENTRIES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Url {
    A,
    B,
    C,
}

/// 検査で扱う URL。ブラウザ自体に、種類の上限はない。
pub(crate) const URLS: [Url; 3] = [Url::A, Url::B, Url::C];

impl Url {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Url::A => "a",
            Url::B => "b",
            Url::C => "c",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Phase {
    Start,
    Loading,
    Done,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Shown {
    Empty,
    Page(Url),
    Error(Url),
}

/// 図の、人が見る状態。様子、履歴、見えている中身。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct DiagramState {
    pub(crate) phase: Phase,
    pub(crate) entries: Vec<Url>,
    pub(crate) index: u8,
    pub(crate) shown: Shown,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Action {
    Open(Url),
    Back,
    Forward,
    Reload,
    Succeed,
    Fail,
    Stale,
}

impl DiagramState {
    pub(crate) fn start() -> Self {
        Self {
            phase: Phase::Start,
            entries: Vec::new(),
            index: 0,
            shown: Shown::Empty,
        }
    }

    pub(crate) fn can_back(&self) -> bool {
        self.index > 0
    }

    pub(crate) fn can_forward(&self) -> bool {
        usize::from(self.index) + 1 < self.entries.len()
    }

    pub(crate) fn has_page(&self) -> bool {
        !self.entries.is_empty()
    }

    pub(crate) fn current(&self) -> Option<Url> {
        self.entries.get(usize::from(self.index)).copied()
    }
}

pub(crate) fn open_fits(state: &DiagramState) -> bool {
    let keep = if state.entries.is_empty() {
        0
    } else {
        usize::from(state.index) + 1
    };
    keep < MAX_ENTRIES
}

/// 履歴を変える操作として受け付けるか。
pub(crate) fn accepted(state: &DiagramState, action: Action) -> bool {
    match action {
        Action::Open(_) => open_fits(state),
        Action::Back => state.can_back(),
        Action::Forward => state.can_forward(),
        Action::Reload => state.has_page(),
        Action::Succeed | Action::Fail | Action::Stale => false,
    }
}

pub(crate) fn push_actions(state: &DiagramState, stale: bool, actions: &mut Vec<Action>) {
    if open_fits(state) {
        for url in URLS {
            actions.push(Action::Open(url));
        }
    }
    actions.push(Action::Back);
    actions.push(Action::Forward);
    actions.push(Action::Reload);
    if state.phase == Phase::Loading {
        actions.push(Action::Succeed);
        actions.push(Action::Fail);
    }
    if stale {
        actions.push(Action::Stale);
    }
}

pub(crate) fn apply(state: &DiagramState, stale: bool, action: Action) -> (DiagramState, bool) {
    let mut after = state.clone();
    let mut after_stale = stale;
    match action {
        Action::Open(url) => {
            if !open_fits(state) {
                return (after, after_stale);
            }
            if state.phase == Phase::Loading {
                after_stale = true;
            }
            let keep = if state.entries.is_empty() {
                0
            } else {
                usize::from(state.index) + 1
            };
            after.entries.truncate(keep);
            after.entries.push(url);
            after.index = u8::try_from(after.entries.len() - 1).expect("index fits in u8");
            after.phase = Phase::Loading;
        }
        Action::Back => {
            if !state.can_back() {
                return (after, after_stale);
            }
            if state.phase == Phase::Loading {
                after_stale = true;
            }
            after.index -= 1;
            after.phase = Phase::Loading;
        }
        Action::Forward => {
            if !state.can_forward() {
                return (after, after_stale);
            }
            if state.phase == Phase::Loading {
                after_stale = true;
            }
            after.index += 1;
            after.phase = Phase::Loading;
        }
        Action::Reload => {
            if !state.has_page() {
                return (after, after_stale);
            }
            if state.phase == Phase::Loading {
                after_stale = true;
            }
            after.phase = Phase::Loading;
        }
        Action::Succeed => {
            let url = state.current().expect("loading has a current url");
            after.phase = Phase::Done;
            after.shown = Shown::Page(url);
        }
        Action::Fail => {
            let url = state.current().expect("loading has a current url");
            after.phase = Phase::Failed;
            after.shown = Shown::Error(url);
        }
        Action::Stale => {
            after_stale = false;
        }
    }
    (after, after_stale)
}

pub(crate) fn action_label(action: Action) -> String {
    match action {
        Action::Open(url) => format!("開く {}", url.label()),
        Action::Back => "戻る".to_string(),
        Action::Forward => "進む".to_string(),
        Action::Reload => "再読み込み".to_string(),
        Action::Succeed => "成功".to_string(),
        Action::Fail => "失敗".to_string(),
        Action::Stale => "遅れた結果".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_in_the_diagram() {
        let mut stale = false;
        let mut state = DiagramState::start();
        assert_eq!(state.phase, Phase::Start);
        assert!(state.entries.is_empty());
        assert_eq!(state.shown, Shown::Empty);
        assert!(!state.can_back());
        assert!(!state.can_forward());

        (state, stale) = apply(&state, stale, Action::Open(Url::A));
        assert_eq!(state.phase, Phase::Loading);
        assert_eq!(state.entries, [Url::A]);
        assert_eq!(state.index, 0);
        assert_eq!(state.shown, Shown::Empty);
        assert!(!stale);
        assert!(!state.can_back());
        assert!(!state.can_forward());

        (state, stale) = apply(&state, stale, Action::Succeed);
        assert_eq!(state.phase, Phase::Done);
        assert_eq!(state.shown, Shown::Page(Url::A));
        assert!(!stale);

        let single = state.clone();
        let stayed = apply(&single, stale, Action::Back);
        assert_eq!(stayed, (single.clone(), false));

        (state, stale) = apply(&state, stale, Action::Reload);
        assert_eq!(state.phase, Phase::Loading);
        assert_eq!(state.entries, [Url::A]);
        assert_eq!(state.index, 0);
        assert_eq!(state.shown, Shown::Page(Url::A));
        assert!(!stale);

        (state, stale) = apply(&state, stale, Action::Succeed);
        assert_eq!(state.phase, Phase::Done);
        assert_eq!(state.shown, Shown::Page(Url::A));

        (state, stale) = apply(&state, stale, Action::Open(Url::B));
        assert_eq!(state.entries, [Url::A, Url::B]);
        assert_eq!(state.index, 1);
        assert_eq!(state.phase, Phase::Loading);
        assert_eq!(state.shown, Shown::Page(Url::A));
        assert!(state.can_back());
        assert!(!state.can_forward());
        assert!(!stale);

        let loading_b = (state.clone(), stale);
        let forwarded = apply(&loading_b.0, loading_b.1, Action::Forward);
        assert_eq!(forwarded, loading_b);

        (state, stale) = apply(&state, stale, Action::Back);
        assert_eq!(state.entries, [Url::A, Url::B]);
        assert_eq!(state.index, 0);
        assert_eq!(state.phase, Phase::Loading);
        assert_eq!(state.shown, Shown::Page(Url::A));
        assert!(stale);
        assert!(!state.can_back());
        assert!(state.can_forward());

        (state, stale) = apply(&state, stale, Action::Stale);
        assert_eq!(state.entries, [Url::A, Url::B]);
        assert_eq!(state.index, 0);
        assert_eq!(state.phase, Phase::Loading);
        assert_eq!(state.shown, Shown::Page(Url::A));
        assert!(!stale);

        (state, stale) = apply(&state, stale, Action::Succeed);
        assert_eq!(state.phase, Phase::Done);
        assert_eq!(state.shown, Shown::Page(Url::A));
        assert!(state.can_forward());
        assert!(!stale);

        (state, stale) = apply(&state, stale, Action::Open(Url::C));
        assert_eq!(state.entries, [Url::A, Url::C]);
        assert_eq!(state.index, 1);
        assert_eq!(state.phase, Phase::Loading);
        assert_eq!(state.shown, Shown::Page(Url::A));
        assert!(state.can_back());
        assert!(!state.can_forward());

        (state, stale) = apply(&state, stale, Action::Fail);
        assert_eq!(state.phase, Phase::Failed);
        assert_eq!(state.shown, Shown::Error(Url::C));
        assert_eq!(state.entries, [Url::A, Url::C]);
        assert_eq!(state.index, 1);
        assert!(!stale);

        let mut same = DiagramState::start();
        (same, _) = apply(&same, false, Action::Open(Url::A));
        (same, _) = apply(&same, false, Action::Succeed);
        (same, _) = apply(&same, false, Action::Open(Url::B));
        (same, _) = apply(&same, false, Action::Succeed);
        (same, _) = apply(&same, false, Action::Back);
        (same, _) = apply(&same, false, Action::Succeed);
        assert_eq!(same.entries, [Url::A, Url::B]);
        assert_eq!(same.index, 0);
        (same, _) = apply(&same, false, Action::Open(Url::A));
        assert_eq!(same.entries, [Url::A, Url::A]);
        assert_eq!(same.index, 1);
        assert!(!same.can_forward());
    }
}
