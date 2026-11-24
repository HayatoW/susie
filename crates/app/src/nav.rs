//! 1 つのウィンドウの履歴。
//! 規則は `docs/evidence/S10/navigation.md` と同じ。
//! 読み込みには番号を付け、今の番号の結果だけを成功か失敗にする。

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Start,
    Loading,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    Empty,
    Page(String),
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Open(String),
    Back,
    Forward,
    Reload,
}

/// 様子、履歴、見えている中身、今の読み込み番号。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    phase: Phase,
    entries: Vec<String>,
    index: usize,
    shown: Shown,
    generation: u64,
}

impl History {
    pub fn start() -> Self {
        Self {
            phase: Phase::Start,
            entries: Vec::new(),
            index: 0,
            shown: Shown::Empty,
            generation: 0,
        }
    }

    /// 起動して最初の URL を開いた直後。 CEF はこの番号で読み始めている。
    pub fn loading_first(url: String, generation: u64) -> Self {
        let mut history = Self::start();
        history.begin(Step::Open(url), generation);
        history
    }

    #[cfg(test)]
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    #[cfg(test)]
    pub fn shown(&self) -> &Shown {
        &self.shown
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    #[cfg(test)]
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn current(&self) -> Option<&str> {
        self.entries.get(self.index).map(String::as_str)
    }

    #[cfg(test)]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 受け付ける移動なら、読み込む URL を返す。受け付けない移動は None。
    pub fn target(&self, step: &Step) -> Option<String> {
        match step {
            Step::Open(url) => Some(url.clone()),
            Step::Back if self.can_back() => self.entries.get(self.index - 1).cloned(),
            Step::Forward if self.can_forward() => self.entries.get(self.index + 1).cloned(),
            Step::Reload if self.has_page() => self.current().map(str::to_string),
            Step::Back | Step::Forward | Step::Reload => None,
        }
    }

    /// 受け付ける移動なら履歴を変え、様子を読み込み中にする。見えている中身は変えない。
    pub fn begin(&mut self, step: Step, generation: u64) -> bool {
        if self.target(&step).is_none() {
            return false;
        }
        match step {
            Step::Open(url) => {
                let keep = if self.entries.is_empty() {
                    0
                } else {
                    self.index + 1
                };
                self.entries.truncate(keep);
                self.entries.push(url);
                self.index = self.entries.len() - 1;
            }
            Step::Back => {
                self.index -= 1;
            }
            Step::Forward => {
                self.index += 1;
            }
            Step::Reload => {}
        }
        self.phase = Phase::Loading;
        self.generation = generation;
        true
    }

    /// 今の読み込みが成功した。違う番号は、遅れた結果として捨てる。
    pub fn succeed(&mut self, generation: u64) -> bool {
        self.finish(generation, true)
    }

    /// 今の読み込みが失敗した。違う番号は、遅れた結果として捨てる。
    pub fn fail(&mut self, generation: u64) -> bool {
        self.finish(generation, false)
    }

    fn finish(&mut self, generation: u64, success: bool) -> bool {
        if self.phase != Phase::Loading || generation != self.generation {
            return false;
        }
        let Some(url) = self.current().map(str::to_string) else {
            return false;
        };
        if success {
            self.phase = Phase::Done;
            self.shown = Shown::Page(url);
        } else {
            self.phase = Phase::Failed;
            self.shown = Shown::Error(url);
        }
        true
    }

    fn can_back(&self) -> bool {
        self.index > 0 && self.index < self.entries.len()
    }

    fn can_forward(&self) -> bool {
        self.index + 1 < self.entries.len()
    }

    fn has_page(&self) -> bool {
        !self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_in_the_diagram() {
        let mut nav = History::start();
        assert_eq!(nav.phase(), &Phase::Start);
        assert!(nav.entries().is_empty());
        assert_eq!(nav.shown(), &Shown::Empty);
        assert!(nav.target(&Step::Back).is_none());
        assert!(nav.target(&Step::Forward).is_none());
        assert!(nav.target(&Step::Reload).is_none());
        assert!(!nav.begin(Step::Back, 9));
        assert_eq!(nav.generation(), 0);

        assert!(nav.begin(Step::Open("a".to_string()), 1));
        assert_eq!(nav.phase(), &Phase::Loading);
        assert_eq!(nav.entries(), ["a"]);
        assert_eq!(nav.index(), 0);
        assert_eq!(nav.shown(), &Shown::Empty);
        assert_eq!(nav.generation(), 1);
        assert!(nav.target(&Step::Back).is_none());
        assert!(nav.target(&Step::Forward).is_none());

        assert!(nav.succeed(1));
        assert_eq!(nav.phase(), &Phase::Done);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));

        let single = nav.clone();
        assert!(!nav.begin(Step::Back, 2));
        assert_eq!(nav, single);

        assert!(nav.begin(Step::Reload, 2));
        assert_eq!(nav.phase(), &Phase::Loading);
        assert_eq!(nav.entries(), ["a"]);
        assert_eq!(nav.index(), 0);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));
        assert!(nav.succeed(2));
        assert_eq!(nav.phase(), &Phase::Done);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));

        assert!(nav.begin(Step::Open("b".to_string()), 3));
        assert_eq!(nav.entries(), ["a", "b"]);
        assert_eq!(nav.index(), 1);
        assert_eq!(nav.phase(), &Phase::Loading);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));
        assert!(nav.target(&Step::Back).is_some());
        assert!(nav.target(&Step::Forward).is_none());
        assert!(!nav.begin(Step::Forward, 8));
        assert_eq!(nav.generation(), 3);

        assert!(nav.begin(Step::Back, 4));
        assert_eq!(nav.entries(), ["a", "b"]);
        assert_eq!(nav.index(), 0);
        assert_eq!(nav.phase(), &Phase::Loading);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));
        assert!(nav.target(&Step::Back).is_none());
        assert!(nav.target(&Step::Forward).is_some());
        assert!(!nav.succeed(3));
        assert_eq!(nav.entries(), ["a", "b"]);
        assert_eq!(nav.index(), 0);
        assert_eq!(nav.phase(), &Phase::Loading);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));

        assert!(nav.succeed(4));
        assert_eq!(nav.phase(), &Phase::Done);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));
        assert!(nav.target(&Step::Forward).is_some());

        assert!(nav.begin(Step::Open("c".to_string()), 5));
        assert_eq!(nav.entries(), ["a", "c"]);
        assert_eq!(nav.index(), 1);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));
        assert!(nav.fail(5));
        assert_eq!(nav.phase(), &Phase::Failed);
        assert_eq!(nav.shown(), &Shown::Error("c".to_string()));
        assert_eq!(nav.entries(), ["a", "c"]);
        assert_eq!(nav.index(), 1);
        assert!(nav.target(&Step::Back).is_some());
        assert!(nav.target(&Step::Reload).is_some());
        assert!(nav.target(&Step::Forward).is_none());
    }

    #[test]
    fn same_url_is_another_entry_and_clears_forward() {
        let mut nav = History::loading_first("a".to_string(), 1);
        nav.succeed(1);
        nav.begin(Step::Open("b".to_string()), 2);
        nav.succeed(2);
        nav.begin(Step::Back, 3);
        nav.succeed(3);
        assert_eq!(nav.entries(), ["a", "b"]);
        assert_eq!(nav.index(), 0);
        nav.begin(Step::Open("a".to_string()), 4);
        assert_eq!(nav.entries(), ["a", "a"]);
        assert_eq!(nav.index(), 1);
        assert!(nav.target(&Step::Forward).is_none());
        assert_eq!(nav.phase(), &Phase::Loading);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));
    }

    #[test]
    fn reload_keeps_the_forward_entry() {
        let mut nav = History::loading_first("a".to_string(), 1);
        nav.succeed(1);
        nav.begin(Step::Open("b".to_string()), 2);
        nav.succeed(2);
        nav.begin(Step::Back, 3);
        nav.succeed(3);
        assert!(nav.begin(Step::Reload, 4));
        assert_eq!(nav.entries(), ["a", "b"]);
        assert_eq!(nav.index(), 0);
        assert_eq!(nav.phase(), &Phase::Loading);
        assert_eq!(nav.shown(), &Shown::Page("a".to_string()));
        assert!(nav.succeed(4));
        assert_eq!(nav.target(&Step::Forward).as_deref(), Some("b"));
    }

    #[test]
    fn late_result_after_done_or_failure_changes_nothing() {
        let mut nav = History::loading_first("a".to_string(), 1);
        nav.succeed(1);
        nav.begin(Step::Open("b".to_string()), 2);
        let loading = nav.clone();
        assert!(nav.fail(2));
        let failed = nav.clone();
        assert!(!nav.succeed(1));
        assert!(!nav.fail(1));
        assert_eq!(nav, failed);
        assert_eq!(loading.shown(), &Shown::Page("a".to_string()));
    }

    #[test]
    fn blocked_move_at_start_changes_nothing() {
        let mut nav = History::start();
        let start = nav.clone();
        assert!(!nav.begin(Step::Back, 1));
        assert!(!nav.begin(Step::Forward, 1));
        assert!(!nav.begin(Step::Reload, 1));
        assert!(!nav.succeed(0));
        assert!(!nav.fail(0));
        assert_eq!(nav, start);
    }
}
