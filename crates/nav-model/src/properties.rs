//! 図の横に並べた性質。文は `docs/evidence/S10/navigation.md` と同じ。

use crate::diagram::{self, Action, DiagramState, MAX_ENTRIES, Phase, Shown};

use super::State;

pub(crate) const BACK_NOOP: &str = "戻れないときに戻しても、履歴も表示も遅れの印も変わらない";
pub(crate) const FORWARD_NOOP: &str = "進めないときに進んでも、履歴も表示も遅れの印も変わらない";
pub(crate) const RELOAD_NOOP: &str =
    "ページがないときに再読み込みしても、履歴も表示も遅れの印も変わらない";
pub(crate) const OPEN_CLEARS_FORWARD: &str =
    "新しい移動を受け付けると、進む履歴は消え、現在はその URL になり、様子は読み込み中になる";
pub(crate) const BACK_MOVES: &str =
    "戻れるときに戻すと、履歴の並びは残って、現在は 1 つ前になり、様子は読み込み中になる";
pub(crate) const FORWARD_MOVES: &str =
    "進めるときに進むと、履歴の並びは残って、現在は 1 つ次になり、様子は読み込み中になる";
pub(crate) const RELOAD_KEEPS_HISTORY: &str =
    "再読み込みでは、履歴の並びと位置は変わらず、様子は読み込み中になる";
pub(crate) const SUPERSEDE_MARKS_STALE: &str =
    "読み込み中の移動は、その読み込みを遅れにする。読み込み中でなければ、残っている遅れを消さない";
pub(crate) const STALE_IGNORED: &str = "遅れて届いた結果は、履歴も表示も変えず、遅れの印だけを消す";
pub(crate) const SHOWN_ONLY_ON_RESULT: &str =
    "見えている中身が変わるのは、今の読み込みが成功したときか、失敗したときだけである";
pub(crate) const SUCCESS_COMMITS_PAGE: &str =
    "成功すると、完了になり、履歴の位置は変わらず、見えている中身は現在の URL のページになる";
pub(crate) const FAILURE_KEEPS_PLACE: &str =
    "失敗すると、失敗になり、履歴の位置は変わらず、見えている中身は現在の URL の失敗になる";
pub(crate) const RESULT_KEEPS_STALE: &str = "成功と失敗は、残っている遅れを消さない";
pub(crate) const CAPABILITY_MATCHES_HISTORY: &str =
    "戻れるのは前に項目があるときだけ、進めるのは先に項目があるときだけである";
pub(crate) const START_IS_EMPTY: &str =
    "開始なのは履歴が空のときだけで、そのとき見えている中身は空である";
pub(crate) const INDEX_IN_RANGE: &str = "位置は履歴の列の中にあり、検査では列は 4 件までである";

pub(crate) const REACH_OPEN: &str = "届く: 開く";
pub(crate) const REACH_BACK_BLOCKED: &str = "届く: 戻れないときの戻る";
pub(crate) const REACH_BACK: &str = "届く: 戻れるときの戻る";
pub(crate) const REACH_FORWARD_BLOCKED: &str = "届く: 進めないときの進む";
pub(crate) const REACH_FORWARD: &str = "届く: 進めるときの進む";
pub(crate) const REACH_RELOAD_EMPTY: &str = "届く: ページがないときの再読み込み";
pub(crate) const REACH_RELOAD: &str = "届く: ページがあるときの再読み込み";
pub(crate) const REACH_SUCCESS: &str = "届く: 成功";
pub(crate) const REACH_FAILURE: &str = "届く: 失敗";
pub(crate) const REACH_STALE: &str = "届く: 遅れた結果";
pub(crate) const REACH_STALE_AFTER_RESULT: &str = "届く: 完了か失敗のあとの遅れた結果";
pub(crate) const REACH_MIDDLE_DONE: &str = "届く: 戻れて進める完了";
pub(crate) const REACH_SUPERSEDE: &str = "届く: 読み込み中の別の移動";

#[cfg(test)]
pub(crate) const NAMES: &[&str] = &[
    BACK_NOOP,
    FORWARD_NOOP,
    RELOAD_NOOP,
    OPEN_CLEARS_FORWARD,
    BACK_MOVES,
    FORWARD_MOVES,
    RELOAD_KEEPS_HISTORY,
    SUPERSEDE_MARKS_STALE,
    STALE_IGNORED,
    SHOWN_ONLY_ON_RESULT,
    SUCCESS_COMMITS_PAGE,
    FAILURE_KEEPS_PLACE,
    RESULT_KEEPS_STALE,
    CAPABILITY_MATCHES_HISTORY,
    START_IS_EMPTY,
    INDEX_IN_RANGE,
    REACH_OPEN,
    REACH_BACK_BLOCKED,
    REACH_BACK,
    REACH_FORWARD_BLOCKED,
    REACH_FORWARD,
    REACH_RELOAD_EMPTY,
    REACH_RELOAD,
    REACH_SUCCESS,
    REACH_FAILURE,
    REACH_STALE,
    REACH_STALE_AFTER_RESULT,
    REACH_MIDDLE_DONE,
    REACH_SUPERSEDE,
];

fn step(state: &State) -> Option<&crate::Step> {
    state.step.as_ref()
}

fn untouched(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    state.diagram == step.before && state.stale == step.before_stale
}

pub(crate) fn back_noop(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if step.action != Action::Back || step.before.can_back() {
        return true;
    }
    untouched(state)
}

pub(crate) fn forward_noop(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if step.action != Action::Forward || step.before.can_forward() {
        return true;
    }
    untouched(state)
}

pub(crate) fn reload_noop(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if step.action != Action::Reload || step.before.has_page() {
        return true;
    }
    untouched(state)
}

pub(crate) fn open_clears_forward(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    let Action::Open(url) = step.action else {
        return true;
    };
    let Some(kept) = prefix(&step.before) else {
        return false;
    };
    let mut expected = kept.to_vec();
    expected.push(url);
    let after = &state.diagram;
    after.entries == expected
        && usize::from(after.index) + 1 == after.entries.len()
        && after.phase == Phase::Loading
}

fn prefix(before: &DiagramState) -> Option<&[diagram::Url]> {
    if before.entries.is_empty() {
        Some(&[])
    } else {
        before.entries.get(..=usize::from(before.index))
    }
}

pub(crate) fn back_moves(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if step.action != Action::Back || !step.before.can_back() {
        return true;
    }
    let after = &state.diagram;
    after.entries == step.before.entries
        && after.index + 1 == step.before.index
        && after.phase == Phase::Loading
}

pub(crate) fn forward_moves(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if step.action != Action::Forward || !step.before.can_forward() {
        return true;
    }
    let after = &state.diagram;
    after.entries == step.before.entries
        && after.index == step.before.index + 1
        && after.phase == Phase::Loading
}

pub(crate) fn reload_keeps_history(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if step.action != Action::Reload || !step.before.has_page() {
        return true;
    }
    let after = &state.diagram;
    after.entries == step.before.entries
        && after.index == step.before.index
        && after.phase == Phase::Loading
}

pub(crate) fn supersede_marks_stale(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if !diagram::accepted(&step.before, step.action) {
        return true;
    }
    let expect = step.before_stale || step.before.phase == Phase::Loading;
    state.stale == expect
}

pub(crate) fn stale_ignored(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if step.action != Action::Stale {
        return true;
    }
    state.diagram == step.before && step.before_stale && !state.stale
}

pub(crate) fn shown_only_on_result(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if matches!(step.action, Action::Succeed | Action::Fail) {
        return true;
    }
    state.diagram.shown == step.before.shown
}

pub(crate) fn success_commits_page(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if step.action != Action::Succeed {
        return true;
    }
    let Some(url) = step.before.current() else {
        return false;
    };
    let after = &state.diagram;
    after.entries == step.before.entries
        && after.index == step.before.index
        && after.phase == Phase::Done
        && after.shown == Shown::Page(url)
}

pub(crate) fn failure_keeps_place(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if step.action != Action::Fail {
        return true;
    }
    let Some(url) = step.before.current() else {
        return false;
    };
    let after = &state.diagram;
    after.entries == step.before.entries
        && after.index == step.before.index
        && after.phase == Phase::Failed
        && after.shown == Shown::Error(url)
}

pub(crate) fn result_keeps_stale(state: &State) -> bool {
    let Some(step) = step(state) else {
        return true;
    };
    if !matches!(step.action, Action::Succeed | Action::Fail) {
        return true;
    }
    state.stale == step.before_stale
}

pub(crate) fn capability_matches_history(state: &State) -> bool {
    let diagram = &state.diagram;
    let back = !diagram.entries.is_empty() && diagram.index > 0;
    let forward = usize::from(diagram.index) + 1 < diagram.entries.len();
    diagram.can_back() == back && diagram.can_forward() == forward
}

pub(crate) fn start_is_empty(state: &State) -> bool {
    let diagram = &state.diagram;
    let start = diagram.phase == Phase::Start;
    let empty = diagram.entries.is_empty();
    if start {
        empty && diagram.shown == Shown::Empty && diagram.index == 0
    } else {
        !empty
    }
}

pub(crate) fn index_in_range(state: &State) -> bool {
    let diagram = &state.diagram;
    if diagram.entries.is_empty() {
        diagram.index == 0
    } else {
        usize::from(diagram.index) < diagram.entries.len() && diagram.entries.len() <= MAX_ENTRIES
    }
}

pub(crate) fn reach_open(state: &State) -> bool {
    matches!(
        state.step.as_ref().map(|step| step.action),
        Some(Action::Open(_))
    )
}

pub(crate) fn reach_back_blocked(state: &State) -> bool {
    state
        .step
        .as_ref()
        .is_some_and(|step| step.action == Action::Back && !step.before.can_back())
}

pub(crate) fn reach_back(state: &State) -> bool {
    state
        .step
        .as_ref()
        .is_some_and(|step| step.action == Action::Back && step.before.can_back())
}

pub(crate) fn reach_forward_blocked(state: &State) -> bool {
    state
        .step
        .as_ref()
        .is_some_and(|step| step.action == Action::Forward && !step.before.can_forward())
}

pub(crate) fn reach_forward(state: &State) -> bool {
    state
        .step
        .as_ref()
        .is_some_and(|step| step.action == Action::Forward && step.before.can_forward())
}

pub(crate) fn reach_reload_empty(state: &State) -> bool {
    state
        .step
        .as_ref()
        .is_some_and(|step| step.action == Action::Reload && !step.before.has_page())
}

pub(crate) fn reach_reload(state: &State) -> bool {
    state
        .step
        .as_ref()
        .is_some_and(|step| step.action == Action::Reload && step.before.has_page())
}

pub(crate) fn reach_success(state: &State) -> bool {
    matches!(
        state.step.as_ref().map(|step| step.action),
        Some(Action::Succeed)
    )
}

pub(crate) fn reach_failure(state: &State) -> bool {
    matches!(
        state.step.as_ref().map(|step| step.action),
        Some(Action::Fail)
    )
}

pub(crate) fn reach_stale(state: &State) -> bool {
    matches!(
        state.step.as_ref().map(|step| step.action),
        Some(Action::Stale)
    )
}

pub(crate) fn reach_stale_after_result(state: &State) -> bool {
    state.step.as_ref().is_some_and(|step| {
        step.action == Action::Stale && matches!(step.before.phase, Phase::Done | Phase::Failed)
    })
}

pub(crate) fn reach_middle_done(state: &State) -> bool {
    let diagram = &state.diagram;
    diagram.phase == Phase::Done && diagram.can_back() && diagram.can_forward()
}

pub(crate) fn reach_supersede(state: &State) -> bool {
    state.step.as_ref().is_some_and(|step| {
        step.before.phase == Phase::Loading && diagram::accepted(&step.before, step.action)
    })
}
