//! 中止 (ERR_ABORTED) は、捨てた読み込みの通知なので、成功にも失敗にもしない。

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavEvent {
    /// ページが自分で URL を開いた。読み込みはこの番号で始まっている。
    PageOpened {
        generation: u64,
        url: String,
    },
    Succeeded {
        generation: u64,
    },
    Failed {
        generation: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoadNotice {
    Start,
    Error { aborted: bool },
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoadEffect {
    None,
    PageOpened {
        generation: u64,
    },
    Succeeded {
        generation: u64,
    },
    Failed {
        generation: u64,
    },
    /// ページが開いた直後、文書が確定する前に失敗した。開くと失敗をこの順で渡す。
    FailedOpen {
        generation: u64,
    },
}

#[derive(Debug)]
pub(crate) struct Loads {
    next: u64,
    requested: Option<u64>,
    started: Option<u64>,
    failed: Option<u64>,
    issued: u64,
}

impl Loads {
    pub(crate) fn new() -> Self {
        Self {
            next: 1,
            requested: None,
            started: None,
            failed: None,
            issued: 0,
        }
    }

    pub(crate) fn request(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        self.requested = Some(id);
        self.issued = id;
        id
    }

    pub(crate) fn issued(&self) -> u64 {
        self.issued
    }

    /// フラグメントだけが違う URL か。CEF はこの移動で開始も終了も送らない。
    pub(crate) fn is_fragment_move(current: &str, next: &str) -> bool {
        let current_base = current
            .split_once('#')
            .map(|(base, _)| base)
            .unwrap_or(current);
        let next_base = next.split_once('#').map(|(base, _)| base).unwrap_or(next);
        current_base == next_base && current != next
    }

    /// フラグメントだけの移動。要求を残さない。次の開始をこの番号が受け取らない。
    pub(crate) fn fragment_move(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        self.issued = id;
        id
    }

    pub(crate) fn notice(&mut self, notice: LoadNotice, url: Option<&str>) -> LoadEffect {
        match notice {
            LoadNotice::Start => self.on_start(),
            LoadNotice::Error { aborted } => self.on_error(aborted, url),
            LoadNotice::End => self.on_end(),
        }
    }

    fn on_start(&mut self) -> LoadEffect {
        if let Some(id) = self.requested.take() {
            self.started = Some(id);
            self.failed = None;
            return LoadEffect::None;
        }
        if self.started.is_some() {
            return LoadEffect::None;
        }
        let id = self.next;
        self.next += 1;
        self.issued = id;
        self.started = Some(id);
        self.failed = None;
        LoadEffect::PageOpened { generation: id }
    }

    fn on_error(&mut self, aborted: bool, url: Option<&str>) -> LoadEffect {
        if aborted {
            return LoadEffect::None;
        }
        let id = if let Some(id) = self.started {
            id
        } else if let Some(id) = self.requested.take() {
            self.started = Some(id);
            id
        } else if self.failed.is_some() || url.is_none_or(|url| url.is_empty()) {
            return LoadEffect::None;
        } else {
            let id = self.next;
            self.next += 1;
            self.issued = id;
            self.failed = Some(id);
            return LoadEffect::FailedOpen { generation: id };
        };
        if self.failed == Some(id) {
            return LoadEffect::None;
        }
        self.failed = Some(id);
        LoadEffect::Failed { generation: id }
    }

    fn on_end(&mut self) -> LoadEffect {
        let Some(id) = self.started.take() else {
            return LoadEffect::None;
        };
        if self.failed == Some(id) {
            self.failed = None;
            return LoadEffect::None;
        }
        LoadEffect::Succeeded { generation: id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_start_and_end_succeed_once() {
        let mut loads = Loads::new();
        let id = loads.request();
        assert_eq!(loads.notice(LoadNotice::Start, None), LoadEffect::None);
        assert_eq!(
            loads.notice(LoadNotice::End, None),
            LoadEffect::Succeeded { generation: id }
        );
        assert_eq!(loads.notice(LoadNotice::End, None), LoadEffect::None);
    }

    #[test]
    fn error_then_end_fails_once() {
        let mut loads = Loads::new();
        let id = loads.request();
        assert_eq!(loads.notice(LoadNotice::Start, None), LoadEffect::None);
        assert_eq!(
            loads.notice(LoadNotice::Error { aborted: false }, None),
            LoadEffect::Failed { generation: id }
        );
        assert_eq!(
            loads.notice(LoadNotice::Error { aborted: false }, None),
            LoadEffect::None
        );
        assert_eq!(loads.notice(LoadNotice::End, None), LoadEffect::None);
    }

    #[test]
    fn aborted_load_is_not_a_result() {
        let mut loads = Loads::new();
        let first = loads.request();
        assert_eq!(loads.notice(LoadNotice::Start, None), LoadEffect::None);
        let second = loads.request();
        assert_eq!(
            loads.notice(LoadNotice::Error { aborted: true }, None),
            LoadEffect::None
        );
        assert_eq!(loads.notice(LoadNotice::Start, None), LoadEffect::None);
        assert_eq!(
            loads.notice(LoadNotice::End, None),
            LoadEffect::Succeeded { generation: second }
        );
        assert_ne!(first, second);
    }

    #[test]
    fn error_before_start_belongs_to_the_request() {
        let mut loads = Loads::new();
        let id = loads.request();
        assert_eq!(
            loads.notice(LoadNotice::Error { aborted: false }, None),
            LoadEffect::Failed { generation: id }
        );
        assert_eq!(loads.notice(LoadNotice::End, None), LoadEffect::None);
    }

    #[test]
    fn unrequested_start_is_a_page_open() {
        let mut loads = Loads::new();
        let first = loads.request();
        assert_eq!(loads.notice(LoadNotice::Start, None), LoadEffect::None);
        assert_eq!(
            loads.notice(LoadNotice::End, None),
            LoadEffect::Succeeded { generation: first }
        );
        let effect = loads.notice(LoadNotice::Start, None);
        let LoadEffect::PageOpened { generation } = effect else {
            panic!("page open was {effect:?}");
        };
        assert_ne!(generation, first);
        assert_eq!(
            loads.notice(LoadNotice::End, None),
            LoadEffect::Succeeded { generation }
        );
    }

    #[test]
    fn second_start_of_the_same_load_is_not_another_open() {
        let mut loads = Loads::new();
        loads.request();
        assert_eq!(loads.notice(LoadNotice::Start, None), LoadEffect::None);
        assert_eq!(loads.notice(LoadNotice::Start, None), LoadEffect::None);
    }

    #[test]
    fn fragment_move_leaves_the_next_start_as_a_page_open() {
        let mut loads = Loads::new();
        let fragment = loads.fragment_move();
        assert!(Loads::is_fragment_move(
            "file:///tmp/a.html",
            "file:///tmp/a.html#mark"
        ));
        assert!(!Loads::is_fragment_move(
            "file:///tmp/a.html",
            "file:///tmp/a.html"
        ));
        assert!(!Loads::is_fragment_move(
            "file:///tmp/a.html",
            "file:///tmp/b.html"
        ));
        let effect = loads.notice(LoadNotice::Start, None);
        let LoadEffect::PageOpened { generation } = effect else {
            panic!("page open was {effect:?}");
        };
        assert_ne!(generation, fragment);
    }

    #[test]
    fn error_without_start_opens_the_failed_url() {
        let mut loads = Loads::new();
        assert_eq!(
            loads.notice(LoadNotice::Error { aborted: false }, None),
            LoadEffect::None
        );
        let effect = loads.notice(
            LoadNotice::Error { aborted: false },
            Some("https://down.example/"),
        );
        let LoadEffect::FailedOpen { generation } = effect else {
            panic!("failed open was {effect:?}");
        };
        assert_eq!(
            loads.notice(
                LoadNotice::Error { aborted: false },
                Some("https://down.example/"),
            ),
            LoadEffect::None
        );
        assert_eq!(loads.notice(LoadNotice::End, None), LoadEffect::None);
        let effect = loads.notice(LoadNotice::Start, None);
        let LoadEffect::PageOpened { generation: next } = effect else {
            panic!("page open was {effect:?}");
        };
        assert_ne!(next, generation);
    }
}
